package dev.rustforgex.instrument;

import dev.rustforgex.bridge.NativeBridge;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;

/**
 * IF-03, côté Java : écriture des enregistrements de sonde.
 *
 * <p>Composant : C-04 (production), C-05 (consommation). Cahier des charges :
 * PARTIE 6.4 et PARTIE 5.5. Exigences : R-320 (aucune allocation dans le chemin
 * chaud), R-708 (le tampon appartient au natif), R-709 (un flush n'alloue ni ne
 * bloque). Maturité : {@code STABLE}.
 *
 * <p>Chaque thread possède son propre tampon, obtenu du natif au premier usage et
 * conservé dans une variable de thread. Écrire un enregistrement revient donc à poser
 * trente-deux octets à la position courante : pas de verrou, pas d'allocation, pas de
 * partage entre threads. Le tampon est vidé une fois par tick, ce qui tient le budget
 * de moins de cinquante traversées de frontière par tick (R-700).
 *
 * <p>Quand le tampon est plein, les enregistrements suivants sont <strong>comptés puis
 * jetés</strong> jusqu'au prochain vidage. Perdre des mesures est sans conséquence —
 * elles sont statistiques — alors que bloquer le thread du jeu pour les conserver
 * toutes en aurait beaucoup.
 */
public final class ProbeSink {

    /** Taille d'un enregistrement (IF-03), en octets. */
    public static final int RECORD_SIZE = 32;

    /** Décalages des champs dans un enregistrement (PARTIE 6.4). */
    private static final int OFFSET_PROBE_ID = 0;

    private static final int OFFSET_KIND = 4;
    private static final int OFFSET_FLAGS = 5;
    private static final int OFFSET_CONTEXT = 6;
    private static final int OFFSET_TIMESTAMP = 8;
    private static final int OFFSET_VALUE = 16;

    /** Entrée dans une méthode sondée. */
    public static final byte KIND_ENTER = 0;

    /** Sortie d'une méthode sondée. */
    public static final byte KIND_EXIT = 1;

    /** Allocation observée. */
    public static final byte KIND_ALLOC = 2;

    /** Événement Forge. */
    public static final byte KIND_EVENT = 3;

    /** Échantillon de pile. */
    public static final byte KIND_SAMPLE = 4;

    /** État de sondage d'un thread. */
    private static final class ThreadState {

        final int threadId;
        final ByteBuffer buffer;
        int position;
        long dropped;

        ThreadState(int threadId, ByteBuffer buffer) {
            this.threadId = threadId;
            this.buffer = buffer;
        }
    }

    /**
     * Tampon du thread courant.
     *
     * <p>Une variable de thread plutôt qu'une table concurrente : la lecture doit
     * coûter le strict minimum, elle a lieu à chaque appel sondé.
     */
    private final ThreadLocal<ThreadState> state = new ThreadLocal<>();

    private final NativeBridge bridge;
    private final long handle;

    /**
     * Identifiants de thread, attribués dans l'ordre de première sollicitation.
     *
     * <p>Propre à cette instance, et non partagé par la classe : les identifiants
     * n'ont de sens que pour le runtime natif auquel ce puits est rattaché. Un
     * compteur de classe lierait entre eux des puits qui n'ont rien à voir.
     */
    private final AtomicInteger nextThreadId = new AtomicInteger();
    private final AtomicLong recordsWritten = new AtomicLong();
    private final AtomicLong recordsDropped = new AtomicLong();
    private final AtomicLong flushes = new AtomicLong();
    private final AtomicLong refusedThreads = new AtomicLong();

    /**
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     */
    public ProbeSink(NativeBridge bridge, long handle) {
        this.bridge = bridge;
        this.handle = handle;
    }

    /**
     * Écrit un enregistrement pour le thread courant.
     *
     * <p>Ne lève jamais d'exception et n'alloue jamais : cette méthode est appelée
     * depuis le code du jeu, potentiellement des milliers de fois par tick.
     *
     * @param probeId identifiant local de la sonde
     * @param kind nature de l'enregistrement
     * @param contextHash hachage court du contexte d'appel
     * @param timestampNs horodatage monotone, en nanosecondes
     * @param value durée, taille d'allocation, ou autre valeur selon la nature
     * @return {@code true} si l'enregistrement a été écrit
     */
    public boolean record(int probeId, byte kind, short contextHash, long timestampNs, long value) {
        ThreadState current = state.get();
        if (current == null) {
            current = attach();
            if (current == null) {
                return false;
            }
        }

        ByteBuffer buffer = current.buffer;
        int at = current.position;
        if (at + RECORD_SIZE > buffer.capacity()) {
            // Tampon plein : on compte et on laisse tomber, jusqu'au prochain vidage.
            current.dropped++;
            recordsDropped.incrementAndGet();
            return false;
        }

        buffer.putInt(at + OFFSET_PROBE_ID, probeId);
        buffer.put(at + OFFSET_KIND, kind);
        buffer.put(at + OFFSET_FLAGS, (byte) 0);
        buffer.putShort(at + OFFSET_CONTEXT, contextHash);
        buffer.putLong(at + OFFSET_TIMESTAMP, timestampNs);
        buffer.putLong(at + OFFSET_VALUE, value);

        current.position = at + RECORD_SIZE;
        recordsWritten.incrementAndGet();
        return true;
    }

    /**
     * Vide le tampon du thread courant vers le natif.
     *
     * <p>Appelée une fois par tick, en fin de tick. Un thread qui n'a rien écrit ne
     * traverse pas la frontière.
     *
     * @return le nombre d'octets remis au natif
     */
    public int flush() {
        ThreadState current = state.get();
        if (current == null || current.position == 0) {
            return 0;
        }
        int used = current.position;
        // Les enregistrements écartés faute de place sont signalés au natif en
        // gonflant la longueur annoncée : c'est ainsi qu'il les compte perdus (R-709).
        int announced = used + (int) Math.min(current.dropped * RECORD_SIZE, Integer.MAX_VALUE - used);

        bridge.probeBufferFlush(handle, current.threadId, announced);
        flushes.incrementAndGet();

        current.position = 0;
        current.dropped = 0;
        return used;
    }

    /**
     * Attache le thread courant à un tampon natif.
     *
     * @return l'état du thread, ou {@code null} s'il ne peut pas être sondé
     */
    private ThreadState attach() {
        int threadId = nextThreadId.getAndIncrement();
        ByteBuffer buffer = bridge.probeBufferAcquire(handle, threadId);
        if (buffer == null) {
            // Budget mémoire atteint, ou trop de threads déjà suivis : ce thread ne
            // sera pas sondé. La mesure est moins complète, le jeu n'est pas affecté.
            refusedThreads.incrementAndGet();
            return null;
        }
        // Le format est petit-boutiste (PARTIE 6.4) ; l'ordre natif de la JVM ne l'est
        // pas nécessairement, il faut donc l'imposer.
        buffer.order(ByteOrder.LITTLE_ENDIAN);

        ThreadState created = new ThreadState(threadId, buffer);
        state.set(created);
        return created;
    }

    /** @return le nombre d'enregistrements écrits depuis le démarrage */
    public long recordsWritten() {
        return recordsWritten.get();
    }

    /** @return le nombre d'enregistrements écartés faute de place (R-709) */
    public long recordsDropped() {
        return recordsDropped.get();
    }

    /** @return le nombre de vidages effectués */
    public long flushes() {
        return flushes.get();
    }

    /** @return le nombre de threads que le natif a refusé de doter d'un tampon */
    public long refusedThreads() {
        return refusedThreads.get();
    }

    /**
     * Détache le thread courant.
     *
     * <p>Réservé aux tests : en production, un thread garde son tampon jusqu'à l'arrêt
     * du runtime.
     */
    void detachCurrentThread() {
        state.remove();
    }
}
