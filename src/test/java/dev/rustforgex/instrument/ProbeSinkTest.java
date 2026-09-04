package dev.rustforgex.instrument;

import dev.rustforgex.bridge.FakeNativeBridge;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Tests de l'écriture des enregistrements de sonde (IF-03, côté Java). */
class ProbeSinkTest {

    private static final long HANDLE = 0x5246_5800_0000_0001L;

    /** Pont simulé : alloue de vrais tampons directs et consigne les vidages. */
    private static class FakeBridge extends FakeNativeBridge {

        final Map<Integer, ByteBuffer> buffers = new HashMap<>();
        final List<int[]> flushes = new ArrayList<>();
        int bufferBytes = 1024;
        int maxThreads = 8;

        @Override
        public ByteBuffer probeBufferAcquire(long handle, int threadId) {
            if (buffers.size() >= maxThreads && !buffers.containsKey(threadId)) {
                return null;
            }
            return buffers.computeIfAbsent(threadId,
                    id -> ByteBuffer.allocateDirect(bufferBytes).order(ByteOrder.LITTLE_ENDIAN));
        }

        @Override
        public int probeBufferFlush(long handle, int threadId, int used) {
            flushes.add(new int[] {threadId, used});
            return 0;
        }













    }

    @Test
    @DisplayName("Un enregistrement est écrit à la disposition exacte de la PARTIE 6.4")
    void aRecordMatchesTheSpecifiedLayout() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        assertTrue(sink.record(0x0102_0304, ProbeSink.KIND_EXIT, (short) 0x1234, 42_000L, 1_500L));

        ByteBuffer buffer = bridge.buffers.get(0);
        assertEquals(0x0102_0304, buffer.getInt(0), "probe_id");
        assertEquals(ProbeSink.KIND_EXIT, buffer.get(4), "kind");
        assertEquals(0, buffer.get(5), "flags");
        assertEquals((short) 0x1234, buffer.getShort(6), "context_hash16");
        assertEquals(42_000L, buffer.getLong(8), "timestamp_ns");
        assertEquals(1_500L, buffer.getLong(16), "value");
    }

    @Test
    @DisplayName("Les enregistrements se suivent sans trou")
    void recordsAreWrittenBackToBack() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        for (int i = 0; i < 4; i++) {
            assertTrue(sink.record(i, ProbeSink.KIND_ENTER, (short) 0, i, i * 10L));
        }

        ByteBuffer buffer = bridge.buffers.get(0);
        for (int i = 0; i < 4; i++) {
            assertEquals(i, buffer.getInt(i * ProbeSink.RECORD_SIZE), "enregistrement " + i);
        }
        assertEquals(4, sink.recordsWritten());
    }

    @Test
    @DisplayName("Le vidage annonce les octets écrits et remet le curseur à zéro")
    void flushReportsAndResets() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        sink.record(1, ProbeSink.KIND_ENTER, (short) 0, 1, 1);
        sink.record(2, ProbeSink.KIND_EXIT, (short) 0, 2, 2);

        assertEquals(2 * ProbeSink.RECORD_SIZE, sink.flush());
        assertEquals(1, bridge.flushes.size());
        assertEquals(2 * ProbeSink.RECORD_SIZE, bridge.flushes.get(0)[1]);

        // Le tampon repart de zéro : le prochain enregistrement écrase le premier.
        sink.record(99, ProbeSink.KIND_ENTER, (short) 0, 3, 3);
        assertEquals(99, bridge.buffers.get(0).getInt(0));
    }

    @Test
    @DisplayName("Un thread qui n'a rien écrit ne traverse pas la frontière")
    void anIdleThreadDoesNotCrossTheBoundary() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        assertEquals(0, sink.flush());
        assertTrue(bridge.flushes.isEmpty(), "aucun appel natif pour un thread inactif");
    }

    /// R-709 : la saturation écarte les enregistrements et les compte.
    @Test
    @DisplayName("R-709 : un tampon plein écarte les enregistrements et les signale")
    void aFullBufferDropsAndReportsRecords() {
        FakeBridge bridge = new FakeBridge();
        bridge.bufferBytes = 2 * ProbeSink.RECORD_SIZE;
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        assertTrue(sink.record(1, ProbeSink.KIND_ENTER, (short) 0, 1, 1));
        assertTrue(sink.record(2, ProbeSink.KIND_ENTER, (short) 0, 2, 2));
        assertFalse(sink.record(3, ProbeSink.KIND_ENTER, (short) 0, 3, 3), "tampon plein");
        assertFalse(sink.record(4, ProbeSink.KIND_ENTER, (short) 0, 4, 4));

        assertEquals(2, sink.recordsWritten());
        assertEquals(2, sink.recordsDropped());

        // Le vidage annonce les deux écrits plus les deux écartés, pour que le natif
        // les compte perdus.
        sink.flush();
        assertEquals(4 * ProbeSink.RECORD_SIZE, bridge.flushes.get(0)[1]);
    }

    @Test
    @DisplayName("Après vidage, un tampon saturé accepte de nouveau des enregistrements")
    void aSaturatedBufferRecoversAfterFlush() {
        FakeBridge bridge = new FakeBridge();
        bridge.bufferBytes = ProbeSink.RECORD_SIZE;
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        assertTrue(sink.record(1, ProbeSink.KIND_ENTER, (short) 0, 1, 1));
        assertFalse(sink.record(2, ProbeSink.KIND_ENTER, (short) 0, 2, 2));

        sink.flush();

        assertTrue(sink.record(3, ProbeSink.KIND_ENTER, (short) 0, 3, 3), "place à nouveau libre");
    }

    @Test
    @DisplayName("Un thread refusé par le natif n'est pas sondé, sans erreur")
    void aRefusedThreadIsSimplyNotProbed() {
        FakeBridge bridge = new FakeBridge();
        bridge.maxThreads = 0;
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        assertFalse(sink.record(1, ProbeSink.KIND_ENTER, (short) 0, 1, 1));
        assertEquals(1, sink.refusedThreads());
        assertEquals(0, sink.recordsWritten());
        assertEquals(0, sink.flush(), "rien à vider");
    }

    @Test
    @DisplayName("Chaque thread écrit dans son propre tampon")
    void eachThreadGetsItsOwnBuffer() throws Exception {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        sink.record(100, ProbeSink.KIND_ENTER, (short) 0, 1, 1);

        Thread other = new Thread(() -> {
            sink.record(200, ProbeSink.KIND_ENTER, (short) 0, 2, 2);
            sink.flush();
        });
        other.start();
        other.join();

        assertEquals(2, bridge.buffers.size(), "un tampon par thread");
        // Chaque tampon porte l'enregistrement de son thread, à sa propre position 0.
        List<Integer> firstIds = bridge.buffers.values().stream()
                .map(b -> b.getInt(0))
                .sorted()
                .toList();
        assertEquals(List.of(100, 200), firstIds);
    }
}
