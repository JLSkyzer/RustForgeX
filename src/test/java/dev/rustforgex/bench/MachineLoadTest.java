package dev.rustforgex.bench;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

/** Résumé de la charge de la machine hors serveur (C-36, ADR-042). */
class MachineLoadTest {

    @Test
    @DisplayName("Médiane, 95e centile au rang le plus proche et maximum")
    void theSummaryUsesNearestRankQuantiles() {
        double[] samples = new double[20];
        for (int i = 0; i < samples.length; i++) {
            samples[i] = (samples.length - i) / 100.0;
        }
        MachineLoad.Summary summary = MachineLoad.summarize(samples);
        assertEquals(20, summary.samples());
        assertEquals(0.10, summary.p50(), 1e-12);
        assertEquals(0.19, summary.p95(), 1e-12);
        assertEquals(0.20, summary.max(), 1e-12);
    }

    @Test
    @DisplayName("Sans relevé, un résumé vide que l'agrégateur tient pour non consigné")
    void noSampleGivesAnEmptySummary() {
        MachineLoad.Summary summary = MachineLoad.summarize(new double[0]);
        assertEquals(0, summary.samples());
        assertEquals(0.0, summary.p95());
    }
}
