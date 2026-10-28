package dev.rustforgex.analysis;

import dev.rustforgex.bridge.Cbor;
import dev.rustforgex.bridge.CborReader;
import dev.rustforgex.bridge.FakeNativeBridge;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.Opcodes;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Deque;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** C-07 côté Java : hiérarchie, pas et classes fournies (ADR-051). */
class CallGraphLinkTest {

    private static final int PUBLIC = Opcodes.ACC_PUBLIC | Opcodes.ACC_SUPER;

    /** Bibliothèque en mémoire, qui note ses lectures. */
    private static final class MemoryLibrary implements ClassLibrary {
        final Map<String, byte[]> classes = new LinkedHashMap<>();
        final Set<String> transformed = new HashSet<>();
        /** Classes d'une couche parente ; les autres sont de la couche du jeu. */
        final Set<String> platform = new HashSet<>();
        final List<String> reads = Collections.synchronizedList(new ArrayList<>());
        volatile boolean failMidway;
        volatile boolean closed;

        MemoryLibrary add(String key, byte[] bytes) {
            classes.put(key, bytes);
            return this;
        }

        MemoryLibrary plain(String name) {
            return add(name, TestClasses.plain(name, "java/lang/Object", PUBLIC));
        }

        @Override
        public boolean forEachClass(ClassSink sink) throws IOException {
            int index = 0;
            for (Map.Entry<String, byte[]> entry : classes.entrySet()) {
                if (failMidway && index++ == classes.size() / 2) {
                    throw new IOException("archive illisible");
                }
                if (!sink.accept(entry.getValue(), !platform.contains(entry.getKey()))) {
                    return false;
                }
            }
            return true;
        }

        @Override
        public byte[] read(String internalName) {
            reads.add(internalName);
            return classes.get(internalName);
        }

        @Override
        public Set<String> transformedClasses() {
            return transformed;
        }

        @Override
        public void close() {
            closed = true;
        }
    }

    /** Pont qui note les appels d'analyse et rend les réponses prévues. */
    private static final class RecordingBridge extends FakeNativeBridge {
        final List<List<?>> batches = Collections.synchronizedList(new ArrayList<>());
        final List<String> threads = Collections.synchronizedList(new ArrayList<>());
        final List<String> classNames = new ArrayList<>();
        final List<Boolean> classGiven = new ArrayList<>();
        final Deque<Integer> steps = new ArrayDeque<>();
        volatile byte[] requests;
        volatile int hierarchyReply;
        volatile int classReply;
        volatile int stepCalls;

        @Override
        public int analysisHierarchy(long handle, byte[] entriesCbor) {
            threads.add(Thread.currentThread().getName());
            try {
                batches.add((List<?>) CborReader.decode(entriesCbor));
            } catch (CborReader.InvalidCbor e) {
                throw new AssertionError(e);
            }
            return hierarchyReply;
        }

        @Override
        public int analysisClass(long handle, byte[] nameUtf8, byte[] bytes) {
            classNames.add(new String(nameUtf8, StandardCharsets.UTF_8));
            classGiven.add(bytes != null);
            return classReply;
        }

        @Override
        public synchronized int analysisStep(long handle) {
            stepCalls++;
            Integer reply = steps.poll();
            return reply == null ? 0 : reply;
        }

        @Override
        public byte[] analysisRequests(long handle) {
            return requests;
        }
    }

    private static CallGraphLink link(RecordingBridge bridge, MemoryLibrary library,
            List<Integer> errors) {
        return new CallGraphLink(bridge, FakeNativeBridge.HANDLE, () -> library, 50,
                errors::add, 10L);
    }

    @Test
    @DisplayName("La hiérarchie part par lots, complète, avant tout pas")
    void theHierarchyIsDeclaredInBatches() throws Exception {
        MemoryLibrary library = new MemoryLibrary();
        for (int index = 0; index < CallGraphLink.HIERARCHY_BATCH + 3; index++) {
            library.plain("p/C" + index);
        }
        library.add("p/Final", TestClasses.plain("p/Final", "p/C0", PUBLIC | Opcodes.ACC_FINAL));
        library.add("garbage", "pas une classe".getBytes(StandardCharsets.UTF_8));
        library.add("m/Mixin", TestClasses.mixin("m/Mixin", List.of("p/C1"), List.of("p.C2")));
        RecordingBridge bridge = new RecordingBridge();
        CallGraphLink link = link(bridge, library, new ArrayList<>());
        Set<String> excluded = new HashSet<>(Set.of("p/Final"));

        assertEquals(0, link.step(library, excluded), "pas de pas avant la hiérarchie");
        assertEquals(0, bridge.stepCalls);

        assertTrue(link.declareHierarchy(library, excluded));
        assertTrue(link.hierarchyReady());
        assertEquals(2, bridge.batches.size());
        assertEquals(CallGraphLink.HIERARCHY_BATCH, bridge.batches.get(0).size());
        long expected = CallGraphLink.HIERARCHY_BATCH + 3 + 2;
        assertEquals(expected, link.declared(), "toutes les classes lisibles, mixin compris");
        assertEquals(expected, bridge.batches.stream().mapToLong(List::size).sum());
        assertEquals(1, link.unreadable());
        assertEquals(Set.of("p/Final", "p/C1", "p/C2"), excluded);
        assertEquals(3, link.hidden());

        List<?> finalEntry = bridge.batches.get(1).stream()
                .map(entry -> (List<?>) entry)
                .filter(entry -> "p/Final".equals(entry.get(0)))
                .findFirst().orElseThrow();
        assertEquals("p/C0", finalEntry.get(1));
        assertEquals((long) PUBLIC, finalEntry.get(2), "final retiré : transformée au chargement");
        List<?> first = (List<?>) bridge.batches.get(0).get(0);
        assertEquals(List.of("p/C0", "java/lang/Object", (long) PUBLIC), first);
    }

    @Test
    @DisplayName("Un pas fournit le bytecode demandé, ou l'absence pour une classe exclue")
    void stepsProvideBytesOrAbsence() throws Exception {
        MemoryLibrary library = new MemoryLibrary().plain("p/A").plain("p/Hidden");
        RecordingBridge bridge = new RecordingBridge();
        CallGraphLink link = link(bridge, library, new ArrayList<>());
        Set<String> excluded = new HashSet<>(Set.of("p/Hidden"));
        assertTrue(link.declareHierarchy(library, excluded));

        bridge.steps.addAll(List.of(1, 2, 0));
        bridge.requests = Cbor.encode(List.of("p/A", "p/Hidden", "p/Missing"));
        assertEquals(1, link.step(library, excluded));
        assertEquals(List.of("p/A", "p/Hidden", "p/Missing"), bridge.classNames);
        assertEquals(List.of(true, false, false), bridge.classGiven);
        assertEquals(List.of("p/A", "p/Missing"), library.reads, "une classe exclue n'est pas lue");
        assertEquals(1, link.provided());
        assertEquals(2, link.absent());

        assertEquals(2, link.step(library, excluded));
        assertEquals(1, link.graphs());
        assertEquals(0, link.step(library, excluded));
    }

    @Test
    @DisplayName("Une classe mixin illisible : aucun pas, ses cibles sont inconnues")
    void anUnreadableMixinBlocksTheAnalysis() throws Exception {
        byte[] mixin = TestClasses.mixin("m/M", List.of("p/A"), List.of());
        MemoryLibrary library = new MemoryLibrary().plain("p/A")
                .add("m/M", Arrays.copyOf(mixin, mixin.length - 12));
        RecordingBridge bridge = new RecordingBridge();
        CallGraphLink link = link(bridge, library, new ArrayList<>());
        assertFalse(link.declareHierarchy(library, new HashSet<>()));
        assertFalse(link.hierarchyReady());
        assertEquals(1, link.unreadableMixins());
        assertEquals(0, link.step(library, Set.of()));
        assertEquals(0, bridge.stepCalls);
    }

    @Test
    @DisplayName("Une classe du jeu qui peut définir des classes : aucun pas (ADR-059)")
    void aClassDefinerInTheGameLayerBlocksTheAnalysis() throws Exception {
        MemoryLibrary game = new MemoryLibrary().plain("p/A")
                .add("p/D", TestClasses.definer("p/D"));
        RecordingBridge bridge = new RecordingBridge();
        CallGraphLink link = link(bridge, game, new ArrayList<>());
        assertFalse(link.declareHierarchy(game, new HashSet<>()));
        assertFalse(link.hierarchyReady());
        assertEquals(1, link.definers());
        assertEquals(0, link.step(game, Set.of()));
        assertEquals(0, bridge.stepCalls);

        // La même classe dans une couche parente (chargeur de la plateforme) : sans effet.
        MemoryLibrary parent = new MemoryLibrary().plain("p/A")
                .add("p/D", TestClasses.definer("p/D"));
        parent.platform.add("p/D");
        CallGraphLink ready = link(new RecordingBridge(), parent, new ArrayList<>());
        assertTrue(ready.declareHierarchy(parent, new HashSet<>()));
        assertEquals(0, ready.definers());
    }

    @Test
    @DisplayName("Un lot refusé ou une archive illisible : aucun pas")
    void anIncompleteHierarchyBlocksTheAnalysis() throws Exception {
        MemoryLibrary library = new MemoryLibrary().plain("p/A").plain("p/B");
        RecordingBridge refusing = new RecordingBridge();
        refusing.hierarchyReply = -1000;
        List<Integer> errors = new ArrayList<>();
        CallGraphLink refused = link(refusing, library, errors);
        assertFalse(refused.declareHierarchy(library, new HashSet<>()));
        assertEquals(List.of(-1000), errors);
        assertEquals(1, refused.rejected());
        assertFalse(refused.hierarchyReady());

        library.failMidway = true;
        CallGraphLink failing = link(new RecordingBridge(), library, new ArrayList<>());
        assertThrows(IOException.class, () -> failing.declareHierarchy(library, new HashSet<>()));
        assertFalse(failing.hierarchyReady());
    }

    @Test
    @DisplayName("Un refus du natif pendant un pas est compté et transmis")
    void refusalsDuringAStepAreReported() throws Exception {
        MemoryLibrary library = new MemoryLibrary().plain("p/A");
        RecordingBridge bridge = new RecordingBridge();
        List<Integer> errors = new ArrayList<>();
        CallGraphLink link = link(bridge, library, errors);
        assertTrue(link.declareHierarchy(library, new HashSet<>()));

        bridge.steps.add(-3001);
        assertEquals(-3001, link.step(library, Set.of()));
        bridge.steps.add(1);
        bridge.requests = Cbor.encode(List.of("p/A"));
        bridge.classReply = -1000;
        assertEquals(-1000, link.step(library, Set.of()));
        assertEquals(List.of(-3001, -1000), errors);
        assertEquals(2, link.rejected());
        assertEquals(0, link.provided(), "refusée, donc pas comptée");
    }

    @Test
    @DisplayName("Les demandes se décodent ; un blob illisible est une erreur, pas un vide")
    void requestsAreDecoded() {
        assertEquals(List.of(), CallGraphLink.requested(null));
        assertEquals(List.of("a/B", "c/D"),
                CallGraphLink.requested(Cbor.encode(List.of("a/B", "c/D"))));
        assertThrows(IllegalStateException.class,
                () -> CallGraphLink.requested(new byte[] {(byte) 0xff}));
        assertThrows(IllegalStateException.class,
                () -> CallGraphLink.requested(Cbor.encode("a/B")));
    }

    @Test
    @DisplayName("La pause ramène la part du fil à son budget")
    void thePauseKeepsTheBudget() {
        assertEquals(190, CallGraphLink.pauseMillis(10_000_000L, 5));
        assertEquals(10, CallGraphLink.pauseMillis(10_000_000L, 50));
        assertEquals(0, CallGraphLink.pauseMillis(-5L, 5));
    }

    @Test
    @DisplayName("Le fil déclare, avance, puis s'arrête et ferme les archives")
    void theThreadRunsAndStops() throws InterruptedException {
        MemoryLibrary library = new MemoryLibrary().plain("p/A");
        RecordingBridge bridge = new RecordingBridge();
        CallGraphLink link = link(bridge, library, new ArrayList<>());
        link.start();
        long deadline = System.nanoTime() + 5_000_000_000L;
        while (bridge.stepCalls < 2 && System.nanoTime() < deadline) {
            Thread.sleep(5L);
        }
        assertTrue(bridge.stepCalls >= 2, "le fil avance après l'attente");
        assertTrue(link.running());
        assertEquals(List.of(CallGraphLink.THREAD_NAME), bridge.threads);
        link.stop();
        assertFalse(link.running());
        assertTrue(library.closed);
    }
}
