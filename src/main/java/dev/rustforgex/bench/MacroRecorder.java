package dev.rustforgex.bench;

import dev.rustforgex.RfxRuntime;
import dev.rustforgex.command.DiscoveryReport;
import dev.rustforgex.instrument.ProbeRegistry;
import dev.rustforgex.instrument.ProbeSink;
import dev.rustforgex.instrument.RfxProbes;
import dev.rustforgex.forge.EventDispatchTable;
import dev.rustforgex.forge.EventObserver;
import net.minecraft.server.MinecraftServer;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.server.ServerLifecycleHooks;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.lang.management.GarbageCollectorMXBean;
import java.lang.management.ManagementFactory;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * C-36, niveau B : enregistrement d'une exécution de macro-benchmark.
 *
 * <p>Cahier des charges : PARTIE 21. Exigences : R-580 et R-860 (aucun chiffre publié
 * qui ne vienne d'un fichier généré), R-581 (jamais de mesure sans dispersion ni
 * nombre de répétitions). Maturité : {@code STABLE}.
 *
 * <p>Mesure le temps de tick d'un serveur dédié, sur une fenêtre déclarée, puis écrit
 * un fichier d'exécution et arrête proprement le serveur. Une exécution ne prouve
 * rien à elle seule : c'est {@code rfx-bench macro} qui agrège plusieurs exécutions
 * indépendantes et rend la médiane et l'écart interquartile qu'exige la PARTIE 21.3.
 *
 * <h2>Armé sur demande, absent sinon</h2>
 *
 * <p>Sans la propriété {@code rustforgex.bench.ticks}, cette classe ne s'abonne à
 * rien et n'existe qu'à l'état de code mort. C'est la règle du projet : on n'installe
 * une accroche que si elle fait quelque chose, et un enregistreur de benchmark ne fait
 * quelque chose que pendant un benchmark.
 *
 * <h2>Indépendant du runtime</h2>
 *
 * <p>Il mesure aussi bien avec RUSTFORGE-X actif qu'avec {@code general.enabled=false}.
 * C'est indispensable : la seule façon de connaître le coût du runtime est de comparer
 * les deux, et un instrument qui disparaît avec ce qu'il mesure ne mesure rien.
 *
 * <h2>Ce qu'il ne fait pas</h2>
 *
 * <p>Il n'alloue rien par tick — les durées vont dans un tableau dimensionné à
 * l'armement — et ne lève jamais. Un défaut de l'instrument ne doit pas se confondre
 * avec un défaut de ce qu'il mesure.
 */
public final class MacroRecorder {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Nombre de ticks mesurés. Sans cette propriété, l'enregistreur reste absent. */
    public static final String PROPERTY_TICKS = "rustforgex.bench.ticks";

    /** Nombre de ticks d'échauffement ignorés (PARTIE 21.3, point 3). */
    public static final String PROPERTY_WARMUP = "rustforgex.bench.warmup";

    /** Chemin du fichier d'exécution à écrire. */
    public static final String PROPERTY_OUT = "rustforgex.bench.out";

    /** Étiquette de configuration comparée, par exemple {@code rfx-on}. */
    public static final String PROPERTY_LABEL = "rustforgex.bench.label";

    /** Numéro de cette exécution parmi les répétitions. */
    public static final String PROPERTY_RUN = "rustforgex.bench.run";

    /**
     * Durée de chronométrage à ouvrir sur la fenêtre mesurée, en secondes. Zéro : aucun.
     *
     * <p>Les niveaux {@code COUNTER} ne produisent que des décomptes : sans fenêtre
     * chronométrée, aucun coût n'est attribué aux unités de travail, et la part du tick
     * expliquée vaut zéro pour une raison qui n'a rien à voir avec la couverture.
     *
     * <p>Éteint par défaut, et c'est délibéré : le chronométrage coûte, et l'allumer
     * partout changerait le sens de toutes les campagnes de coût déjà étalonnées.
     */
    public static final String PROPERTY_PROFILE_SECONDS = "rustforgex.bench.profile_seconds";

    /** Version du schéma du fichier d'exécution. */
    private static final int SCHEMA = 1;

    /** Nanosecondes par milliseconde. */
    private static final double NANOS_PER_MS = 1_000_000.0;

    private final int warmupTicks;
    private final int measuredTicks;
    private final Path out;
    private final String label;
    private final int run;

    /** Durées de tick mesurées, en nanosecondes. Dimensionné une fois, à l'armement. */
    private final long[] durations;

    /** Ticks pendant lesquels le compteur de ramasse-miettes a bougé (PARTIE 21.3.8). */
    private final boolean[] collected;

    /** Tick auquel le profil de charge est appliqué, bien avant la fenêtre de mesure. */
    private static final int LOAD_AT_TICK = 20;

    private int seen;
    private int recorded;
    private boolean loadApplied;

    /** Commandes de charge restant à exécuter, ou {@code null} s'il n'y en a pas. */
    private java.util.Deque<String> pendingLoad;

    /** Commandes de charge effectivement exécutées. */
    private int loadCommands;
    private long tickStartNs;
    private long windowStartNs;
    private long gcCountAtStart;
    private long gcTimeAtStartMs;
    private long lastGcCount;
    private boolean finished;

    private MacroRecorder(int warmupTicks, int measuredTicks, Path out, String label, int run) {
        this.warmupTicks = warmupTicks;
        this.measuredTicks = measuredTicks;
        this.out = out;
        this.label = label;
        this.run = run;
        this.durations = new long[measuredTicks];
        this.collected = new boolean[measuredTicks];
    }

    /**
     * Arme l'enregistreur si les propriétés le demandent.
     *
     * <p>Appelée à la construction du mod. En l'absence de {@link #PROPERTY_TICKS},
     * elle ne fait rien et ne s'abonne à aucun événement.
     */
    public static void armIfRequested() {
        int ticks = intProperty(PROPERTY_TICKS, 0);
        if (ticks <= 0) {
            return;
        }
        int warmup = intProperty(PROPERTY_WARMUP, 0);
        String label = System.getProperty(PROPERTY_LABEL, "unlabelled");
        int run = intProperty(PROPERTY_RUN, 1);
        Path out = Path.of(System.getProperty(
                PROPERTY_OUT, "benchmarks/runs/" + label + "-" + run + ".json"));

        MacroRecorder recorder = new MacroRecorder(warmup, ticks, out, label, run);
        MinecraftForge.EVENT_BUS.register(recorder);
        LOGGER.info(
                "Macro-benchmark armé : {} ticks d'échauffement puis {} ticks mesurés, "
                        + "configuration « {} », exécution {}. Le serveur s'arrêtera ensuite.",
                warmup, ticks, label, run);
    }

    /**
     * Ouvre une fenêtre de chronométrage sur toute la fenêtre mesurée, si demandée.
     *
     * <p>Ici et pas à l'armement : ouvrir pendant l'échauffement ferait expirer la
     * demande avant que la mesure commence, et mesurer un chronométrage qui s'est éteint
     * en route ne dirait rien de lisible.
     */
    private void openProfilingWindow() {
        int seconds = intProperty(PROPERTY_PROFILE_SECONDS, 0);
        if (seconds <= 0) {
            return;
        }
        RfxRuntime runtime = RfxRuntime.instance();
        boolean opened = runtime != null && runtime.requestProfilingDepth(seconds);
        LOGGER.info("Fenêtre de chronométrage de {} s sur la fenêtre mesurée : {}.",
                seconds, opened ? "ouverte" : "refusée");
    }

    /** Ouvre le chronomètre du tick, avant tout autre travail. */
    @SubscribeEvent(priority = EventPriority.HIGHEST)
    public void onTickStart(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.START || finished) {
            return;
        }
        tickStartNs = System.nanoTime();
    }

    /** Ferme le chronomètre du tick, après tout autre travail. */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void onTickEnd(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.END || finished || tickStartNs == 0) {
            return;
        }
        long elapsed = System.nanoTime() - tickStartNs;
        tickStartNs = 0;
        seen++;

        // La charge est posée tôt dans l'échauffement : elle provoque un pic — des
        // centaines de commandes, des entités créées — qui n'a rien à faire dans la
        // fenêtre mesurée, et le serveur a ensuite tout l'échauffement pour se stabiliser.
        if (!loadApplied && seen == LOAD_AT_TICK) {
            loadApplied = true;
            pendingLoad = queueLoadProfile();
        }
        if (pendingLoad != null && !pendingLoad.isEmpty()) {
            // Étalé sur plusieurs ticks : huit mille commandes d'un bloc feraient durer
            // un tick des dizaines de secondes, et le chien de garde du serveur y
            // verrait un blocage.
            pumpLoadProfile();
        }

        if (seen <= warmupTicks) {
            if (seen == warmupTicks) {
                // L'échauffement s'achève : c'est ici, et pas avant, que commencent la
                // fenêtre de mesure et le relevé du ramasse-miettes.
                windowStartNs = System.nanoTime();
                gcCountAtStart = gcCount();
                gcTimeAtStartMs = gcTimeMs();
                lastGcCount = gcCountAtStart;
                openProfilingWindow();
            }
            return;
        }

        if (recorded < durations.length) {
            long gcNow = gcCount();
            durations[recorded] = elapsed;
            // Un tick contaminé par une collecte est marqué, jamais supprimé
            // (PARTIE 21.3, point 8) : le supprimer embellirait la mesure.
            collected[recorded] = gcNow != lastGcCount;
            lastGcCount = gcNow;
            recorded++;
        }

        if (recorded >= durations.length) {
            finished = true;
            finish();
        }
    }

    /**
     * Applique le profil de charge demandé, s'il y en a un.
     *
     * <p>Ne lève jamais : un profil qui échoue laisse un serveur au repos, ce que le
     * fichier d'exécution dira, plutôt qu'un benchmark interrompu.
     */
    private java.util.Deque<String> queueLoadProfile() {
        String profile = LoadProfile.requested();
        if (LoadProfile.NONE.equals(profile)) {
            return null;
        }
        java.util.List<String> commands = LoadProfile.commandsFor(profile);
        if (commands.isEmpty()) {
            return null;
        }
        LOGGER.info("Profil de charge « {} » : {} commandes, étalées sur {} par tick.",
                profile, commands.size(), LoadProfile.COMMANDS_PER_TICK);
        return new java.util.ArrayDeque<>(commands);
    }

    /** Exécute la tranche de commandes du tick courant. */
    private void pumpLoadProfile() {
        try {
            MinecraftServer server = ServerLifecycleHooks.getCurrentServer();
            if (server == null) {
                LOGGER.warn("Profil de charge ignoré : aucun serveur courant.");
                pendingLoad = null;
                return;
            }
            loadCommands += LoadProfile.pump(server, pendingLoad);
            if (pendingLoad.isEmpty()) {
                LOGGER.info("Profil de charge appliqué : {} commandes exécutées.", loadCommands);
            }
        } catch (RuntimeException | LinkageError e) {
            LOGGER.warn("Profil de charge interrompu. La mesure décrira un serveur moins "
                    + "chargé que demandé.", e);
            pendingLoad = null;
        }
    }

    /** Écrit le fichier d'exécution puis demande l'arrêt du serveur. */
    private void finish() {
        long windowNs = System.nanoTime() - windowStartNs;
        try {
            write(windowNs);
            LOGGER.info("Macro-benchmark terminé : {} ticks mesurés, résultat dans {}",
                    recorded, out.toAbsolutePath());
        } catch (IOException | RuntimeException e) {
            // Sans fichier, l'exécution n'a rien produit : le dire fort, et ne surtout
            // pas laisser l'agrégateur croire à une exécution valide manquante.
            LOGGER.error("Macro-benchmark : écriture de {} impossible, exécution perdue",
                    out.toAbsolutePath(), e);
        }
        MinecraftForge.EVENT_BUS.unregister(this);
        var server = ServerLifecycleHooks.getCurrentServer();
        if (server != null) {
            server.halt(false);
        }
    }

    /** Compose et écrit le fichier d'exécution. */
    private void write(long windowNs) throws IOException {
        long[] sorted = new long[recorded];
        System.arraycopy(durations, 0, sorted, 0, recorded);
        java.util.Arrays.sort(sorted);

        int contaminated = 0;
        for (int i = 0; i < recorded; i++) {
            if (collected[i]) {
                contaminated++;
            }
        }

        double windowSeconds = windowNs / 1_000_000_000.0;
        double tps = windowSeconds > 0 ? recorded / windowSeconds : 0.0;

        StringBuilder json = new StringBuilder(2048);
        json.append("{\n");
        json.append("  \"schema\": ").append(SCHEMA).append(",\n");
        json.append("  \"label\": \"").append(escape(label)).append("\",\n");
        json.append("  \"run\": ").append(run).append(",\n");
        // Sans lui, on ne saurait pas si un résultat décrit un serveur au repos ou un
        // serveur sous charge — deux régimes que rien ne permet de comparer.
        json.append("  \"load\": \"").append(escape(LoadProfile.requested())).append("\",\n");
        json.append("  \"warmup_ticks\": ").append(warmupTicks).append(",\n");
        json.append("  \"measured_ticks\": ").append(recorded).append(",\n");
        json.append("  \"window_seconds\": ").append(format(windowSeconds)).append(",\n");
        json.append("  \"jvm\": {\n");
        json.append("    \"vendor\": \"").append(escape(System.getProperty("java.vendor", "?")))
                .append("\",\n");
        json.append("    \"version\": \"").append(escape(System.getProperty("java.version", "?")))
                .append("\",\n");
        json.append("    \"flags\": [").append(jvmFlags()).append("]\n");
        json.append("  },\n");
        json.append("  \"metrics\": {\n");
        json.append("    \"mspt_p50\": ").append(format(percentileMs(sorted, 0.50))).append(",\n");
        json.append("    \"mspt_p95\": ").append(format(percentileMs(sorted, 0.95))).append(",\n");
        json.append("    \"mspt_p99\": ").append(format(percentileMs(sorted, 0.99))).append(",\n");
        json.append("    \"mspt_max\": ").append(format(percentileMs(sorted, 1.00))).append(",\n");
        json.append("    \"tps\": ").append(format(tps)).append(",\n");
        json.append("    \"gc_collections\": ").append(gcCount() - gcCountAtStart).append(",\n");
        json.append("    \"gc_time_ms\": ").append(gcTimeMs() - gcTimeAtStartMs).append(",\n");
        json.append("    \"gc_contaminated_ticks\": ").append(contaminated).append("\n");
        json.append("  },\n");
        json.append("  \"rfx\": {\n").append(runtimeState()).append("  }\n");
        json.append("}\n");

        Path parent = out.toAbsolutePath().getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.writeString(out, json.toString(), StandardCharsets.UTF_8);
    }

    /**
     * État de RUSTFORGE-X pendant la mesure.
     *
     * <p>Sans lui, deux fichiers d'exécution seraient indiscernables et la comparaison
     * ne voudrait rien dire : c'est précisément ce qui distingue une configuration
     * d'une autre.
     */
    private static String runtimeState() {
        RfxRuntime runtime = RfxRuntime.instance();
        if (runtime == null) {
            return "    \"active\": false,\n    \"reason\": \"mod non démarré\"\n";
        }
        StringBuilder state = new StringBuilder(256);
        state.append("    \"active\": ").append(runtime.active()).append(",\n");
        state.append("    \"mode\": \"")
                .append(escape(runtime.configuration().mode())).append("\",\n");
        state.append("    \"instrumentation\": \"")
                .append(runtime.instrumentation().state()).append("\",\n");
        state.append("    \"methods_probed\": ")
                .append(runtime.instrumentation().methodsProbed());
        appendProbeActivity(state);
        appendThreshold(state, runtime);
        appendProfilerLevel(state, runtime);
        appendDiscovery(state, runtime);
        appendTop(state, runtime);
        appendProbeThreads(state, runtime);
        appendEvents(state, runtime);
        appendBaseline(state, runtime);
        state.append("\n");
        return state.toString();
    }

    /**
     * Ajoute ce que le profiler dit de son propre coût (PARTIE 12.4).
     *
     * <p>La campagne mesure le coût de l'extérieur, en comparant deux configurations ;
     * le profiler le mesure de l'intérieur, en s'éteignant vingt ticks. Les deux
     * doivent converger, et s'ils divergent, l'un des deux ment. Les consigner ensemble
     * est la seule façon de s'en apercevoir.
     *
     * <p>Rien n'est écrit si aucune pause n'a abouti : une exécution plus courte que la
     * période de mesure n'a pas de ligne de base, et écrire un zéro le laisserait croire.
     */
    private static void appendBaseline(StringBuilder state, RfxRuntime runtime) {
        Map<String, Object> profiler = runtime.profilerCounters();
        if (profiler == null) {
            return;
        }
        long measurements = longValue(profiler, "baseline_measurements");
        if (measurements == 0) {
            return;
        }
        state.append(",\n    \"baseline\": {\n");
        state.append("      \"measurements\": ").append(measurements).append(",\n");
        state.append("      \"overhead_ns\": ")
                .append(longValue(profiler, "baseline_overhead_ns")).append(",\n");
        state.append("      \"overhead_pct\": ")
                .append(format(longValue(profiler, "baseline_overhead_pct_x100") / 100.0))
                .append(",\n");
        state.append("      \"cycles\": ")
                .append(longValue(profiler, "baseline_cycles")).append(",\n");
        // La dispersion des cycles, sans laquelle la médiane ne dit pas si elle résume
        // un signal ou du bruit. Le profilage ne peut pas rendre un tick plus rapide :
        // une proportion de cycles positifs proche de la moitié signale que la mesure
        // ne mesure rien.
        state.append("      \"delta_min_ns\": ")
                .append(longValue(profiler, "baseline_delta_min_ns")).append(",\n");
        state.append("      \"delta_max_ns\": ")
                .append(longValue(profiler, "baseline_delta_max_ns")).append(",\n");
        state.append("      \"positive_cycles\": ")
                .append(longValue(profiler, "baseline_positive_cycles")).append(",\n");
        state.append("      \"tick\": ")
                .append(longValue(profiler, "baseline_tick")).append("\n");
        state.append("    }");
    }

    /**
     * Ajoute la profondeur de sondage en vigueur à la fin de la mesure.
     *
     * <p>Sans elle, deux exécutions au même coût seraient indiscernables alors que
     * l'une sonde tout et l'autre plus rien : c'est l'auto-mesure qui fait descendre
     * cette profondeur, et son effet doit se lire dans le résultat.
     */
    /**
     * Consigne ce que les sondes font réellement, et pas seulement combien il y en a.
     *
     * <p>Sans ces trois nombres, une campagne ne distingue pas un système qui sonde
     * d'un système dont les sondes sont éteintes. C'est arrivé : cinq campagnes ont
     * mesuré un runtime annonçant « 2 647 méthodes sondées » alors que la table des
     * niveaux était vide et le puits absent. Le coût mesuré était celui de sondes qui
     * sortaient à leur première ligne, et rien ne le disait.
     *
     * <p>{@code probes_armed} à zéro alors que {@code methods_probed} est élevé est le
     * signe exact de cette panne, et il est désormais dans chaque fichier de campagne.
     */
    private static void appendProbeActivity(StringBuilder state) {
        state.append(",\n    \"probes_in_table\": ").append(RfxProbes.probeCount());
        state.append(",\n    \"probes_armed\": ").append(RfxProbes.armedCount());
        state.append(",\n    \"probe_sink_installed\": ").append(RfxProbes.active());
        state.append(",\n    \"probe_records_failed\": ").append(RfxProbes.failedRecords());
    }

    /**
     * Consigne le seuil de sondage et ce qu'il écarte (ADR-021).
     *
     * <p>Sans ces trois nombres, une campagne compare des surfaces sondées sans savoir
     * quel réglage les a produites, ni combien de méthodes le seuil a refusées. Deux
     * campagnes à seuils différents seraient indiscernables dans leurs fichiers.
     */
    private static void appendThreshold(StringBuilder state, RfxRuntime runtime) {
        state.append(",\n    \"min_instructions\": ")
                .append(runtime.instrumentation().minInstructions());
        state.append(",\n    \"refused_by_threshold\": ")
                .append(runtime.instrumentation().refusedByThreshold());
        state.append(",\n    \"refused_under_spec\": ")
                .append(runtime.instrumentation().refusedUnderSpec());
    }

    /**
     * Nombre de méthodes chaudes non sondées consignées dans le fichier de campagne.
     *
     * <p>Assez pour qu'une décision de sondage s'y lise, pas au point de rendre le
     * fichier illisible : au-delà de quelques dizaines, la traîne ne se distingue plus
     * du bruit d'échantillonnage.
     */
    private static final int DISCOVERY_ROWS = 40;

    /**
     * Consigne ce que l'échantillonneur a vu s'exécuter sans sonde (C-05, ADR-027).
     *
     * <p>C'est la mesure du <strong>trou de couverture</strong>. Trois campagnes ont
     * établi qu'on ne le comble pas en armant plus de sondes ; celle-ci dit ce qui reste
     * dehors, et lesquelles il faudrait choisir.
     *
     * <p>Le classement est écrit avec le nombre d'échantillons de chaque méthode, jamais
     * la part seule : cent prélèvements par seconde font qu'une méthode vue trois fois
     * est un indice, pas une mesure, et seul le décompte permet d'en juger (R-660).
     */
    private static void appendDiscovery(StringBuilder state, RfxRuntime runtime) {
        DiscoveryReport.Census census = runtime.unknownFrames(DISCOVERY_ROWS);
        if (census == null) {
            return;
        }
        state.append(",\n    \"discovery\": {\n");
        state.append("      \"samples_taken\": ").append(census.samplesTaken()).append(",\n");
        state.append("      \"unknown_samples\": ").append(census.recorded()).append(",\n");
        state.append("      \"unknown_frames\": ").append(census.distinct()).append(",\n");
        state.append("      \"unknown_frames_dropped\": ")
                .append(census.distinctDropped()).append(",\n");
        state.append("      \"top\": [");
        boolean first = true;
        for (DiscoveryReport.Row row : census.rows()) {
            if (row.samples() <= 0L) {
                break;
            }
            state.append(first ? "\n" : ",\n");
            first = false;
            state.append("        {\"owner\": \"").append(escape(row.owner()))
                    .append("\", \"method\": \"").append(escape(row.label()))
                    .append("\", \"samples\": ").append(row.samples()).append('}');
        }
        state.append(first ? "]\n" : "\n      ]\n");
        state.append("    }");
    }

    /**
     * Consigne quels threads ont produit des enregistrements de sonde, et lesquels
     * partent réellement au profileur.
     *
     * <p>Le vidage a lieu à la clôture du tick, donc sur le seul thread autoritatif, et
     * ne concerne que son propre tampon. Un thread à zéro vidage est du travail sondé
     * qui n'arrive nulle part — et aucune campagne ne pouvait le voir jusqu'ici.
     */
    private static void appendProbeThreads(StringBuilder state, RfxRuntime runtime) {
        ProbeSink sink = runtime.probeSink();
        if (sink == null) {
            return;
        }
        List<ProbeSink.ThreadUsage> threads = sink.threads();
        if (threads.isEmpty()) {
            return;
        }
        state.append(",\n    \"probe_threads\": [");
        boolean first = true;
        for (ProbeSink.ThreadUsage thread : threads) {
            state.append(first ? "\n" : ",\n");
            first = false;
            state.append("      {\"name\": \"").append(escape(thread.name()))
                    .append("\", \"flushes\": ").append(thread.flushes())
                    .append(", \"pending_passes\": ").append(thread.pendingPasses())
                    .append(", \"dropped\": ").append(thread.dropped()).append('}');
        }
        state.append("\n    ]");
    }

    /**
     * Unités de travail demandées au natif pour calculer la part expliquée.
     *
     * <p>Au-dessus du plafond d'unités suivies : un classement tronqué donnerait une
     * somme tronquée, donc une part sous-estimée sans qu'on puisse le voir.
     */
    private static final int TOP_QUERY_LIMIT = 8_192;

    /** Unités détaillées dans le fichier. Le reste n'entre que dans la somme. */
    private static final int TOP_ROWS = 25;

    /**
     * Consigne le classement des unités et, surtout, la part du tick qu'il explique.
     *
     * <p>C'est le chiffre que quatre décisions successives cherchaient sans pouvoir le
     * produire : tant que la collecte ne ramassait qu'un thread sur dix-huit (ADR-028),
     * il ne mesurait pas la couverture mais l'étendue du défaut.
     *
     * <p>La somme porte sur <strong>toutes</strong> les unités suivies, pas sur les
     * lignes détaillées : additionner un classement tronqué sous-estimerait la part sans
     * que rien ne le signale (R-660).
     */
    private static void appendTop(StringBuilder state, RfxRuntime runtime) {
        Map<String, Object> top = runtime.topWorkloads(TOP_QUERY_LIMIT);
        if (top == null) {
            return;
        }
        long tickNs = longValue(top, "tick_ns");
        Object rawEntries = top.get("entries");
        List<?> entries = rawEntries instanceof List<?> list ? list : List.of();

        long explained = 0L;
        for (Object element : entries) {
            if (element instanceof Map<?, ?> entry) {
                explained += longValue(castEntry(entry), "cost_ns_per_tick");
            }
        }

        state.append(",\n    \"top\": {\n");
        state.append("      \"tracked\": ").append(longValue(top, "tracked")).append(",\n");
        state.append("      \"measured\": ").append(longValue(top, "measured")).append(",\n");
        state.append("      \"tick_ns\": ").append(tickNs).append(",\n");
        state.append("      \"explained_ns\": ").append(explained).append(",\n");
        // Sans tick connu, la part n'est pas calculable : -1 le dit, la 0 le tairait
        // en se faisant passer pour une mesure (R-660).
        state.append("      \"explained_pct_x100\": ")
                .append(tickNs == 0 ? -1L : explained * 10_000L / tickNs).append(",\n");
        state.append("      \"entries\": [");
        appendTopRows(state, entries, runtime);
        state.append("    }");
    }

    /** Détaille les premières unités du classement, nom et mod propriétaire compris. */
    private static void appendTopRows(StringBuilder state, List<?> entries, RfxRuntime runtime) {
        ProbeRegistry registry = runtime.instrumentation() == null
                ? null : runtime.instrumentation().registry();
        boolean first = true;
        int rows = 0;
        for (Object element : entries) {
            if (rows >= TOP_ROWS || !(element instanceof Map<?, ?> raw)) {
                break;
            }
            Map<String, Object> entry = castEntry(raw);
            long cost = longValue(entry, "cost_ns_per_tick");
            if (cost <= 0L) {
                // Le classement est décroissant : la première unité sans coût annonce
                // que toutes les suivantes n'en ont pas non plus.
                break;
            }
            int probeId = (int) longValue(entry, "probe_id");
            String name = registry == null ? null : registry.nameOf(probeId);
            String owner = registry == null ? null : registry.ownerOf(probeId);
            state.append(first ? "\n" : ",\n");
            first = false;
            rows++;
            state.append("        {\"owner\": \"").append(escape(owner == null ? "?" : owner))
                    .append("\", \"method\": \"").append(escape(name == null ? "sonde#" + probeId : name))
                    .append("\", \"cost_ns_per_tick\": ").append(cost)
                    .append(", \"source\": \"").append(escape(stringValue(entry, "source")))
                    .append("\"}");
        }
        state.append(first ? "]\n" : "\n      ]\n");
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> castEntry(Map<?, ?> entry) {
        return (Map<String, Object>) entry;
    }

    /** Lit une chaîne des tables décodées, ou {@code "?"} si elle est absente. */
    private static String stringValue(Map<String, Object> table, String key) {
        Object value = table == null ? null : table.get(key);
        return value instanceof String text ? text : "?";
    }

    private static void appendProfilerLevel(StringBuilder state, RfxRuntime runtime) {
        Map<String, Object> profiler = runtime.profilerCounters();
        if (profiler == null) {
            return;
        }
        Object level = profiler.get("level");
        if (level instanceof String text) {
            state.append(",\n    \"profiler_level\": \"").append(escape(text)).append('"');
        }
    }

    /**
     * Ajoute ce que l'observateur du bus a vu passer (C-06, étape 1).
     *
     * <p>Sans ces compteurs, on ne saurait pas distinguer un observateur qui n'a rien vu
     * d'un observateur absent — et le coût de l'observation resterait invérifiable.
     */
    private static void appendEvents(StringBuilder state, RfxRuntime runtime) {
        EventObserver observer = runtime.eventObserver();
        if (observer == null || !observer.observing()) {
            return;
        }
        EventDispatchTable table = observer.table();
        state.append(",\n    \"events\": {\n");
        state.append("      \"dispatched\": ").append(table.dispatched()).append(",\n");
        state.append("      \"types\": ").append(table.knownTypes()).append(",\n");
        state.append("      \"timed\": ").append(table.timed()).append(",\n");
        state.append("      \"abandoned\": ").append(table.abandoned()).append("\n");
        state.append("    }");
    }

    /** Lit un entier des compteurs natifs, ou zéro s'il est absent ou d'un autre type. */
    private static long longValue(Map<String, Object> counters, String key) {
        Object value = counters.get(key);
        return value instanceof Number number ? number.longValue() : 0L;
    }

    /** Centile d'un tableau trié, exprimé en millisecondes. */
    private static double percentileMs(long[] sorted, double quantile) {
        if (sorted.length == 0) {
            return 0.0;
        }
        int index = (int) Math.round(quantile * (sorted.length - 1));
        return sorted[Math.max(0, Math.min(sorted.length - 1, index))] / NANOS_PER_MS;
    }

    /** Collectes cumulées, tous ramasse-miettes confondus. */
    private static long gcCount() {
        long total = 0;
        for (GarbageCollectorMXBean bean : ManagementFactory.getGarbageCollectorMXBeans()) {
            long count = bean.getCollectionCount();
            if (count > 0) {
                total += count;
            }
        }
        return total;
    }

    /** Temps cumulé de collecte, en millisecondes. */
    private static long gcTimeMs() {
        long total = 0;
        for (GarbageCollectorMXBean bean : ManagementFactory.getGarbageCollectorMXBeans()) {
            long time = bean.getCollectionTime();
            if (time > 0) {
                total += time;
            }
        }
        return total;
    }

    /** Arguments de la JVM, cités dans le résultat (PARTIE 21.3, point 9). */
    private static String jvmFlags() {
        List<String> quoted = new ArrayList<>();
        for (String argument : ManagementFactory.getRuntimeMXBean().getInputArguments()) {
            // Les propriétés du benchmark lui-même ne décrivent pas la JVM mesurée.
            if (argument.startsWith("-Drustforgex.bench.")) {
                continue;
            }
            quoted.add("\"" + escape(argument) + "\"");
        }
        return String.join(", ", quoted);
    }

    /** Nombre décimal à trois chiffres après la virgule, en notation JSON. */
    private static String format(double value) {
        return String.format(java.util.Locale.ROOT, "%.3f", value);
    }

    /** Échappe les caractères qui ne peuvent pas figurer tels quels dans une chaîne JSON. */
    private static String escape(String value) {
        return value.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    /** Lit une propriété système entière, ou rend la valeur par défaut. */
    private static int intProperty(String name, int fallback) {
        String raw = System.getProperty(name);
        if (raw == null || raw.isBlank()) {
            return fallback;
        }
        try {
            return Integer.parseInt(raw.trim());
        } catch (NumberFormatException e) {
            LOGGER.warn("Macro-benchmark : « {} » n'est pas un entier pour {}, {} retenu",
                    raw, name, fallback);
            return fallback;
        }
    }
}
