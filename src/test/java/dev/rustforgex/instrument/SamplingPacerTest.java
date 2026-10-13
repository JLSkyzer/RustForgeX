package dev.rustforgex.instrument;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Cadence de l'échantillonnage réglée sur son coût (C-05, ADR-037, T-140).
 */
class SamplingPacerTest {

    @Test
    @DisplayName("Des prélèvements bon marché gardent la cadence d'origine, 100 Hz")
    void cheapSamplesKeepTheOriginalRate() {
        SamplingPacer pacer = new SamplingPacer(0.005);
        for (int i = 0; i < 100; i++) {
            pacer.record(20_000);
        }
        // 20 µs / 0,5 % = 4 ms, sous le plancher de 10 ms.
        assertEquals(SamplingPacer.MIN_PERIOD_NS, pacer.periodNs());
    }

    @Test
    @DisplayName("Des prélèvements coûteux ralentissent la cadence jusqu'à la part de budget")
    void expensiveSamplesSlowTheRateDownToTheBudget() {
        SamplingPacer pacer = new SamplingPacer(0.005);
        for (int i = 0; i < 200; i++) {
            pacer.record(200_000);
        }
        // 200 µs / 0,5 % = 40 ms, soit 25 Hz : 0,5 % du temps passé à suspendre le fil.
        assertEquals(40_000_000L, pacer.periodNs(), 1_000_000L);
    }

    @Test
    @DisplayName("La cadence ne descend jamais sous un prélèvement par seconde")
    void theRateNeverStops() {
        SamplingPacer pacer = new SamplingPacer(0.005);
        for (int i = 0; i < 100; i++) {
            pacer.record(50_000_000);
        }
        assertEquals(SamplingPacer.MAX_PERIOD_NS, pacer.periodNs());
    }

    @Test
    @DisplayName("Un pic isolé ne fait pas sauter la cadence : le coût est lissé")
    void anIsolatedSpikeIsSmoothed() {
        SamplingPacer pacer = new SamplingPacer(0.005);
        for (int i = 0; i < 100; i++) {
            pacer.record(20_000);
        }
        pacer.record(2_000_000);
        // Lissé : 20 µs + 0,1 × (2 ms − 20 µs) ≈ 218 µs, soit ≈ 44 ms et non 400 ms.
        assertTrue(pacer.periodNs() < 50_000_000L, "période " + pacer.periodNs());
        for (int i = 0; i < 100; i++) {
            pacer.record(20_000);
        }
        assertEquals(SamplingPacer.MIN_PERIOD_NS, pacer.periodNs(), "retour à 100 Hz");
    }

    @Test
    @DisplayName("Une part de budget hors de ]0, 1] est refusée")
    void anInvalidBudgetIsRefused() {
        assertThrows(IllegalArgumentException.class, () -> new SamplingPacer(0.0));
        assertThrows(IllegalArgumentException.class, () -> new SamplingPacer(1.5));
        assertThrows(IllegalArgumentException.class, () -> new SamplingPacer(Double.NaN));
    }
}
