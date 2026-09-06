package dev.rustforgex.forge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;

/**
 * Tests de C-06 étape 1 : le comptage des événements (PARTIE 5.6).
 *
 * <p>Aucun Forge ici, et c'est délibéré : la table ne connaît que des {@link Class} et
 * des horodatages. Ce qui se teste est ce qui décide — l'échantillonnage, la clôture par
 * identité, la résistance à une distribution interrompue.
 */
class EventDispatchTableTest {

    /** Deux types quelconques : la table ne sait rien d'eux, et n'a pas à en savoir. */
    private static final class Alpha { }

    private static final class Beta { }

    /** Une instance distincte par événement, comme sur un vrai bus. */
    private static Object event() {
        return new Object();
    }

    @Test
    @DisplayName("Chaque distribution est comptée, par type")
    void everyDispatchIsCountedPerType() {
        EventDispatchTable table = new EventDispatchTable();
        for (int i = 0; i < 10; i++) {
            table.shouldTime(Alpha.class);
        }
        for (int i = 0; i < 3; i++) {
            table.shouldTime(Beta.class);
        }

        assertEquals(13, table.dispatched());
        assertEquals(2, table.knownTypes());
        assertEquals(10, statsOf(table, Alpha.class).dispatched());
        assertEquals(3, statsOf(table, Beta.class).dispatched());
    }

    /**
     * Le point de conception le plus important de l'étape : chronométrer chaque
     * événement coûterait deux lectures d'horloge sur des milliers d'événements par
     * tick, soit l'ordre de grandeur du budget entier de RUSTFORGE-X.
     */
    @Test
    @DisplayName("Un événement sur SAMPLE_EVERY est chronométré, pas davantage")
    void onlyOneEventInSampleEveryIsTimed() {
        EventDispatchTable table = new EventDispatchTable();
        int wanted = 0;
        for (int i = 0; i < EventDispatchTable.SAMPLE_EVERY * 4; i++) {
            if (table.shouldTime(Alpha.class)) {
                wanted++;
                Object e = event();
                table.beginTimed(e, 1_000);
                table.end(Alpha.class, e, 3_000, false, false);
            }
        }

        assertEquals(4, wanted);
        assertEquals(4, table.timed());
        assertEquals(EventDispatchTable.SAMPLE_EVERY * 4, table.dispatched());
    }

    @Test
    @DisplayName("La durée relevée est celle de l'instance close")
    void theRecordedDurationIsThatOfTheClosedInstance() {
        EventDispatchTable table = new EventDispatchTable();
        Object e = event();
        table.beginTimed(e, 1_000);
        table.end(Alpha.class, e, 4_000, false, false);

        EventDispatchTable.EventStats stats = statsOf(table, Alpha.class);
        assertEquals(1, stats.timed());
        assertEquals(3_000, stats.totalNs());
        assertEquals(3_000, stats.maxNs());
        assertEquals(3_000, stats.meanNs());
    }

    @Test
    @DisplayName("Les distributions imbriquées se referment dans le bon ordre")
    void nestedDispatchesCloseInOrder() {
        EventDispatchTable table = new EventDispatchTable();
        Object outer = event();
        Object inner = event();

        table.beginTimed(outer, 1_000);
        table.beginTimed(inner, 2_000);
        table.end(Beta.class, inner, 2_500, false, false);
        table.end(Alpha.class, outer, 5_000, false, false);

        assertEquals(500, statsOf(table, Beta.class).totalNs());
        assertEquals(4_000, statsOf(table, Alpha.class).totalNs());
        assertEquals(0, table.depth());
        assertEquals(0, table.abandoned());
    }

    /**
     * Un gestionnaire qui lève interrompt la distribution : la clôture de l'événement
     * qu'il traitait n'arrive jamais. Sans recherche par identité, la pile resterait
     * désynchronisée pour le reste de la partie.
     */
    @Test
    @DisplayName("Une distribution interrompue ne désynchronise pas la pile")
    void anAbortedDispatchDoesNotDesyncTheStack() {
        EventDispatchTable table = new EventDispatchTable();
        Object outer = event();
        Object lost = event();

        table.beginTimed(outer, 1_000);
        table.beginTimed(lost, 2_000);
        // `lost` ne se referme jamais : son gestionnaire a levé.
        table.end(Alpha.class, outer, 5_000, false, false);

        assertEquals(4_000, statsOf(table, Alpha.class).totalNs());
        assertEquals(0, table.depth(), "la pile est revenue à zéro");
        assertEquals(1, table.abandoned(), "l'abandon est compté, pas tu");
    }

    @Test
    @DisplayName("Une clôture inconnue ne touche à rien")
    void anUnknownCloseChangesNothing() {
        EventDispatchTable table = new EventDispatchTable();
        Object tracked = event();
        table.beginTimed(tracked, 1_000);

        table.end(Beta.class, event(), 9_000, false, false);

        assertEquals(1, table.depth(), "l'événement suivi reste en attente");
        assertEquals(0, statsOf(table, Beta.class).timed());
    }

    @Test
    @DisplayName("L'annulation et le résultat sont comptés même sans chronométrage")
    void cancellationAndResultAreCountedWithoutTiming() {
        EventDispatchTable table = new EventDispatchTable();
        Object e = event();

        table.end(Alpha.class, e, 0L, true, true);

        EventDispatchTable.EventStats stats = statsOf(table, Alpha.class);
        assertEquals(1, stats.canceled());
        assertEquals(1, stats.withResult());
        assertEquals(0, stats.timed());
    }

    @Test
    @DisplayName("La profondeur d'imbrication est bornée")
    void nestingDepthIsBounded() {
        EventDispatchTable table = new EventDispatchTable();
        for (int i = 0; i < EventDispatchTable.MAX_NESTING + 5; i++) {
            table.beginTimed(event(), 1_000);
        }

        assertEquals(EventDispatchTable.MAX_NESTING, table.depth());
        assertFalse(table.shouldTime(Alpha.class),
                "pile pleine : on compte encore, on ne chronomètre plus");
    }

    @Test
    @DisplayName("Une horloge qui recule ne produit pas de durée")
    void aBackwardClockProducesNoDuration() {
        EventDispatchTable table = new EventDispatchTable();
        Object e = event();
        table.beginTimed(e, 5_000);
        table.end(Alpha.class, e, 1_000, false, false);

        assertEquals(0, statsOf(table, Alpha.class).timed());
        assertEquals(0, table.timed());
    }

    @Test
    @DisplayName("isTimed distingue ce qui est suivi de ce qui ne l'est pas")
    void isTimedTellsTrackedFromUntracked() {
        EventDispatchTable table = new EventDispatchTable();
        Object tracked = event();
        table.beginTimed(tracked, 1_000);

        assertTrue(table.isTimed(tracked));
        assertFalse(table.isTimed(event()));
    }

    @Test
    @DisplayName("La remise à zéro de la pile ne touche pas aux compteurs")
    void resettingTheStackKeepsTheCounters() {
        EventDispatchTable table = new EventDispatchTable();
        table.shouldTime(Alpha.class);
        table.beginTimed(event(), 1_000);

        table.resetNesting();

        assertEquals(0, table.depth());
        assertEquals(1, table.dispatched());
        assertEquals(1, statsOf(table, Alpha.class).dispatched());
    }

    /** Retrouve les compteurs d'un type dans l'instantané. */
    private static EventDispatchTable.EventStats statsOf(EventDispatchTable table, Class<?> type) {
        List<EventDispatchTable.EventStats> all = table.snapshot();
        return all.stream()
                .filter(s -> s.type().equals(type.getName()))
                .findFirst()
                .orElseThrow(() -> new AssertionError("type absent de l'instantané : " + type));
    }
}
