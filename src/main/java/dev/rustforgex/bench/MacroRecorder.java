package dev.rustforgex.bench;

import dev.rustforgex.RfxRuntime;
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

    private int seen;
    private int recorded;
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

        if (seen <= warmupTicks) {
            if (seen == warmupTicks) {
                // L'échauffement s'achève : c'est ici, et pas avant, que commencent la
                // fenêtre de mesure et le relevé du ramasse-miettes.
                windowStartNs = System.nanoTime();
                gcCountAtStart = gcCount();
                gcTimeAtStartMs = gcTimeMs();
                lastGcCount = gcCountAtStart;
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
                .append(runtime.instrumentation().methodsProbed()).append("\n");
        return state.toString();
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
