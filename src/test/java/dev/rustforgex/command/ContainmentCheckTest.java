package dev.rustforgex.command;

import dev.rustforgex.bridge.FakeNativeBridge;
import dev.rustforgex.diag.Incident;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * {@code /rfx panic-test} (C-38, T-420) : le confinement est vérifié en mode
 * {@code debug}, et nulle part ailleurs.
 */
class ContainmentCheckTest {

    /** Pont qui compte ses appels et rend une valeur choisie. */
    private static final class CountingBridge extends FakeNativeBridge {
        int calls;
        int result = -3001;

        @Override
        public int panicTest(long handle) {
            calls++;
            return result;
        }
    }

    private static final Map<String, Object> STATUS = Map.of("state", "RUNNING", "panics", 1L);

    @Test
    @DisplayName("Hors du mode debug, le natif n'est pas appelé")
    void outsideDebugNothingIsCalled() {
        CountingBridge bridge = new CountingBridge();
        List<Incident> incidents = new ArrayList<>();

        for (String mode : List.of("safe", "balanced", "performance", "experimental")) {
            ContainmentCheck.Outcome outcome =
                    ContainmentCheck.run(bridge, 1L, mode, () -> STATUS, incidents::add);
            assertEquals(ContainmentCheck.Verdict.REFUSED, outcome.verdict(), mode);
        }

        assertEquals(0, bridge.calls, "une panic, même confinée, est comptée par le runtime");
        assertTrue(incidents.isEmpty());
    }

    @Test
    @DisplayName("En mode debug, une panic confinée rend E-3001 et devient un incident")
    void aContainedPanicIsReportedAndRecorded() {
        CountingBridge bridge = new CountingBridge();
        List<Incident> incidents = new ArrayList<>();

        ContainmentCheck.Outcome outcome =
                ContainmentCheck.run(bridge, 1L, "debug", () -> STATUS, incidents::add);

        assertEquals(ContainmentCheck.Verdict.CONFINED, outcome.verdict());
        assertEquals(1, bridge.calls);
        assertEquals("RUNNING", outcome.state());
        assertEquals(1L, outcome.panics());
        assertEquals(1, incidents.size());
        assertEquals(Incident.Kind.NATIVE_PANIC, incidents.get(0).kind());
        assertEquals("panic-test", incidents.get(0).subject());
    }

    @Test
    @DisplayName("Un natif qui rend autre chose que E-3001 n'est pas déclaré confiné")
    void anythingButE3001IsNotConfinement() {
        CountingBridge bridge = new CountingBridge();
        bridge.result = 0;
        List<Incident> incidents = new ArrayList<>();

        ContainmentCheck.Outcome outcome =
                ContainmentCheck.run(bridge, 1L, "debug", () -> null, incidents::add);

        assertEquals(ContainmentCheck.Verdict.NOT_CONFINED, outcome.verdict());
        assertEquals(0, outcome.code());
        assertEquals(-1, outcome.panics(), "statut illisible : aucun compte inventé");
        assertTrue(incidents.isEmpty());
    }

    @Test
    @DisplayName("Sans runtime natif, le test répond qu'il est impossible")
    void withoutRuntimeItSaysSo() {
        ContainmentCheck.Outcome outcome =
                ContainmentCheck.run(null, 0L, "debug", () -> STATUS, incident -> { });

        assertEquals(ContainmentCheck.Verdict.UNAVAILABLE, outcome.verdict());
        assertFalse(ContainmentCheck.lines(outcome).isEmpty());
    }
}
