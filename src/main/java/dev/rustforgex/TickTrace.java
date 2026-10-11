package dev.rustforgex;

import com.mojang.logging.LogUtils;
import dev.rustforgex.forge.EventDispatchTable;
import dev.rustforgex.forge.EventObserver;
import dev.rustforgex.forge.TickCycle;
import dev.rustforgex.instrument.Instrumentation;
import dev.rustforgex.instrument.RfxProbes;
import org.slf4j.Logger;

import java.util.concurrent.Executor;

/**
 * C-34 : journal de tick, trace périodique en {@code DEBUG}.
 *
 * <p>Cahier des charges : PARTIE 5.32 (« journal de tick optionnel »). Test : T-401.
 * Maturité : {@code STABLE}.
 *
 * <p>Séparé de {@link RustForgeX} pour que le banc de T-401 puisse en chronométrer
 * l'appel tel qu'il est fait en jeu : c'est une part du coût de la télémétrie.
 */
public final class TickTrace {

    private static final Logger LOGGER = LogUtils.getLogger();

    private TickTrace() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Intervalle, en ticks, de la trace de progression du cycle de tick.
     *
     * <p>Six cents ticks, soit trente secondes de jeu nominal. La trace est émise en
     * {@code DEBUG} : elle ne parle qu'à qui la cherche, et son coût — un modulo par
     * tick — est compté dans celui de la télémétrie (T-401). C'est le « journal de tick »
     * que la PARTIE 5.32 prévoit.
     */
    public static final long INTERVAL = 600;

    /**
     * Trace périodique du cycle de tick et de l'instrumentation.
     *
     * <p>Elle dit ce que les assertions ne pensent pas à demander : combien de classes
     * sont passées par le transformateur, combien de méthodes en sont ressorties
     * sondées, et combien de fois la table des niveaux a changé. Un profileur qui ne
     * sonde rien et un profileur qui sonde tout produisent le même silence.
     *
     * <p>Seul le dépôt de la tâche a lieu sur le fil appelant ; la lecture des compteurs,
     * la mise en forme et l'écriture se font sur le fil des rapports. Sur un serveur
     * Forge de production, {@code DEBUG} est réellement écrit dans {@code debug.log}, et
     * cette écriture coûtait 144 à 193 µs au fil du serveur (mesure T-401, ADR-034).
     * Les compteurs lus sont atomiques : ils peuvent l'être d'un autre fil, quelques
     * microsecondes plus tard, sans que la trace en soit faussée.
     *
     * @param ticks numéro du tick, figé au moment de l'appel
     * @param cycle cycle de tick dont la trace rend compte
     */
    public static void log(long ticks, TickCycle cycle) {
        log(ticks, cycle, RfxRuntime.REPORT_WRITER);
    }

    /**
     * Comme {@link #log(long, TickCycle)}, sur un exécuteur donné.
     *
     * @param ticks numéro du tick
     * @param cycle cycle de tick
     * @param writer fil qui lit, met en forme et écrit la trace
     */
    static void log(long ticks, TickCycle cycle, Executor writer) {
        writer.execute(() -> write(ticks, cycle));
    }

    /** Lit les compteurs et écrit la trace. Hors du fil du serveur. */
    private static void write(long ticks, TickCycle cycle) {
        RfxRuntime runtime = RfxRuntime.instance();
        Instrumentation instrumentation = runtime == null ? null : runtime.instrumentation();
        if (instrumentation == null || !instrumentation.armed()) {
            LOGGER.debug("Cycle de tick : {} ticks observés, {} appels refusés.",
                    ticks, cycle.rejectedCalls());
            return;
        }
        LOGGER.debug(
                "Cycle de tick : {} ticks observés, {} appels refusés. Instrumentation : "
                        + "{} classes vues, {} méthodes sondées, {} échecs, "
                        + "{} mises à jour de niveaux. Sondes : {} dans la table, "
                        + "{} armées, puits {}.",
                ticks, cycle.rejectedCalls(),
                instrumentation.classesSeen(), instrumentation.methodsProbed(),
                instrumentation.transformFailures(), cycle.levelUpdates(),
                RfxProbes.probeCount(), RfxProbes.armedCount(),
                RfxProbes.active() ? "installé" : "absent");
        logEvents(runtime);
    }

    /**
     * Trace de l'observation du bus d'événements (C-06, étape 1).
     *
     * <p>Un observateur qui compte zéro événement et un observateur absent produisent le
     * même silence : cette ligne les distingue, comme celle de l'instrumentation.
     */
    private static void logEvents(RfxRuntime runtime) {
        EventObserver observer = runtime.eventObserver();
        if (observer == null || !observer.observing()) {
            return;
        }
        EventDispatchTable table = observer.table();
        LOGGER.debug("Événements : {} distribués sur {} types, {} chronométrés, "
                        + "{} chronométrages abandonnés.",
                table.dispatched(), table.knownTypes(), table.timed(), table.abandoned());
    }
}
