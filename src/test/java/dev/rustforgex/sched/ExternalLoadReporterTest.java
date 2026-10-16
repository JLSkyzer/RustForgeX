package dev.rustforgex.sched;

import dev.rustforgex.bridge.FakeNativeBridge;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.lang.management.ManagementFactory;
import java.util.ArrayList;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Tests de la transmission de la charge externe au natif (C-17, R-421, ADR-046). */
class ExternalLoadReporterTest {

    /** Pont qui consigne les relevés reçus et rend le code choisi par le test. */
    private static final class RecordingBridge extends FakeNativeBridge {
        final List<Integer> received = new ArrayList<>();
        int answer;

        @Override
        public int schedReportExternalLoad(long handle, int runnableThreads) {
            received.add(runnableThreads);
            return answer;
        }
    }

    /** Un groupe sans aucun fil : le relevé vaut zéro, connu d'avance. */
    private static ExternalLoadSensor emptySensor() {
        return new ExternalLoadSensor(new ThreadGroup("rfx-test-vide"), 8,
                ManagementFactory.getThreadMXBean());
    }

    @Test
    @DisplayName("Le premier relevé sert de référence et n'est pas transmis")
    void theFirstSampleIsAReferenceAndIsNotSent() {
        RecordingBridge bridge = new RecordingBridge();
        ExternalLoadReporter reporter = new ExternalLoadReporter(
                emptySensor(), bridge, FakeNativeBridge.HANDLE, code -> { });

        reporter.reportOnce();
        assertTrue(bridge.received.isEmpty());

        reporter.reportOnce();
        reporter.reportOnce();
        assertEquals(List.of(0, 0), bridge.received);
        assertEquals(2, reporter.reports());
        assertEquals(0, reporter.lastSample());
    }

    @Test
    @DisplayName("Un refus du natif est compté et transmis, sans exception")
    void aNativeRefusalIsCountedAndForwarded() {
        RecordingBridge bridge = new RecordingBridge();
        bridge.answer = -3001;
        List<Integer> errors = new ArrayList<>();
        ExternalLoadReporter reporter = new ExternalLoadReporter(
                emptySensor(), bridge, FakeNativeBridge.HANDLE, errors::add);

        reporter.reportOnce();
        reporter.reportOnce();

        assertEquals(List.of(-3001), errors);
        assertEquals(1, reporter.rejected());
        assertEquals(0, reporter.reports());
    }

    @Test
    @DisplayName("Le fil de relevé démarre, porte son nom, et s'arrête")
    void theReportingThreadStartsAndStops() {
        ExternalLoadReporter reporter = new ExternalLoadReporter(
                emptySensor(), new RecordingBridge(), FakeNativeBridge.HANDLE, code -> { });
        try {
            reporter.start();
            assertTrue(reporter.running());
            boolean named = Thread.getAllStackTraces().keySet().stream()
                    .anyMatch(thread -> thread.getName().equals(ExternalLoadReporter.THREAD_NAME));
            assertTrue(named, "le fil s'appelle " + ExternalLoadReporter.THREAD_NAME);
        } finally {
            reporter.stop();
        }
        assertFalse(reporter.running());
    }
}
