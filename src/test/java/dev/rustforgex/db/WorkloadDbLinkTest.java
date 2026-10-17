package dev.rustforgex.db;

import dev.rustforgex.bridge.CborReader;
import dev.rustforgex.bridge.FakeNativeBridge;
import dev.rustforgex.forge.ModDiscovery;
import dev.rustforgex.forge.ModEntry;
import dev.rustforgex.forge.ModSource;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Deque;
import java.util.List;
import java.util.Map;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** C-09 côté Java : ouverture et synchronisation de la base (ADR-049). */
class WorkloadDbLinkTest {

    private static final Map<String, String> VERSIONS =
            Map.of("minecraft", "1.20.1", "forge", "47.4.0", "java", "17.0.12 (Test)");

    /** Pont qui note ses appels et rend les réponses prévues pour {@code dbSync}. */
    private static final class RecordingBridge extends FakeNativeBridge {
        final List<byte[]> opens = Collections.synchronizedList(new ArrayList<>());
        final List<String> threads = Collections.synchronizedList(new ArrayList<>());
        final Deque<Integer> syncReplies = new ArrayDeque<>();
        volatile int openReply;
        volatile int syncs;

        @Override
        public int dbOpen(long handle, byte[] openCbor) {
            opens.add(openCbor);
            threads.add(Thread.currentThread().getName());
            return openReply;
        }

        @Override
        public synchronized int dbSync(long handle) {
            syncs++;
            threads.add(Thread.currentThread().getName());
            Integer reply = syncReplies.poll();
            return reply == null ? 1 : reply;
        }
    }

    private static List<ModEntry> mods() {
        ModSource source = () -> List.of(
                new ModSource.RawMod("alpha", "1.0", null, null, Set.of("a.b")),
                new ModSource.RawMod("beta", "2.3", null, null, Set.of("c.d")));
        return ModDiscovery.from(source).mods();
    }

    private static WorkloadDbLink link(RecordingBridge bridge, List<Integer> errors,
            long periodMs) {
        return new WorkloadDbLink(bridge, FakeNativeBridge.HANDLE, Path.of("cache"), VERSIONS,
                errors::add, periodMs);
    }

    @Test
    @DisplayName("Le blob d'ouverture porte le répertoire, les versions et les mods")
    void theOpenBlobMatchesDbOpen() throws CborReader.InvalidCbor {
        byte[] blob = WorkloadDbLink.openBlob(Path.of("cache"), VERSIONS,
                List.of("alpha@1.0@x", "beta@2.3@y"));
        @SuppressWarnings("unchecked")
        Map<String, Object> decoded = (Map<String, Object>) CborReader.decode(blob);
        assertEquals(1L, decoded.get("schema"));
        assertEquals(Path.of("cache").toAbsolutePath().toString(), decoded.get("dir"));
        assertEquals("1.20.1", decoded.get("minecraft"));
        assertEquals("47.4.0", decoded.get("forge"));
        assertEquals("17.0.12 (Test)", decoded.get("java"));
        assertEquals(List.of("alpha@1.0@x", "beta@2.3@y"), decoded.get("mods"));
    }

    @Test
    @DisplayName("Chaque mod entre dans l'empreinte avec sa version et l'empreinte de son jar")
    void theModSetCarriesIdVersionAndHash() {
        List<String> set = WorkloadDbLink.modSet(mods());
        assertEquals(2, set.size());
        assertTrue(set.contains("alpha@1.0@" + ModEntry.NO_HASH), set.toString());
        assertTrue(set.contains("beta@2.3@" + ModEntry.NO_HASH), set.toString());
    }

    @Test
    @DisplayName("Un refus du natif est compté et transmis, sans exception")
    void aRefusedOpenIsCountedAndReported() {
        RecordingBridge bridge = new RecordingBridge();
        bridge.openReply = -1000;
        List<Integer> errors = new ArrayList<>();
        WorkloadDbLink link = link(bridge, errors, 60_000L);
        assertFalse(link.open(mods()));
        assertFalse(link.opened());
        assertEquals(1, link.rejected());
        assertEquals(List.of(-1000), errors);
    }

    @Test
    @DisplayName("Un cycle appelle le natif tranche par tranche, jusqu'à la fin annoncée")
    void aCycleWalksSlicesUntilDone() throws InterruptedException {
        RecordingBridge bridge = new RecordingBridge();
        bridge.syncReplies.addAll(List.of(0, 0, 1));
        WorkloadDbLink link = link(bridge, new ArrayList<>(), 60_000L);
        link.syncCycle();
        assertEquals(3, bridge.syncs);
        assertEquals(1, link.cycles());
    }

    @Test
    @DisplayName("Une erreur arrête le cycle, comptée et transmise")
    void anErrorStopsTheCycle() throws InterruptedException {
        RecordingBridge bridge = new RecordingBridge();
        bridge.syncReplies.addAll(List.of(0, -3001, 1));
        List<Integer> errors = new ArrayList<>();
        WorkloadDbLink link = link(bridge, errors, 60_000L);
        link.syncCycle();
        assertEquals(2, bridge.syncs);
        assertEquals(0, link.cycles());
        assertEquals(List.of(-3001), errors);
    }

    @Test
    @DisplayName("Le fil rustforgex-db ouvre la base puis la synchronise, hors du fil appelant")
    void theThreadOpensThenSyncs() throws InterruptedException {
        RecordingBridge bridge = new RecordingBridge();
        WorkloadDbLink link = link(bridge, new ArrayList<>(), 10L);
        link.start(mods());
        long deadline = System.nanoTime() + 10_000_000_000L;
        while (link.cycles() < 2 && System.nanoTime() < deadline) {
            Thread.sleep(5L);
        }
        link.stop();
        assertTrue(link.opened());
        assertTrue(link.cycles() >= 2, "cycles : " + link.cycles());
        assertFalse(link.running());
        assertEquals(1, bridge.opens.size());
        synchronized (bridge.threads) {
            assertTrue(bridge.threads.stream().allMatch(WorkloadDbLink.THREAD_NAME::equals),
                    "appels hors du fil : " + bridge.threads);
        }
    }
}
