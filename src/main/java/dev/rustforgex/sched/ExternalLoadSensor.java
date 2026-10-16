package dev.rustforgex.sched;

import java.lang.management.ManagementFactory;
import java.lang.management.ThreadMXBean;
import java.util.Arrays;

/**
 * C-17, côté Java : capteur de charge externe pour l'ordonnanceur.
 *
 * <p>Cahier des charges : PARTIE 5.17.2, R-421 (détecter les pools de fils tiers par le
 * nombre de fils exécutables du processus), RISK-05. Décision : ADR-046. Maturité :
 * {@code STABLE}.
 *
 * <h2>Pourquoi pas l'état {@code RUNNABLE}</h2>
 *
 * <p>Pour la JVM, un fil bloqué dans un appel natif est {@code RUNNABLE}. Sur le modpack
 * d'essai, une photographie des fils (ADR-046) en a montré 53 dans cet état, dont environ
 * 46 attendaient en réalité une entrée-sortie — files de complétion, surveillance de
 * fichiers, boucles réseau. Compter les états aurait fait croire la machine saturée en
 * permanence, et l'ordonnanceur se serait réduit à un seul worker.
 *
 * <h2>Ce qu'il compte</h2>
 *
 * <p>Les fils tiers qui ont réellement consommé du processeur : au moins
 * {@link #BUSY_SHARE} d'un cœur entre deux relevés, d'après leur temps CPU. Un fil en
 * attente consomme presque rien et disparaît du compte ; en vraie sur-souscription,
 * trente-deux fils occupés sur seize cœurs reçoivent chacun la moitié d'un cœur, et sont
 * tous comptés. C'est bien un nombre de fils exécutables, au sens du cahier des charges.
 *
 * <p>Les fils de RUSTFORGE-X sont exclus, et les workers de l'ordonnanceur, fils natifs,
 * ne sont pas vus par la JVM : c'est la charge des <em>autres</em> qui est mesurée. Les
 * fils internes de la JVM (compilation, ramasse-miettes) n'apparaissent pas non plus dans
 * l'énumération.
 *
 * <h2>Son coût</h2>
 *
 * <p>Une énumération des fils, puis une lecture de temps CPU par fil, sans arrêt de la
 * JVM. Aucune allocation en régime établi : le tableau d'énumération et les tables de
 * temps ne grandissent que s'ils débordent. Il tourne sur son propre fil
 * ({@link ExternalLoadReporter}), jamais dans le tick.
 */
public final class ExternalLoadSensor {

    /** Part d'un cœur au-delà de laquelle un fil compte comme exécutable. */
    static final double BUSY_SHARE = 0.10;

    /** Capacité initiale du tableau d'énumération. */
    static final int INITIAL_CAPACITY = 256;

    /**
     * Agrandissements tentés au plus dans un même relevé. Au-delà, le compte est un
     * minorant ; le relevé suivant repart du tableau agrandi.
     */
    private static final int MAX_GROWTH = 4;

    /** Case vide des tables de temps : les identifiants de fil sont positifs. */
    private static final long EMPTY = -1L;

    private final ThreadGroup root;
    private final ThreadMXBean threadBean;
    private Thread[] threads;

    /** Temps CPU par fil au relevé précédent, puis au relevé courant : adressage ouvert. */
    private long[] previousIds = emptyTable(16);
    private long[] previousCpu = new long[16];
    private long[] currentIds = emptyTable(16);
    private long[] currentCpu = new long[16];
    private long previousWallNs = -1L;

    /** Capteur sur tous les fils de la JVM. */
    public ExternalLoadSensor() {
        this(rootGroup(), INITIAL_CAPACITY, ManagementFactory.getThreadMXBean());
    }

    /**
     * @param root groupe dont les fils, sous-groupes compris, sont comptés
     * @param initialCapacity capacité initiale du tableau d'énumération
     * @param threadBean source des temps CPU par fil
     */
    ExternalLoadSensor(ThreadGroup root, int initialCapacity, ThreadMXBean threadBean) {
        this.root = root;
        this.threads = new Thread[Math.max(1, initialCapacity)];
        this.threadBean = threadBean;
    }

    private static ThreadGroup rootGroup() {
        ThreadGroup group = Thread.currentThread().getThreadGroup();
        while (group.getParent() != null) {
            group = group.getParent();
        }
        return group;
    }

    private static long[] emptyTable(int size) {
        long[] table = new long[size];
        Arrays.fill(table, EMPTY);
        return table;
    }

    /**
     * @return {@code true} si la JVM mesure le temps CPU par fil. Sinon, aucun relevé
     *     n'est possible : la charge reste « non mesurée », jamais nulle (R-660).
     */
    public boolean supported() {
        return threadBean.isThreadCpuTimeSupported() && threadBean.isThreadCpuTimeEnabled();
    }

    /**
     * Compte les fils tiers qui ont consommé au moins {@link #BUSY_SHARE} d'un cœur depuis
     * le relevé précédent.
     *
     * @return ce nombre, ou {@code -1} au premier relevé, qui ne sert que de référence
     */
    public int sample() {
        long now = System.nanoTime();
        int count = root.enumerate(threads, true);
        for (int growth = 0; count == threads.length && growth < MAX_GROWTH; growth++) {
            threads = new Thread[threads.length * 2];
            count = root.enumerate(threads, true);
        }
        int needed = Integer.highestOneBit(Math.max(16, count * 2) - 1) << 1;
        if (currentIds.length < needed) {
            currentIds = new long[needed];
            currentCpu = new long[needed];
        }
        Arrays.fill(currentIds, EMPTY);

        long interval = previousWallNs < 0 ? -1L : now - previousWallNs;
        long busyCpu = (long) (BUSY_SHARE * interval);
        int busy = 0;
        for (int i = 0; i < count; i++) {
            Thread thread = threads[i];
            threads[i] = null;
            if (isOwn(thread)) {
                continue;
            }
            long id = thread.getId();
            long cpu = threadBean.getThreadCpuTime(id);
            if (cpu < 0) {
                // Fil terminé entre l'énumération et la lecture.
                continue;
            }
            put(currentIds, currentCpu, id, cpu);
            if (interval > 0) {
                long before = get(previousIds, previousCpu, id);
                if (before >= 0 && cpu - before >= busyCpu) {
                    busy++;
                }
            }
        }
        long[] ids = previousIds;
        long[] cpus = previousCpu;
        previousIds = currentIds;
        previousCpu = currentCpu;
        currentIds = ids;
        currentCpu = cpus;
        previousWallNs = now;
        return interval > 0 ? busy : -1;
    }

    private static int slot(long id, int length) {
        long mixed = id * 0x9E37_79B9_7F4A_7C15L;
        return (int) (mixed >>> 32) & (length - 1);
    }

    private static void put(long[] ids, long[] values, long id, long value) {
        int at = slot(id, ids.length);
        while (ids[at] != EMPTY && ids[at] != id) {
            at = (at + 1) & (ids.length - 1);
        }
        ids[at] = id;
        values[at] = value;
    }

    /** @return la valeur de {@code id}, ou {@code -1} s'il n'y est pas */
    private static long get(long[] ids, long[] values, long id) {
        int at = slot(id, ids.length);
        while (ids[at] != EMPTY) {
            if (ids[at] == id) {
                return values[at];
            }
            at = (at + 1) & (ids.length - 1);
        }
        return -1L;
    }

    /**
     * Un fil de RUSTFORGE-X : l'échantillonneur de piles, le relevé de charge, l'écriture
     * des rapports.
     *
     * <p>Le nom est le seul repère possible, et il est sûr : ces noms sont donnés ici même,
     * jamais par un mod tiers. Aucun nom de mod n'entre dans ce choix (INV-12).
     */
    static boolean isOwn(Thread thread) {
        String name = thread.getName();
        return name.startsWith("RFX-") || name.startsWith("rustforgex-");
    }
}
