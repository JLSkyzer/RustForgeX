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
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

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

    /**
     * R-700 : mille appels ne font pas mille traversées.
     *
     * <p>Le niveau {@code COUNTER} écrivait un enregistrement de trente-deux octets par
     * appel pour transporter un « plus un ». C'est exactement ce que R-700 interdit, et
     * c'était le chemin de toutes les sondes tant que le profileur reste à
     * {@code LIGHT}.
     */
    @Test
    @DisplayName("R-700 : mille passages comptés donnent un seul enregistrement")
    void athousandCountedPassesGiveASingleRecord() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);
        sink.announceProbeCapacity(64);

        for (int i = 0; i < 1_000; i++) {
            assertTrue(sink.count(7));
        }
        assertEquals(0, sink.recordsWritten(), "compter ne doit rien écrire avant la vidange");

        sink.flush();

        assertEquals(1, sink.recordsWritten(), "un lot, pas mille enregistrements");
        ByteBuffer buffer = bridge.buffers.get(0);
        assertEquals(7, buffer.getInt(0), "probe_id");
        assertEquals(ProbeSink.KIND_ENTER, buffer.get(4), "kind");
        assertEquals(1_000L, buffer.getLong(16), "les mille passages sont dans la valeur");
    }

    @Test
    @DisplayName("Chaque sonde touchée donne son propre lot, une seule fois")
    void eachTouchedProbeGivesItsOwnBatchOnce() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);
        sink.announceProbeCapacity(64);

        sink.count(3);
        sink.count(9);
        sink.count(3);

        sink.flush();

        assertEquals(2, sink.recordsWritten(), "deux sondes touchées, deux enregistrements");
        ByteBuffer buffer = bridge.buffers.get(0);
        assertEquals(3, buffer.getInt(0));
        assertEquals(2L, buffer.getLong(16), "la sonde 3 a été vue deux fois");
        assertEquals(9, buffer.getInt(ProbeSink.RECORD_SIZE));
        assertEquals(1L, buffer.getLong(ProbeSink.RECORD_SIZE + 16));
    }

    @Test
    @DisplayName("Une seconde vidange ne réémet pas les compteurs déjà reversés")
    void asecondFlushDoesNotResendDrainedCounters() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);
        sink.announceProbeCapacity(64);

        sink.count(1);
        sink.flush();
        sink.flush();

        assertEquals(1, sink.recordsWritten(), "un compteur reversé ne l'est pas deux fois");
    }

    /**
     * Une sonde enregistrée après l'attachement du thread dépasse la capacité de ses
     * tableaux. Agrandir ceux-ci serait une allocation dans le chemin chaud (R-320) :
     * l'enregistrement direct reste correct, simplement plus coûteux.
     */
    @Test
    @DisplayName("Une sonde hors capacité est enregistrée directement, jamais perdue")
    void aprobeBeyondCapacityIsRecordedDirectly() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);
        sink.announceProbeCapacity(4);

        assertTrue(sink.count(99));

        assertEquals(1, sink.recordsWritten(), "le passage doit être écrit tout de suite");
        ByteBuffer buffer = bridge.buffers.get(0);
        assertEquals(99, buffer.getInt(0));
        assertEquals(1L, buffer.getLong(16));
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

    /**
     * R-709 et R-700 ensemble : un tampon plein se vide sur place, mais pas
     * indéfiniment.
     *
     * <p>Vider plutôt qu'écarter rend le nombre de traversées proportionnel au volume
     * (ADR-029). C'est ce qu'il faut, et c'est aussi ce qui pourrait faire dériver le
     * budget de frontière : au-delà de {@link ProbeSink#MAX_FLUSHES_PER_EPOCH} vidages
     * dans le même tick, on retombe donc sur l'écart des enregistrements, et le natif
     * en est informé par la longueur annoncée.
     */
    @Test
    @DisplayName("R-700 : passé son budget de vidages, un tampon plein écarte et le signale")
    void pastItsFlushBudgetAFullBufferDropsAndReportsRecords() {
        FakeBridge bridge = new FakeBridge();
        bridge.bufferBytes = 2 * ProbeSink.RECORD_SIZE;
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        // Deux enregistrements par tampon, deux vidages accordés : six passent.
        int fits = 2 * (ProbeSink.MAX_FLUSHES_PER_EPOCH + 1);
        for (int i = 0; i < fits; i++) {
            assertTrue(sink.record(i, ProbeSink.KIND_ENTER, (short) 0, i, i),
                    "enregistrement " + i);
        }
        assertEquals(ProbeSink.MAX_FLUSHES_PER_EPOCH, bridge.flushes.size());

        // Le budget est épuisé : les suivants sont écartés, et comptés.
        assertFalse(sink.record(50, ProbeSink.KIND_ENTER, (short) 0, 50, 50),
                "budget de traversées épuisé");
        assertFalse(sink.record(51, ProbeSink.KIND_ENTER, (short) 0, 51, 51));

        assertEquals(fits, sink.recordsWritten());
        assertEquals(2, sink.recordsDropped());
        assertEquals(2, sink.flushBudgetExceeded());

        // La clôture annonce les deux écrits qui restent plus les deux écartés, pour
        // que le natif les compte perdus.
        sink.flush();
        int[] last = bridge.flushes.get(bridge.flushes.size() - 1);
        assertEquals(4 * ProbeSink.RECORD_SIZE, last[1]);
    }

    @Test
    @DisplayName("La clôture du tick rend son budget de vidages au thread")
    void thetickCloseGivesTheThreadItsFlushBudgetBack() {
        FakeBridge bridge = new FakeBridge();
        bridge.bufferBytes = ProbeSink.RECORD_SIZE;
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        for (int i = 0; i <= ProbeSink.MAX_FLUSHES_PER_EPOCH; i++) {
            assertTrue(sink.record(i, ProbeSink.KIND_ENTER, (short) 0, i, i));
        }
        assertFalse(sink.record(9, ProbeSink.KIND_ENTER, (short) 0, 9, 9), "budget épuisé");

        sink.flush();

        assertTrue(sink.record(3, ProbeSink.KIND_ENTER, (short) 0, 3, 3), "budget rendu");
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

    /**
     * Un thread de travail unique, réutilisé d'une tâche à l'autre.
     *
     * <p>Deux {@link Thread} distincts ne partagent pas la variable de thread du puits :
     * leur donner le même nom donnerait deux états séparés, et le test vérifierait
     * l'inverse de ce qu'il croit. C'est ce qu'un exécuteur à un seul thread garantit.
     */
    private static final class Worker implements AutoCloseable {

        private final ExecutorService executor;

        Worker(String name) {
            this.executor = Executors.newSingleThreadExecutor(task -> new Thread(task, name));
        }

        void run(Runnable task) {
            try {
                executor.submit(task).get();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new AssertionError(e);
            } catch (ExecutionException e) {
                throw new AssertionError(e.getCause());
            }
        }

        @Override
        public void close() {
            executor.shutdownNow();
        }
    }

    /**
     * Le recensement doit rendre visible un thread qui retient des passages.
     *
     * <p>Ici le thread de travail n'exécute rien après le changement de tick : rien ne
     * peut donc venir reverser ses compteurs, et c'est la seule perte que le mécanisme
     * d'époque ne rattrape pas — au plus un tick de travail d'un thread qui s'est tu.
     */
    @Test
    @DisplayName("Un thread qui se tait garde ses derniers passages, et cela se voit")
    void athreadThatFallsSilentKeepsItsLastPassesAndItShows() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);
        sink.announceProbeCapacity(16);

        try (Worker worker = new Worker("faux-thread-de-mod")) {
            worker.run(() -> {
                sink.count(4);
                sink.count(4);
                sink.count(5);
            });
        }
        sink.flush();

        ProbeSink.ThreadUsage orphan = usageOf(sink, "faux-thread-de-mod");
        assertEquals(3L, orphan.pendingPasses(),
                "il ne repassera plus : personne ne peut reverser à sa place");
    }

    /**
     * ADR-029, le défaut que ce mécanisme corrige : le vidage n'avait lieu qu'à la
     * clôture du tick, donc sur le seul fil autoritatif, et ne concernait que
     * <em>son</em> tampon. Sur une campagne de 8 900 ticks, dix-sept threads sur
     * dix-huit n'ont jamais rien remis — 152 millions de passages comptés pour rien.
     *
     * <p>Un thread sondé vide désormais le sien dès qu'il constate que le tick a changé.
     */
    @Test
    @DisplayName("ADR-029 : un thread de travail reverse ses passages au tick suivant")
    void aworkerThreadDrainsItsPassesOnTheNextTick() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);
        sink.announceProbeCapacity(16);

        try (Worker worker = new Worker("thread-de-dimension")) {
            // Tick 1 : le thread de travail compte trois passages sur deux sondes.
            worker.run(() -> {
                sink.count(4);
                sink.count(4);
                sink.count(5);
            });

            // Clôture du tick par le fil autoritatif : elle publie la nouvelle époque.
            sink.flush();
            assertTrue(bridge.flushes.isEmpty(),
                    "le fil autoritatif n'avait rien à remettre");

            // Tick 2 : au premier appel sondé, le thread de travail constate le
            // changement et reverse ce qu'il retenait.
            worker.run(() -> sink.count(4));

            assertEquals(1, bridge.flushes.size(), "le thread de travail a traversé");
            ProbeSink.ThreadUsage usage = usageOf(sink, "thread-de-dimension");
            assertTrue(usage.flushes() > 0, "il vide désormais lui-même");
            assertEquals(1L, usage.pendingPasses(),
                    "il ne retient plus que le passage du tick en cours");
        }
    }

    /**
     * Un thread reversant lui-même, le tampon d'un thread n'est plus vidé par la
     * clôture d'un tick qu'il n'exécute pas. Il ne doit donc pas déborder en silence :
     * un tampon plein se vide sur place.
     */
    @Test
    @DisplayName("Un tampon plein se vide au lieu de perdre la suite")
    void afullBufferFlushesInsteadOfLosingWhatFollows() {
        FakeBridge bridge = new FakeBridge();
        bridge.bufferBytes = ProbeSink.RECORD_SIZE * 4;
        ProbeSink sink = new ProbeSink(bridge, HANDLE);

        try (Worker worker = new Worker("thread-de-dimension")) {
            worker.run(() -> {
                for (int i = 0; i < 12; i++) {
                    assertTrue(sink.record(i, ProbeSink.KIND_EXIT, (short) 0, 0L, 1L),
                            "aucun enregistrement ne doit être perdu : " + i);
                }
            });
        }

        assertEquals(0L, sink.recordsDropped());
        assertEquals(ProbeSink.MAX_FLUSHES_PER_EPOCH, bridge.flushes.size(),
                "les tampons pleins sont remis en cours de tick");
    }

    /**
     * Un thread attaché avant l'arrivée de nouvelles sondes gardait ses tableaux à vie :
     * tout identifiant au-delà retombait sur l'écriture directe, trente-deux octets par
     * appel, et saturait le tampon. C'est de là que venaient onze millions
     * d'enregistrements perdus par thread de dimension.
     */
    @Test
    @DisplayName("Les tableaux de comptage rattrapent les sondes apparues depuis")
    void thecountingArraysCatchUpWithProbesAddedSince() {
        FakeBridge bridge = new FakeBridge();
        ProbeSink sink = new ProbeSink(bridge, HANDLE);
        sink.announceProbeCapacity(2);

        try (Worker worker = new Worker("thread-de-dimension")) {
            // Attaché quand deux sondes seulement existaient.
            worker.run(() -> sink.count(0));

            sink.announceProbeCapacity(64);
            sink.flush();

            // Le tick a changé : la clôture d'époque rattrape la taille, et
            // l'identifiant 50 se compte au lieu de s'écrire un par un.
            worker.run(() -> {
                sink.count(50);
                sink.count(50);
            });

            assertEquals(2L, usageOf(sink, "thread-de-dimension").pendingPasses(),
                    "les deux passages sont comptés, pas écrits un par un");
        }
    }

    private static ProbeSink.ThreadUsage usageOf(ProbeSink sink, String name) {
        return sink.threads().stream()
                .filter(usage -> name.equals(usage.name()))
                .findFirst()
                .orElseThrow();
    }
}
