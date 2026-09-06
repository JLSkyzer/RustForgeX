package dev.rustforgex.forge;

import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.eventbus.api.Event;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.IEventBus;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.function.Consumer;

/**
 * C-06, étape 1 : observer le bus d'événements sans rien y changer.
 *
 * <p>Composant : C-06. Cahier des charges : PARTIE 5.6. Exigences : R-330, R-332.
 * Conception : {@code docs/design/C-06-event-observer.md}. Maturité : {@code STABLE}.
 *
 * <h2>Ce que cette étape fait, et ce qu'elle ne fait pas</h2>
 *
 * <p>Deux auditeurs sont enregistrés sur le type de base {@link Event}, l'un en
 * {@link EventPriority#HIGHEST}, l'autre en {@link EventPriority#LOWEST}. Un auditeur du
 * type de base reçoit tout ce qui passe sur le bus : la liste d'auditeurs d'un événement
 * chaîne celle de ses classes parentes.
 *
 * <p><strong>Aucun auditeur tiers n'est touché.</strong> Rien n'est enveloppé, rien n'est
 * désenregistré, aucun ordre n'est modifié : R-330 est tenu par construction, et non par
 * précaution. L'attribution d'une annulation à un gestionnaire précis viendra plus tard,
 * et seulement si elle sert à décider quelque chose.
 *
 * <h2>Deux choix qui ne sont pas des détails</h2>
 *
 * <p><strong>Les deux auditeurs reçoivent les événements annulés.</strong> Sans cela, un
 * événement annulé par un gestionnaire de priorité intermédiaire n'atteindrait jamais
 * notre {@code LOWEST} : le compteur d'annulations serait toujours nul, et la pile de
 * chronométrage se désynchroniserait à la première annulation.
 *
 * <p><strong>Seul le fil autoritatif est observé.</strong> Les événements postés depuis
 * les fils réseau ou les fils de travail ne consomment pas de temps de tick. Les ignorer
 * évite tout verrou et tout compteur atomique dans un chemin appelé des milliers de fois
 * par tick (INV-14). Le fil est reconnu au premier {@code ServerTickEvent} observé.
 *
 * <h2>Ce que la mesure vaut</h2>
 *
 * <p>La durée relevée est celle qui sépare notre {@code HIGHEST} de notre
 * {@code LOWEST}. Si un autre mod s'enregistre en {@code HIGHEST} avant nous, son temps
 * n'y figure pas : ADR-018 a établi qu'on ne peut pas garantir d'être premier, et
 * prétendre le contraire serait faux. C'est une borne inférieure du coût de distribution,
 * pas sa valeur exacte.
 */
public final class EventObserver {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex");

    private final EventDispatchTable table = new EventDispatchTable();
    private final IEventBus bus;

    /**
     * Références retenues des deux auditeurs : {@code unregister} identifie un auditeur
     * enregistré par {@code addListener} à l'instance du {@link Consumer}.
     */
    private final Consumer<Event> onFirst = this::onFirst;
    private final Consumer<Event> onLast = this::onLast;

    /** Fil qui exécute le tick, reconnu au premier {@code ServerTickEvent}. */
    private volatile Thread authoritative;

    private boolean registered;

    /**
     * @param bus bus d'événements à observer, en général {@code MinecraftForge.EVENT_BUS}
     */
    public EventObserver(IEventBus bus) {
        this.bus = bus;
    }

    /** @return un observateur du bus principal de Forge */
    public static EventObserver onForgeBus() {
        return new EventObserver(MinecraftForge.EVENT_BUS);
    }

    /**
     * Enregistre les deux auditeurs. Sans effet s'ils le sont déjà.
     *
     * <p>N'échoue jamais : un défaut de C-06 ne doit pas empêcher de jouer. Le cas
     * échéant, l'observation reste simplement inactive.
     */
    public synchronized void start() {
        if (registered) {
            return;
        }
        try {
            // receiveCanceled = true : voir la note de tête, c'est ce qui empêche la
            // pile de se désynchroniser et le compteur d'annulations d'être toujours nul.
            bus.addListener(EventPriority.HIGHEST, true, Event.class, onFirst);
            bus.addListener(EventPriority.LOWEST, true, Event.class, onLast);
            registered = true;
            LOGGER.info("Observation des événements armée sur le bus Forge.");
            logModuleAccess();
        } catch (RuntimeException | LinkageError e) {
            LOGGER.warn("Observation des événements impossible : les événements ne "
                    + "seront pas comptés. Le jeu tourne normalement.", e);
        }
    }

    /** Retire les deux auditeurs. Sans effet s'ils ne sont pas enregistrés. */
    public synchronized void stop() {
        if (!registered) {
            return;
        }
        registered = false;
        try {
            bus.unregister(onFirst);
            bus.unregister(onLast);
        } catch (RuntimeException | LinkageError e) {
            // L'arrêt du mod ne doit jamais échouer sur ce chemin.
            LOGGER.debug("Retrait des auditeurs d'observation impossible.");
        }
        table.resetNesting();
    }

    /**
     * Dit si le paquet interne d'{@code eventbus} nous est ouvert.
     *
     * <p>C'est la question qui commande l'étendue des étapes 2 et 3 : reconstituer
     * l'ordre et la propriété des auditeurs demande de lire des champs privés d'un
     * module nommé, ce qu'un {@code setAccessible} ne peut faire que sur un paquet
     * ouvert. La poser coûte une ligne et évite d'écrire du code qui ne pourrait pas
     * s'exécuter.
     */
    private static void logModuleAccess() {
        try {
            Module eventbus = Event.class.getModule();
            LOGGER.info("Module {} : paquet interne ouvert à {} — {}. "
                            + "L'étape 1 n'en a pas besoin ; les étapes 2 et 3, si.",
                    eventbus.getName(),
                    EventObserver.class.getModule().getName(),
                    eventbus.isOpen("net.minecraftforge.eventbus",
                            EventObserver.class.getModule()));
        } catch (RuntimeException | LinkageError e) {
            LOGGER.debug("Accessibilité du module eventbus indéterminable.");
        }
    }

    /** Premier auditeur : compte le passage, et n'ouvre le chronomètre qu'échantillon. */
    private void onFirst(Event event) {
        if (!isAuthoritative(event)) {
            return;
        }
        if (table.shouldTime(event.getClass())) {
            table.beginTimed(event, System.nanoTime());
        }
    }

    /** Dernier auditeur : relève l'annulation, le résultat, et clôt le chronomètre. */
    private void onLast(Event event) {
        if (Thread.currentThread() != authoritative) {
            return;
        }
        // L'horloge n'est lue que pour les événements effectivement chronométrés : une
        // comparaison de références sur seize cases coûte bien moins qu'un nanoTime.
        long now = table.isTimed(event) ? System.nanoTime() : 0L;
        table.end(event.getClass(), event, now, event.isCanceled(), hasNonDefaultResult(event));
    }

    /**
     * Indique si l'événement porte un résultat autre que le résultat neutre.
     *
     * <p>{@code getResult} n'a de sens que pour un type annoté ; l'interroger ailleurs
     * rendrait {@code DEFAULT} sans rien signifier.
     */
    private static boolean hasNonDefaultResult(Event event) {
        return event.hasResult() && event.getResult() != Event.Result.DEFAULT;
    }

    /**
     * Reconnaît le fil autoritatif, et filtre tout ce qui n'en vient pas.
     *
     * <p>Tant qu'il n'est pas connu, rien n'est compté : attribuer au tick des
     * événements postés ailleurs fausserait la mesure dès le premier chiffre.
     */
    private boolean isAuthoritative(Event event) {
        Thread known = authoritative;
        if (known == null) {
            if (event instanceof TickEvent.ServerTickEvent) {
                authoritative = Thread.currentThread();
            }
            return false;
        }
        return Thread.currentThread() == known;
    }

    /** @return les compteurs par type d'événement */
    public EventDispatchTable table() {
        return table;
    }

    /** @return {@code true} si les auditeurs sont en place */
    public boolean observing() {
        return registered;
    }

    /** @return le fil autoritatif, ou {@code null} s'il n'est pas encore reconnu */
    public Thread authoritativeThread() {
        return authoritative;
    }
}
