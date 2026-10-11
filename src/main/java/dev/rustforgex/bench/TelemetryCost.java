package dev.rustforgex.bench;

import dev.rustforgex.RfxRuntime;
import dev.rustforgex.TickTrace;
import dev.rustforgex.forge.HookGuard;
import dev.rustforgex.forge.TickCycle;
import net.minecraft.server.MinecraftServer;
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
import java.util.Arrays;
import java.util.Locale;

/**
 * C-36 : mesure de T-401 — « le coût de la télémétrie est inférieur à 0,2 % du MSPT »
 * (R-561).
 *
 * <p>Cahier des charges : PARTIE 5.32. Maturité : {@code STABLE}. Méthode : ADR-034.
 *
 * <p>Un écart de 0,2 % du MSPT est hors de portée d'une comparaison de deux serveurs :
 * leur dispersion est de plusieurs pour cent (ADR-021). La mesure se fait donc en place,
 * sur le fil du serveur, en chronométrant à chaque tick ce que la télémétrie Java exécute
 * à chaque tick :
 * <ul>
 *   <li>la tenue des compteurs de deux gardes d'accroche — une garde identique aux
 *       deux du cycle de tick, sur une action vide, deux fois, avec le modulo qui décide
 *       du journal de tick ;
 *   <li>le journal de tick, appelé tel qu'en jeu tous les {@link TickTrace#INTERVAL}
 *       ticks, et amorti sur cet intervalle.
 * </ul>
 * Le chronomètre lui-même est compris dans la mesure, qui est donc un <strong>majorant</strong>.
 * Son coût à vide est relevé à part, pour qu'on sache ce qu'il pèse.
 *
 * <p>Le MSPT est mesuré par le même fil : du début du tick (priorité la plus haute) à sa
 * fin (priorité la plus basse). Il manque ce que le serveur fait hors des événements de
 * tick, ce qui le sous-estime légèrement — et surestime donc le rapport, dans le sens
 * prudent.
 *
 * <p>Sans {@link #PROPERTY_OUT}, cette classe ne s'abonne à rien.
 */
public final class TelemetryCost {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Fichier de résultats. Sans cette propriété, la mesure reste absente. */
    public static final String PROPERTY_OUT = "rustforgex.bench.telemetry.out";

    /** Ticks mesurés. Distincte de {@code rustforgex.bench.ticks}, qui arme le banc macro. */
    public static final String PROPERTY_TICKS = "rustforgex.bench.telemetry.ticks";

    /** Ticks d'échauffement avant la mesure. */
    public static final String PROPERTY_WARMUP = "rustforgex.bench.telemetry.warmup";

    /** Gardes d'accroche exécutées à chaque tick par le cycle de tick : PRE et POST. */
    static final int GUARDS_PER_TICK = 2;

    private static final Runnable NOOP = () -> { };

    private final Path out;
    private final String label;
    private final int warmup;
    private final long[] msptNs;
    private final long[] guardNs;
    private final long[] bracketNs;
    private final long[] traceNs;
    private final HookGuard replica = new HookGuard("t401-replica", message -> { });

    private int seen;
    private int measured;
    private int traces;
    private long tickStart;
    private long sink;
    private boolean finished;

    private TelemetryCost(Path out, String label, int warmup, int ticks) {
        this.out = out;
        this.label = label;
        this.warmup = warmup;
        this.msptNs = new long[ticks];
        this.guardNs = new long[ticks];
        this.bracketNs = new long[ticks];
        this.traceNs = new long[ticks / (int) TickTrace.INTERVAL + 1];
    }

    /** Arme la mesure si {@link #PROPERTY_OUT} est renseignée. */
    public static void armIfRequested() {
        String target = System.getProperty(PROPERTY_OUT, "");
        if (target.isBlank()) {
            return;
        }
        int ticks = Integer.getInteger(PROPERTY_TICKS, 6_000);
        int warmup = Integer.getInteger(PROPERTY_WARMUP, 1_200);
        MinecraftForge.EVENT_BUS.register(new TelemetryCost(Path.of(target.trim()),
                System.getProperty(MacroRecorder.PROPERTY_LABEL, "unlabelled"), warmup, ticks));
        LOGGER.info("T-401 armé : {} ticks d'échauffement, {} mesurés, résultats dans {}. "
                + "Le serveur s'arrêtera ensuite.", warmup, ticks, target);
    }

    /** Début du tick, avant tout autre mod. */
    @SubscribeEvent(priority = EventPriority.HIGHEST)
    public void onTickStart(final TickEvent.ServerTickEvent event) {
        if (event.phase == TickEvent.Phase.START && !finished) {
            tickStart = System.nanoTime();
        }
    }

    /** Fin du tick, après tout autre mod : relevé du MSPT, puis mesure de la télémétrie. */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void onTickEnd(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.END || finished) {
            return;
        }
        long tickEnd = System.nanoTime();
        seen++;
        RfxRuntime runtime = RfxRuntime.instance();
        TickCycle cycle = runtime == null ? null : runtime.tickCycle();
        if (seen <= warmup || cycle == null) {
            return;
        }

        msptNs[measured] = tickEnd - tickStart;

        long ticks = cycle.currentTick();
        long t0 = System.nanoTime();
        for (int g = 0; g < GUARDS_PER_TICK; g++) {
            replica.run(NOOP);
        }
        if (ticks % TickTrace.INTERVAL == 0) {
            sink++;
        }
        long t1 = System.nanoTime();
        long t2 = System.nanoTime();
        guardNs[measured] = t1 - t0;
        bracketNs[measured] = t2 - t1;

        // Décalé d'un demi-intervalle : le journal réel tombe sur les multiples de
        // l'intervalle, et deux écritures dans le même tick ne mesureraient pas la même chose.
        if (ticks % TickTrace.INTERVAL == TickTrace.INTERVAL / 2 && traces < traceNs.length) {
            long s = System.nanoTime();
            TickTrace.log(ticks, cycle);
            traceNs[traces++] = System.nanoTime() - s;
        }

        measured++;
        if (measured < msptNs.length) {
            return;
        }
        finished = true;
        try {
            write(runtime.active());
        } catch (IOException | RuntimeException e) {
            LOGGER.error("T-401 : résultats impossibles à écrire, mesure perdue", e);
        }
        MinecraftForge.EVENT_BUS.unregister(this);
        MinecraftServer server = ServerLifecycleHooks.getCurrentServer();
        if (server != null) {
            server.halt(false);
        }
    }

    private void write(boolean active) throws IOException {
        long[] trace = Arrays.copyOf(traceNs, traces);
        double guardMean = mean(guardNs);
        double traceMean = mean(trace);
        double perTick = guardMean + traceMean / TickTrace.INTERVAL;
        double msptMean = mean(msptNs);

        StringBuilder json = new StringBuilder("{\n");
        json.append("  \"test\": \"T-401\",\n");
        json.append("  \"label\": \"").append(label.replace("\"", "'")).append("\",\n");
        json.append("  \"rfx_active\": ").append(active).append(",\n");
        json.append("  \"trace_debug_enabled\": ")
                .append(LoggerFactory.getLogger(TickTrace.class).isDebugEnabled()).append(",\n");
        json.append("  \"ticks\": ").append(measured).append(",\n");
        json.append("  \"guards_per_tick\": ").append(GUARDS_PER_TICK).append(",\n");
        json.append("  \"trace_interval\": ").append(TickTrace.INTERVAL).append(",\n");
        json.append("  \"mspt_ns\": ").append(stats(msptNs)).append(",\n");
        json.append("  \"guards_ns\": ").append(stats(guardNs)).append(",\n");
        json.append("  \"bracket_ns\": ").append(stats(bracketNs)).append(",\n");
        json.append("  \"trace_ns\": ").append(stats(trace)).append(",\n");
        json.append("  \"java_telemetry_ns_per_tick\": ").append(format(perTick)).append(",\n");
        json.append("  \"java_ratio_pct\": ").append(format(100.0 * perTick / msptMean))
                .append(",\n");
        json.append("  \"sink\": ").append(sink).append("\n}\n");

        Files.createDirectories(out.toAbsolutePath().getParent());
        Files.writeString(out, json.toString(), StandardCharsets.UTF_8);
        LOGGER.info("T-401 : télémétrie Java {} ns par tick pour un MSPT moyen de {} ns "
                + "({} %).", format(perTick), format(msptMean),
                format(100.0 * perTick / msptMean));
    }

    private static String stats(long[] values) {
        if (values.length == 0) {
            return "{\"n\": 0}";
        }
        long[] sorted = values.clone();
        Arrays.sort(sorted);
        return "{\"n\": " + sorted.length
                + ", \"mean\": " + format(mean(sorted))
                + ", \"p50\": " + sorted[sorted.length / 2]
                + ", \"p95\": " + sorted[(int) Math.min(sorted.length - 1, sorted.length * 0.95)]
                + ", \"max\": " + sorted[sorted.length - 1] + "}";
    }

    private static double mean(long[] values) {
        return values.length == 0 ? 0.0 : Arrays.stream(values).average().orElse(0.0);
    }

    private static String format(double value) {
        return String.format(Locale.ROOT, "%.4f", value);
    }
}
