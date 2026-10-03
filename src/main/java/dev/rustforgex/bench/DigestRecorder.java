package dev.rustforgex.bench;

import dev.rustforgex.RfxRuntime;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.server.ServerLifecycleHooks;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.Locale;

/**
 * C-36 : exécution d'un test de gameplay — G-03 (génération) ou G-01 (cinq minutes de
 * tick à vide) — jusqu'à l'empreinte de l'état.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Chaque scénario est exécuté deux fois, sans
 * RUSTFORGE-X puis avec, et le critère est l'<strong>égalité d'état</strong>, pas la
 * performance. Maturité : {@code STABLE}.
 *
 * <p>Distinct de {@link MacroRecorder} parce que l'un mesure le temps et l'autre l'état :
 * les deux n'ont ni les mêmes fenêtres, ni les mêmes critères, ni les mêmes sorties.
 *
 * <h2>Déroulé</h2>
 *
 * <ol>
 *   <li>au vingtième tick, le profil {@link LoadProfile#WORLDGEN} fige le monde et force
 *       un carré de chunks ;
 *   <li>tant que tous ne sont pas complets, on attend — c'est la génération ;
 *   <li>puis on laisse reposer : les ticks planifiés à la génération, l'eau et la lave
 *       qui coulent, doivent s'être écoulés avant qu'on regarde ;
 *   <li>empreinte, fichier, arrêt.
 * </ol>
 *
 * <p>Si la génération n'aboutit pas dans le délai, l'empreinte est prise quand même et le
 * fichier le <strong>dit</strong> : une comparaison sur un monde incomplet n'est pas
 * valide, mais un fichier absent serait pire — on ne saurait pas pourquoi.
 *
 * <h2>Armé sur demande, absent sinon</h2>
 *
 * <p>Sans {@link #PROPERTY_OUT}, cette classe ne s'abonne à rien.
 */
public final class DigestRecorder {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Chemin du fichier d'empreinte. Sans cette propriété, l'enregistreur reste absent. */
    public static final String PROPERTY_OUT = "rustforgex.bench.digest.out";

    /** Ticks de repos entre la génération complète et l'empreinte. */
    public static final String PROPERTY_SETTLE = "rustforgex.bench.digest.settle";

    /** Ticks accordés à la génération avant de prendre l'empreinte malgré tout. */
    public static final String PROPERTY_TIMEOUT = "rustforgex.bench.digest.timeout";

    /** Scénario joué : {@code g03} (défaut) ou {@code g01}. */
    public static final String PROPERTY_SCENARIO = "rustforgex.bench.scenario";

    /**
     * G-01 : « démarrage serveur dédié, chargement du monde, 5 minutes de tick à vide ».
     *
     * <p>Six mille ticks : cinq minutes au rythme nominal de vingt par seconde. Compter en
     * ticks plutôt qu'en secondes rend les exécutions comparables même si l'une rame : le
     * jeu y a fait exactement le même nombre de pas.
     */
    static final int G01_TICKS = 6_000;

    /**
     * Demi-côté du carré relu par G-01, en chunks, autour du chunk d'apparition.
     *
     * <p>Sans joueur, seuls les chunks d'apparition restent chargés, dans un rayon d'une
     * dizaine de chunks. Huit reste à l'intérieur avec de la marge : un chunk absent rend
     * la comparaison invalide, il ne la fausse pas.
     */
    static final int G01_RADIUS = 8;

    /** Les scénarios de la PARTIE 20.3.4 que ce harnais sait jouer. */
    enum Scenario {
        /** Génération de 2 000 chunks, comparée à la référence. */
        G03("G-03"),
        /** Démarrage, chargement du monde, cinq minutes de tick à vide. */
        G01("G-01");

        final String id;

        Scenario(String id) {
            this.id = id;
        }

        static Scenario of(String value) {
            return "g01".equalsIgnoreCase(value == null ? "" : value.trim()) ? G01 : G03;
        }
    }

    /**
     * Repos par défaut : dix secondes de jeu.
     *
     * <p>Assez pour qu'une source d'eau générée s'étale, ce qui prend quelques dizaines
     * de ticks ; et identique d'une exécution à l'autre, ce qui seul importe.
     */
    static final int DEFAULT_SETTLE_TICKS = 200;

    /** Délai par défaut : vingt minutes de ticks, très au-delà d'une génération normale. */
    static final int DEFAULT_TIMEOUT_TICKS = 24_000;

    /** Tick d'application du profil, après la mise en route du serveur. */
    private static final int APPLY_AT_TICK = 20;

    /** Cadence de vérification de la génération, en ticks. */
    private static final int POLL_EVERY_TICKS = 20;

    /**
     * Version du fichier. 2 : empreinte par section et ordonnée de la première section.
     *
     * <p>L'empreinte des blocs d'un chunk se compose désormais de celles de ses sections :
     * un fichier de version 1 ne se compare pas à un fichier de version 2.
     */
    private static final int SCHEMA = 2;

    private final Path out;
    private final String label;
    private final Scenario scenario;
    private final int settleTicks;
    private final int timeoutTicks;

    /** Carré relu, bornes incluses. Fixé à l'armement pour G-03, au chargement pour G-01. */
    private int minX;
    private int maxX;
    private int minZ;
    private int maxZ;

    private int seen;
    private int appliedAt = -1;
    private int readyAt = -1;
    private Deque<String> pending;
    private boolean finished;

    private DigestRecorder(Path out, String label, Scenario scenario, int settleTicks,
            int timeoutTicks) {
        this.out = out;
        this.label = label;
        this.scenario = scenario;
        this.settleTicks = settleTicks;
        this.timeoutTicks = timeoutTicks;
        int[] range = LoadProfile.forcedChunkRange(LoadProfile.WORLDGEN_CHUNKS);
        this.minX = range[0];
        this.maxX = range[1];
        this.minZ = range[0];
        this.maxZ = range[1];
    }

    /** Arme l'enregistreur si {@link #PROPERTY_OUT} est renseignée. */
    public static void armIfRequested() {
        String target = System.getProperty(PROPERTY_OUT, "");
        if (target.isBlank()) {
            return;
        }
        Scenario scenario = Scenario.of(System.getProperty(PROPERTY_SCENARIO));
        DigestRecorder recorder = new DigestRecorder(Path.of(target.trim()),
                System.getProperty(MacroRecorder.PROPERTY_LABEL, "unlabelled"),
                scenario,
                intProperty(PROPERTY_SETTLE, DEFAULT_SETTLE_TICKS),
                intProperty(PROPERTY_TIMEOUT, DEFAULT_TIMEOUT_TICKS));
        MinecraftForge.EVENT_BUS.register(recorder);
        LOGGER.info("Test {} armé, empreinte dans {}. Le serveur s'arrêtera ensuite.",
                scenario.id, target);
    }

    /** Fait avancer le scénario d'un pas, en fin de tick. */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void onTick(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.END || finished) {
            return;
        }
        seen++;
        MinecraftServer server = ServerLifecycleHooks.getCurrentServer();
        if (server == null) {
            return;
        }
        if (scenario == Scenario.G01) {
            tickIdle(server);
            return;
        }
        if (seen == APPLY_AT_TICK) {
            pending = new ArrayDeque<>(LoadProfile.commandsFor(LoadProfile.WORLDGEN));
            appliedAt = seen;
        }
        if (pending == null) {
            return;
        }
        if (!pending.isEmpty()) {
            LoadProfile.pump(server, pending);
            return;
        }
        if (seen % POLL_EVERY_TICKS != 0) {
            return;
        }

        ServerLevel level = server.overworld();
        int ready = countReady(level);
        if (ready == expected() && readyAt < 0) {
            readyAt = seen;
            LOGGER.info("G-03 : {} chunks générés en {} ticks, repos de {} ticks.",
                    ready, seen - appliedAt, settleTicks);
        }
        boolean settled = readyAt >= 0 && seen - readyAt >= settleTicks;
        boolean timedOut = seen - appliedAt >= timeoutTicks;
        if (settled || timedOut) {
            finished = true;
            finish(server, level, ready, timedOut && !settled);
        }
    }

    /**
     * G-01 : on ne touche à rien, on laisse tourner, on regarde.
     *
     * <p>Rien n'est figé — c'est la différence avec G-03. Les règles du jeu sont celles
     * d'un serveur réel au repos : cycle jour-nuit, météo, ticks planifiés, entités de
     * bloc. Sans joueur, ni apparition naturelle ni tick aléatoire n'ont lieu : le jeu les
     * réserve aux chunks proches d'un joueur.
     */
    private void tickIdle(MinecraftServer server) {
        if (seen < G01_TICKS) {
            return;
        }
        finished = true;
        ServerLevel level = server.overworld();
        // Le carré est centré sur le chunk d'apparition, connu seulement une fois le monde
        // chargé. Il est écrit dans le fichier par ses clés : deux mondes à apparitions
        // différentes ne couvriraient pas les mêmes chunks, et la comparaison le dirait.
        int spawnX = level.getSharedSpawnPos().getX() >> 4;
        int spawnZ = level.getSharedSpawnPos().getZ() >> 4;
        minX = spawnX - G01_RADIUS;
        maxX = spawnX + G01_RADIUS;
        minZ = spawnZ - G01_RADIUS;
        maxZ = spawnZ + G01_RADIUS;
        appliedAt = 0;
        finish(server, level, countReady(level), false);
    }

    private int expected() {
        return (maxX - minX + 1) * (maxZ - minZ + 1);
    }

    private int countReady(ServerLevel level) {
        int ready = 0;
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                if (level.getChunkSource().getChunkNow(x, z) != null) {
                    ready++;
                }
            }
        }
        return ready;
    }

    /** Prend l'empreinte, écrit le fichier, arrête le serveur. Ne lève jamais. */
    private void finish(MinecraftServer server, ServerLevel level, int ready, boolean timedOut) {
        try {
            long start = System.nanoTime();
            Tables tables = digestAll(level);
            long digestMs = (System.nanoTime() - start) / 1_000_000L;
            write(level, ready, timedOut, digestMs, tables);
            LOGGER.info("{} terminé : empreinte de {} chunks en {} ms, dans {}.",
                    scenario.id, ready, digestMs, out.toAbsolutePath());
        } catch (IOException | RuntimeException e) {
            LOGGER.error("{} : empreinte ou écriture impossible, exécution perdue",
                    scenario.id, e);
        }
        MinecraftForge.EVENT_BUS.unregister(this);
        server.halt(false);
    }

    /**
     * Empreintes de tous les chunks, en deux tables JSON : composantes, et sections de
     * blocs.
     *
     * @param chunks une ligne par chunk : ses quatre empreintes, ou {@code null}
     * @param sections une ligne par chunk : l'empreinte de chacune de ses sections
     */
    private record Tables(String chunks, String sections) {
    }

    /** Une ligne par chunk dans chaque table, ou {@code null} s'il n'est pas chargé. */
    private Tables digestAll(ServerLevel level) {
        StringBuilder chunks = new StringBuilder(LoadProfile.WORLDGEN_CHUNKS * 96);
        StringBuilder sections = new StringBuilder(LoadProfile.WORLDGEN_CHUNKS * 480);
        boolean first = true;
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                String key = "    \"" + x + ',' + z + "\": ";
                chunks.append(first ? "\n" : ",\n").append(key);
                sections.append(first ? "\n" : ",\n").append(key);
                first = false;
                LevelChunk chunk = level.getChunkSource().getChunkNow(x, z);
                if (chunk == null) {
                    // Absent n'est pas une empreinte : une valeur inventée ici se ferait
                    // passer pour un chunk généré (R-660).
                    chunks.append("null");
                    sections.append("null");
                    continue;
                }
                WorldDigest.ChunkDigest digest = WorldDigest.digest(level, chunk);
                chunks.append(String.format(Locale.ROOT,
                        "[\"%016x\", \"%016x\", \"%016x\", \"%016x\"]",
                        digest.blocks(), digest.biomes(),
                        digest.blockEntities(), digest.structures()));
                sections.append('[');
                long[] perSection = digest.sections();
                for (int i = 0; i < perSection.length; i++) {
                    sections.append(i == 0 ? "\"" : ", \"")
                            .append(String.format(Locale.ROOT, "%016x", perSection[i]))
                            .append('"');
                }
                sections.append(']');
            }
        }
        return new Tables(chunks.toString(), sections.toString());
    }

    private void write(ServerLevel level, int ready, boolean timedOut, long digestMs,
            Tables tables) throws IOException {
        RfxRuntime runtime = RfxRuntime.instance();
        boolean active = runtime != null && runtime.active();
        long probed = runtime == null || runtime.instrumentation() == null
                ? 0L : runtime.instrumentation().methodsProbed();

        StringBuilder json = new StringBuilder(
                tables.chunks().length() + tables.sections().length() + 1024);
        json.append("{\n");
        json.append("  \"schema\": ").append(SCHEMA).append(",\n");
        json.append("  \"test\": \"").append(scenario.id).append("\",\n");
        json.append("  \"ticks\": ").append(seen).append(",\n");
        json.append("  \"label\": \"").append(escape(label)).append("\",\n");
        json.append("  \"rfx_active\": ").append(active).append(",\n");
        json.append("  \"methods_probed\": ").append(probed).append(",\n");
        json.append("  \"seed\": ").append(level.getSeed()).append(",\n");
        json.append("  \"chunks_expected\": ").append(expected()).append(",\n");
        json.append("  \"chunks_ready\": ").append(ready).append(",\n");
        json.append("  \"timed_out\": ").append(timedOut).append(",\n");
        json.append("  \"ready_after_ticks\": ")
                .append(readyAt < 0 ? -1 : readyAt - appliedAt).append(",\n");
        json.append("  \"settle_ticks\": ").append(settleTicks).append(",\n");
        json.append("  \"digest_ms\": ").append(digestMs).append(",\n");
        json.append("  \"components\": [\"blocks\", \"biomes\", \"block_entities\", "
                + "\"structures\"],\n");
        json.append("  \"chunks\": {").append(tables.chunks()).append("\n  },\n");
        // Ordonnée de la première section : sans elle, l'indice d'une section dans la
        // table ne dirait pas à quelle hauteur se trouve un écart.
        json.append("  \"min_section_y\": ").append(level.getMinSection()).append(",\n");
        json.append("  \"block_sections\": {").append(tables.sections()).append("\n  }\n");
        json.append("}\n");

        Path parent = out.toAbsolutePath().getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.writeString(out, json.toString(), StandardCharsets.UTF_8);
    }

    private static String escape(String text) {
        return text.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private static int intProperty(String name, int fallback) {
        try {
            String value = System.getProperty(name);
            return value == null || value.isBlank() ? fallback : Integer.parseInt(value.trim());
        } catch (NumberFormatException e) {
            return fallback;
        }
    }
}
