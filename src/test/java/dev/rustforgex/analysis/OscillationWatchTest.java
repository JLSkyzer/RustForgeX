package dev.rustforgex.analysis;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;

/** Tests de la surveillance des verrouillages pour oscillation (C-15, FM-21, E-2201). */
class OscillationWatchTest {

    private static Map<String, Object> locked(long count) {
        return Map.of("decision", Map.of("locked", count));
    }

    @Test
    @DisplayName("Un verrouillage n'est signalé qu'une fois, tant qu'il dure")
    void aLockIsReportedOnce() {
        OscillationWatch watch = new OscillationWatch();
        assertEquals(0, watch.observe(locked(0)));
        assertEquals(2, watch.observe(locked(2)), "deux nouveaux verrouillages");
        assertEquals(0, watch.observe(locked(2)), "les mêmes : rien de neuf");
        assertEquals(1, watch.observe(locked(3)));
        assertEquals(0, watch.observe(locked(1)), "une levée n'est pas un verrouillage");
        assertEquals(1, watch.observe(locked(2)), "un nouveau, après la levée");
    }

    @Test
    @DisplayName("Un statut absent ou illisible ne signale rien et ne fausse pas la suite")
    void anUnreadableStatusSaysNothing() {
        OscillationWatch watch = new OscillationWatch();
        assertEquals(1, watch.observe(locked(1)));
        assertEquals(0, watch.observe(null));
        assertEquals(0, watch.observe(Map.of()));
        assertEquals(0, watch.observe(Map.of("decision", Map.of("locked", "deux"))));
        assertEquals(0, watch.observe(locked(1)), "le relevé illisible n'a rien effacé");
    }
}
