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
 * C-36 : exécution d'un test de gameplay G-03, de la génération à l'empreinte.
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

    private static final int SCHEMA = 1;

    private final Path out;
    private final String label;
    private final int settleTicks;
    private final int timeoutTicks;
    private final int firstChunk;
    private final int lastChunk;

    private int seen;
    private int appliedAt = -1;
    private int readyAt = -1;
    private Deque<String> pending;
    private boolean finished;

    private DigestRecorder(Path out, String label, int settleTicks, int timeoutTicks) {
        this.out = out;
        this.label = label;
        this.settleTicks = settleTicks;
        this.timeoutTicks = timeoutTicks;
        int[] range = LoadProfile.forcedChunkRange(LoadProfile.WORLDGEN_CHUNKS);
        this.firstChunk = range[0];
        this.lastChunk = range[1];
    }

    /** Arme l'enregistreur si {@link #PROPERTY_OUT} est renseignée. */
    public static void armIfRequested() {
        String target = System.getProperty(PROPERTY_OUT, "");
        if (target.isBlank()) {
            return;
        }
        DigestRecorder recorder = new DigestRecorder(Path.of(target.trim()),
                System.getProperty(MacroRecorder.PROPERTY_LABEL, "unlabelled"),
                intProperty(PROPERTY_SETTLE, DEFAULT_SETTLE_TICKS),
                intProperty(PROPERTY_TIMEOUT, DEFAULT_TIMEOUT_TICKS));
        MinecraftForge.EVENT_BUS.register(recorder);
        LOGGER.info("Test G-03 armé : génération de {} chunks, empreinte dans {}. "
                + "Le serveur s'arrêtera ensuite.", LoadProfile.WORLDGEN_CHUNKS, target);
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

    private int expected() {
        int side = lastChunk - firstChunk + 1;
        return side * side;
    }

    private int countReady(ServerLevel level) {
        int ready = 0;
        for (int x = firstChunk; x <= lastChunk; x++) {
            for (int z = firstChunk; z <= lastChunk; z++) {
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
            String chunks = digestAll(level);
            long digestMs = (System.nanoTime() - start) / 1_000_000L;
            write(level, ready, timedOut, digestMs, chunks);
            LOGGER.info("G-03 terminé : empreinte de {} chunks en {} ms, dans {}.",
                    ready, digestMs, out.toAbsolutePath());
        } catch (IOException | RuntimeException e) {
            LOGGER.error("G-03 : empreinte ou écriture impossible, exécution perdue", e);
        }
        MinecraftForge.EVENT_BUS.unregister(this);
        server.halt(false);
    }

    /** Une ligne par chunk, ou {@code null} s'il n'est pas chargé. */
    private String digestAll(ServerLevel level) {
        StringBuilder json = new StringBuilder(LoadProfile.WORLDGEN_CHUNKS * 96);
        boolean first = true;
        for (int x = firstChunk; x <= lastChunk; x++) {
            for (int z = firstChunk; z <= lastChunk; z++) {
                json.append(first ? "\n" : ",\n");
                first = false;
                json.append("    \"").append(x).append(',').append(z).append("\": ");
                LevelChunk chunk = level.getChunkSource().getChunkNow(x, z);
                if (chunk == null) {
                    // Absent n'est pas une empreinte : une valeur inventée ici se ferait
                    // passer pour un chunk généré (R-660).
                    json.append("null");
                    continue;
                }
                WorldDigest.ChunkDigest digest = WorldDigest.digest(level, chunk);
                json.append(String.format(Locale.ROOT,
                        "[\"%016x\", \"%016x\", \"%016x\", \"%016x\"]",
                        digest.blocks(), digest.biomes(),
                        digest.blockEntities(), digest.structures()));
            }
        }
        return json.toString();
    }

    private void write(ServerLevel level, int ready, boolean timedOut, long digestMs,
            String chunks) throws IOException {
        RfxRuntime runtime = RfxRuntime.instance();
        boolean active = runtime != null && runtime.active();
        long probed = runtime == null || runtime.instrumentation() == null
                ? 0L : runtime.instrumentation().methodsProbed();

        StringBuilder json = new StringBuilder(chunks.length() + 1024);
        json.append("{\n");
        json.append("  \"schema\": ").append(SCHEMA).append(",\n");
        json.append("  \"test\": \"G-03\",\n");
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
        json.append("  \"chunks\": {").append(chunks).append("\n  }\n");
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
