package dev.rustforgex.analysis;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;

/** Tests du suivi de l'étalonnage A/A (C-15, ADR-069). */
class CalibrationWatchTest {

    private static Map<String, Object> status(long runs, long falseGains) {
        return status(runs, falseGains, 4L);
    }

    private static Map<String, Object> status(long runs, long falseGains, long traced) {
        return Map.of("decision", Map.of("aa_runs", runs, "aa_false_gains", falseGains,
                "aa_ticks", runs * 1000, "traced", traced));
    }

    @Test
    @DisplayName("Un état n'est rendu que si de nouvelles comparaisons sont conclues")
    void onlyNewRunsAreReported() {
        CalibrationWatch watch = new CalibrationWatch();
        assertEquals(0, watch.observe(status(0, 0, 0)).traced(), "premier relevé");
        assertNull(watch.observe(status(0, 0, 0)), "aucune comparaison, rien de neuf");
        assertEquals(new CalibrationWatch.Calibration(3, 1, 3000, 4),
                watch.observe(status(3, 1)));
        assertNull(watch.observe(status(3, 1)), "rien de neuf");
        assertEquals(4, watch.observe(status(4, 1)).runs());
        assertEquals(2, watch.observe(status(4, 1, 2)).traced(), "suivi changé");
    }

    @Test
    @DisplayName("Un statut absent ou illisible ne rend rien et ne fausse pas la suite")
    void anUnreadableStatusSaysNothing() {
        CalibrationWatch watch = new CalibrationWatch();
        assertNull(watch.observe(null));
        assertNull(watch.observe(Map.of()));
        assertNull(watch.observe(Map.of("decision", Map.of("aa_runs", "deux"))));
        assertEquals(2, watch.observe(status(2, 0)).runs(), "le relevé illisible n'a rien effacé");
    }
}
