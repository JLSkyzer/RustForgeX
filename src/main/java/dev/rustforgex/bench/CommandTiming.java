package dev.rustforgex.bench;

import dev.rustforgex.RfxRuntime;
import dev.rustforgex.command.RfxCommands;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.phys.Vec3;
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
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Deque;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * C-36 : mesure de T-421 — « aucune commande ne dépasse 5 ms sur le thread serveur »
 * (R-601).
 *
 * <p>Cahier des charges : PARTIE 5.36. Maturité : {@code STABLE}.
 *
 * <p>Sur un serveur réel, runtime démarré et réchauffé ({@value #WARMUP_TICKS} ticks),
 * chaque commande de {@code /rfx} — trouvée en parcourant l'arbre enregistré — est
 * exécutée {@value #RUNS} fois, une par tick, sur le fil du serveur, et chronométrée.
 * Le fichier donne pour chacune la médiane, le maximum et le nombre d'exécutions au-delà
 * du budget ; le serveur s'arrête ensuite.
 *
 * <p>Le critère est le <strong>maximum</strong> : la spécification dit « aucune
 * commande ne dépasse », pas « en moyenne ». Une pause du ramasse-miettes qui tombe
 * pendant une commande compte donc contre elle ; le fichier le dit, il ne le cache pas.
 *
 * <p>Sans {@link #PROPERTY_OUT}, cette classe ne s'abonne à rien.
 */
public final class CommandTiming {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Fichier de résultats. Sans cette propriété, la mesure reste absente. */
    public static final String PROPERTY_OUT = "rustforgex.bench.commands.out";

    /** Budget d'une commande sur le fil du serveur (R-601). */
    static final long BUDGET_NANOS = 5_000_000L;

    /** Ticks avant la première commande : runtime démarré, sondes armées. */
    static final int WARMUP_TICKS = 600;

    /** Exécutions par commande. */
    static final int RUNS = 20;

    private final Path out;
    private final String label;
    private final Map<String, List<Long>> timings = new LinkedHashMap<>();
    private Deque<String> pending;
    private int seen;
    private boolean finished;

    private CommandTiming(Path out, String label) {
        this.out = out;
        this.label = label;
    }

    /** Arme la mesure si {@link #PROPERTY_OUT} est renseignée. */
    public static void armIfRequested() {
        String target = System.getProperty(PROPERTY_OUT, "");
        if (target.isBlank()) {
            return;
        }
        MinecraftForge.EVENT_BUS.register(new CommandTiming(Path.of(target.trim()),
                System.getProperty(MacroRecorder.PROPERTY_LABEL, "unlabelled")));
        LOGGER.info("T-421 armé, résultats dans {}. Le serveur s'arrêtera ensuite.", target);
    }

    /** Une commande par tick, en fin de tick, après le réchauffement. */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void onTick(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.END || finished) {
            return;
        }
        seen++;
        MinecraftServer server = ServerLifecycleHooks.getCurrentServer();
        if (server == null || seen < WARMUP_TICKS) {
            return;
        }
        if (pending == null) {
            pending = new ArrayDeque<>();
            for (int run = 0; run < RUNS; run++) {
                pending.addAll(RfxCommands.executablePaths(
                        server.getCommands().getDispatcher()));
            }
            LOGGER.info("T-421 : {} exécutions à chronométrer.", pending.size());
        }
        if (!pending.isEmpty()) {
            String command = pending.poll();
            ServerLevel level = server.overworld();
            long start = System.nanoTime();
            CommandCapture.run(server, level, Vec3.ZERO, command);
            long elapsed = System.nanoTime() - start;
            timings.computeIfAbsent(command, key -> new ArrayList<>()).add(elapsed);
            return;
        }
        finished = true;
        try {
            write();
        } catch (IOException | RuntimeException e) {
            LOGGER.error("T-421 : résultats impossibles à écrire, mesure perdue", e);
        }
        MinecraftForge.EVENT_BUS.unregister(this);
        server.halt(false);
    }

    private void write() throws IOException {
        RfxRuntime runtime = RfxRuntime.instance();
        boolean active = runtime != null && runtime.active();
        StringBuilder json = new StringBuilder("{\n");
        json.append("  \"test\": \"T-421\",\n");
        json.append("  \"label\": \"").append(label.replace("\"", "'")).append("\",\n");
        json.append("  \"rfx_active\": ").append(active).append(",\n");
        json.append("  \"budget_us\": ").append(BUDGET_NANOS / 1_000).append(",\n");
        json.append("  \"commands\": {");
        boolean first = true;
        int over = 0;
        for (Map.Entry<String, List<Long>> entry : timings.entrySet()) {
            long[] sorted = entry.getValue().stream().mapToLong(Long::longValue).sorted()
                    .toArray();
            long median = sorted[sorted.length / 2];
            long max = sorted[sorted.length - 1];
            long beyond = Arrays.stream(sorted).filter(n -> n > BUDGET_NANOS).count();
            // Rang de l'exécution la plus lente : 0 désigne le premier appel, celui qui
            // charge les classes de la commande ; un autre rang, un aléa du moment.
            int maxRun = entry.getValue().indexOf(max);
            over += (int) beyond;
            json.append(first ? "\n" : ",\n").append(String.format(Locale.ROOT,
                    "    \"%s\": {\"runs\": %d, \"median_us\": %d, \"max_us\": %d, "
                            + "\"max_run\": %d, \"over_budget\": %d}",
                    entry.getKey(), sorted.length, median / 1_000, max / 1_000, maxRun,
                    beyond));
            first = false;
            LOGGER.info("T-421 « {} » : médiane {} µs, maximum {} µs (exécution {}), "
                    + "{} au-delà de 5 ms", entry.getKey(), median / 1_000, max / 1_000,
                    maxRun, beyond);
        }
        json.append("\n  },\n  \"over_budget\": ").append(over).append("\n}\n");
        Path parent = out.toAbsolutePath().getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.writeString(out, json.toString(), StandardCharsets.UTF_8);
        LOGGER.info("T-421 terminé : {} exécution(s) au-delà de 5 ms, dans {}.", over,
                out.toAbsolutePath());
    }
}
