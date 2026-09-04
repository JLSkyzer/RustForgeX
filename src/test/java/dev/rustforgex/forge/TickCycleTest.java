package dev.rustforgex.forge;

import dev.rustforgex.bridge.NativeBridge;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests du pilotage de la fenêtre de tick (C-01, IF-02).
 *
 * <p>Le pont est simulé : ce qui est vérifié ici, c'est la <em>séquence</em> d'appels
 * émise vers le natif. La conformité de la machine à états elle-même est vérifiée du
 * côté Rust, dans `rfx-core::tick`.
 */
class TickCycleTest {

    private static final long HANDLE = 0x5246_5800_0000_0001L;

    /** Pont simulé : consigne la séquence d'appels du cycle de tick. */
    private static final class RecordingBridge implements NativeBridge {

        final List<String> calls = new ArrayList<>();
        int beginResult;
        int phaseResult;
        long endResult;

        @Override
        public int tickBegin(long handle, long tick, int side) {
            calls.add("begin(" + tick + "," + side + ")");
            return beginResult;
        }

        @Override
        public int tickPhase(long handle, int phase) {
            calls.add("phase(" + phase + ")");
            return phaseResult;
        }

        @Override
        public long tickEnd(long handle) {
            calls.add("end");
            return endResult;
        }

        // --- Points d'entrée sans rapport avec ce test -----------------------

        @Override
        public int abiVersion() {
            return 1;
        }

        @Override
        public long init(byte[] configCbor) {
            return HANDLE;
        }

        @Override
        public int shutdown(long handle) {
            return 0;
        }

        @Override
        public int noop(long handle) {
            return 0;
        }

        @Override
        public int hwProbe(long handle) {
            return 0;
        }

        @Override
        public int hwSetFfiCosts(long handle, int jniCallNs, int ffiBatchNsPerKb) {
            return 0;
        }

        @Override
        public long transferProbe(long handle, ByteBuffer buffer, int length) {
            return length;
        }

        @Override
        public byte[] status(long handle) {
            return new byte[0];
        }

        @Override
        public int panicTest(long handle) {
            return -3001;
        }
    }

    @Test
    @DisplayName("Un tick nominal émet la séquence complète de SM-04")
    void aNominalTickEmitsTheFullSequence() {
        RecordingBridge bridge = new RecordingBridge();
        TickCycle cycle = new TickCycle(bridge, HANDLE, TickCycle.SIDE_SERVER);

        cycle.onTickPre();
        cycle.onTickPost();

        assertEquals(
                List.of("begin(1,1)", "phase(0)", "phase(1)", "phase(2)", "phase(3)", "end"),
                bridge.calls,
                "PRE puis VANILLA au START, DRAIN puis POST à la fin");
        assertFalse(cycle.windowOpen());
        assertEquals(1, cycle.currentTick());
        assertEquals(0, cycle.rejectedCalls());
    }

    @Test
    @DisplayName("Le numéro de tick progresse à chaque ouverture")
    void theTickNumberAdvances() {
        RecordingBridge bridge = new RecordingBridge();
        TickCycle cycle = new TickCycle(bridge, HANDLE, TickCycle.SIDE_SERVER);

        for (int i = 0; i < 5; i++) {
            cycle.onTickPre();
            cycle.onTickPost();
        }
        assertEquals(5, cycle.currentTick());
        assertTrue(bridge.calls.contains("begin(5,1)"), bridge.calls.toString());
    }

    @Test
    @DisplayName("Le côté transmis est celui du contexte d'exécution")
    void theSideIsForwarded() {
        RecordingBridge bridge = new RecordingBridge();
        new TickCycle(bridge, HANDLE, TickCycle.SIDE_CLIENT).onTickPre();
        assertTrue(bridge.calls.contains("begin(1,0)"), bridge.calls.toString());
    }

    @Test
    @DisplayName("R-706 : un POST sans PRE n'émet rien et laisse le natif refermer la fenêtre")
    void aPostWithoutPreEmitsNothing() {
        RecordingBridge bridge = new RecordingBridge();
        TickCycle cycle = new TickCycle(bridge, HANDLE, TickCycle.SIDE_SERVER);

        assertEquals(0, cycle.onTickPost());

        assertTrue(bridge.calls.isEmpty(),
                "aucun appel ne doit être émis hors d'une fenêtre ouverte : " + bridge.calls);
    }

    @Test
    @DisplayName("Un refus d'ouverture n'ouvre pas la fenêtre et n'émet aucune phase")
    void aRefusedBeginStopsTheSequence() {
        RecordingBridge bridge = new RecordingBridge();
        bridge.beginResult = -1000;
        TickCycle cycle = new TickCycle(bridge, HANDLE, TickCycle.SIDE_SERVER);

        cycle.onTickPre();

        assertEquals(List.of("begin(1,1)"), bridge.calls, "aucune phase après un refus");
        assertFalse(cycle.windowOpen());
        assertEquals(1, cycle.rejectedCalls());

        // Le POST qui suit ne doit rien émettre non plus.
        cycle.onTickPost();
        assertEquals(List.of("begin(1,1)"), bridge.calls);
    }

    @Test
    @DisplayName("Les codes d'erreur du natif sont comptés, jamais propagés")
    void nativeErrorCodesAreCountedNotThrown() {
        RecordingBridge bridge = new RecordingBridge();
        bridge.phaseResult = -1000;
        bridge.endResult = -1000;
        TickCycle cycle = new TickCycle(bridge, HANDLE, TickCycle.SIDE_SERVER);

        cycle.onTickPre();
        assertEquals(0, cycle.onTickPost(), "un échec de fermeture ne rend aucun drapeau");

        // Deux phases refusées à l'ouverture, deux à la fermeture, plus la fermeture.
        assertEquals(5, cycle.rejectedCalls());
        assertFalse(cycle.windowOpen(), "la fenêtre est refermée quoi qu'il arrive");
    }

    @Test
    @DisplayName("Les drapeaux du tick sont rendus tels quels")
    void tickFlagsAreForwarded() {
        RecordingBridge bridge = new RecordingBridge();
        bridge.endResult = 1;
        TickCycle cycle = new TickCycle(bridge, HANDLE, TickCycle.SIDE_SERVER);

        cycle.onTickPre();
        assertEquals(1, cycle.onTickPost(), "le drapeau de dépassement de deadline remonte");
    }
}
