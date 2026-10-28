package dev.rustforgex.analysis;

import dev.rustforgex.bridge.Cbor;
import dev.rustforgex.bridge.CborReader;
import dev.rustforgex.bridge.NativeBridge;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.function.IntConsumer;
import java.util.function.Supplier;

/**
 * C-07, côté Java : fournit au natif la hiérarchie de l'installation, puis le bytecode
 * qu'il demande, et fait avancer la construction des graphes d'appel, hors du tick.
 *
 * <p>Cahier des charges : PARTIE 5.7 (R-340, R-341), FM-15, UNKNOWN = CONSERVATIVE.
 * Décision : ADR-051. Maturité : {@code STABLE}.
 *
 * <p>Tout se passe sur le fil {@code rustforgex-analysis}, de priorité minimale, qui
 * s'efface pour ne consommer que {@code analysis.cpu_budget_pct} pour cent d'un cœur :
 * <ol>
 *   <li>déclarer la hiérarchie <strong>complète</strong>, par lots. Le natif analyse
 *       dès qu'il en connaît une classe ; or une hiérarchie partielle cacherait des
 *       sous-classes, et un appel serait déclaré monomorphe à tort. Aucun pas n'est donc
 *       demandé avant le dernier lot, ni jamais si un lot a échoué ;</li>
 *   <li>avancer pas à pas : à chaque demande, lire les classes voulues et les fournir.
 *       Une cible de mixin, d'access transformer ou de coremod est déclarée absente :
 *       son bytecode final nous échappe. Les cibles de mixin sont relevées pendant la
 *       déclaration, dans toutes les classes ({@link MixinTargets}) ; une classe mixin
 *       illisible rend l'ensemble incertain, et aucun pas n'est alors demandé ;</li>
 *   <li>rien à faire : attendre que des unités deviennent chaudes.</li>
 * </ol>
 *
 * <p>Une classe dont l'en-tête est illisible est sautée : la JVM la refuserait aussi
 * ({@code ClassFormatError}), elle ne peut donc pas être une sous-classe chargée. Une
 * archive illisible, elle, interrompt la déclaration : on ne sait pas ce qu'elle
 * contenait.
 *
 * <p>Ne lève jamais : un code d'erreur est compté et transmis ; cinq échecs de suite
 * arrêtent le fil sans toucher au jeu.
 */
public final class CallGraphLink {

    /** Nom du fil : le préfixe {@code rustforgex-} l'exclut du relevé de charge. */
    public static final String THREAD_NAME = "rustforgex-analysis";

    /** Classes par lot de hiérarchie. */
    static final int HIERARCHY_BATCH = 4_096;

    /** Attente quand rien n'est à analyser, en millisecondes. */
    static final long IDLE_MS = 30_000L;

    /** Intervalle minimal entre deux lignes d'avancement, en nanosecondes. */
    private static final long PROGRESS_NANOS = 5L * 60 * 1_000_000_000L;

    /** Pas : rien à faire, des classes manquent, un graphe est terminé. */
    static final int STEP_IDLE = 0;
    static final int STEP_NEEDS_CLASSES = 1;
    static final int STEP_FINISHED = 2;

    private static final int ACC_FINAL = 0x0010;
    private static final int MAX_FAILURES = 5;

    /** Classes qui définissent des classes nommées au journal, au plus. */
    private static final int MAX_NAMED_DEFINERS = 5;

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex");

    private final NativeBridge bridge;
    private final long handle;
    private final Supplier<ClassLibrary> libraries;
    private final int budgetPct;
    private final IntConsumer nativeErrors;
    private final long idleMs;

    private volatile Thread worker;
    private volatile boolean running;
    private volatile boolean hierarchyReady;
    private volatile long declared;
    private volatile long unreadable;
    private volatile long unreadableMixins;
    private volatile long definers;
    private final List<String> definerNames = new ArrayList<>();
    private volatile long hidden;
    private volatile long provided;
    private volatile long absent;
    private volatile long graphs;
    private volatile long rejected;
    private int failures;

    /**
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @param libraries ouvre les archives de l'installation, sur le fil d'analyse
     * @param budgetPct part d'un cœur accordée, en pourcent ({@code 1..50})
     * @param nativeErrors destinataire des codes d'erreur négatifs
     */
    public CallGraphLink(NativeBridge bridge, long handle, Supplier<ClassLibrary> libraries,
            int budgetPct, IntConsumer nativeErrors) {
        this(bridge, handle, libraries, budgetPct, nativeErrors, IDLE_MS);
    }

    CallGraphLink(NativeBridge bridge, long handle, Supplier<ClassLibrary> libraries,
            int budgetPct, IntConsumer nativeErrors, long idleMs) {
        this.bridge = bridge;
        this.handle = handle;
        this.libraries = libraries;
        this.budgetPct = Math.max(1, Math.min(100, budgetPct));
        this.nativeErrors = nativeErrors;
        this.idleMs = idleMs;
    }

    /** Démarre le fil. Sans effet s'il tourne déjà. */
    public synchronized void start() {
        if (running) {
            return;
        }
        running = true;
        Thread thread = new Thread(this::loop, THREAD_NAME);
        thread.setDaemon(true);
        thread.setPriority(Thread.MIN_PRIORITY);
        worker = thread;
        thread.start();
    }

    /**
     * Arrête le fil et attend brièvement sa fin. À appeler avant l'arrêt du natif : le
     * fil ne doit plus l'appeler.
     */
    public synchronized void stop() {
        running = false;
        Thread thread = worker;
        worker = null;
        if (thread == null) {
            return;
        }
        thread.interrupt();
        try {
            thread.join(2_000L);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    private void loop() {
        LOGGER.info("Graphe d'appel : lecture des classes de l'installation en arrière-plan, "
                + "à {} % d'un cœur au plus ; le jeu n'attend pas.", budgetPct);
        try (ClassLibrary library = libraries.get()) {
            Set<String> excluded = new HashSet<>(library.transformedClasses());
            long started = System.nanoTime();
            if (!declareHierarchy(library, excluded)) {
                running = false;
                return;
            }
            LOGGER.info("Graphe d'appel : hiérarchie de {} classes transmise en {} s ({} "
                    + "illisibles sautées, {} cibles de mixin, d'access transformer ou de "
                    + "coremod tenues pour inconnues). Analyse des unités chaudes en "
                    + "arrière-plan.", declared, (System.nanoTime() - started) / 1_000_000_000L,
                    unreadable, hidden);
            long loggedGraphs = 0;
            long loggedAt = 0;
            while (running) {
                long begin = System.nanoTime();
                int state = runStep(library, excluded);
                // Le premier graphe aussitôt, puis une ligne toutes les cinq minutes au plus.
                if (graphs != loggedGraphs
                        && (loggedGraphs == 0 || begin - loggedAt >= PROGRESS_NANOS)) {
                    loggedGraphs = graphs;
                    loggedAt = begin;
                    LOGGER.info("Graphe d'appel : {} graphe(s) construit(s) ; {} classes lues, "
                            + "{} tenues pour inconnues.", graphs, provided, absent);
                }
                if (state == STEP_IDLE) {
                    Thread.sleep(idleMs);
                } else {
                    pace(System.nanoTime() - begin);
                }
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            if (!hierarchyReady) {
                LOGGER.info("Graphe d'appel : arrêt avant la fin de la lecture ({} classes "
                        + "déclarées) ; aucun graphe construit.", declared);
            }
        } catch (IOException | RuntimeException | LinkageError e) {
            running = false;
            LOGGER.warn("Graphe d'appel abandonné ({}). Aucune analyse ; le jeu tourne "
                    + "normalement.", e.toString());
        }
    }

    /** Un pas, compté comme échec s'il est refusé ; cinq échecs de suite arrêtent. */
    private int runStep(ClassLibrary library, Set<String> excluded) {
        int state;
        try {
            state = step(library, excluded);
        } catch (RuntimeException | LinkageError e) {
            state = -1;
            if (failures + 1 >= MAX_FAILURES) {
                LOGGER.debug("Dernier échec du graphe d'appel", e);
            }
        }
        if (state >= 0) {
            failures = 0;
            return state;
        }
        if (++failures >= MAX_FAILURES) {
            running = false;
            LOGGER.warn("Graphe d'appel arrêté après {} échecs de suite ; le jeu tourne "
                    + "normalement.", MAX_FAILURES);
        }
        // Un refus n'est pas « rien à faire » : on réessaie au rythme du budget.
        return STEP_FINISHED;
    }

    /**
     * Déclare toute la hiérarchie, par lots, et relève au passage les cibles de mixin.
     *
     * <p>Visible dans le paquet pour les tests.
     *
     * @param library archives de l'installation
     * @param excluded en entrée, les classes modifiées par un access transformer ou un
     *     coremod : leur drapeau {@code final} n'est pas fiable, il est retiré. En
     *     sortie, complété des cibles de mixin
     * @return {@code true} si toute la hiérarchie a été acceptée par le natif et que
     *     toutes les classes mixin ont été lues
     * @throws IOException si une archive n'a pu être lue
     * @throws InterruptedException si le fil est interrompu
     */
    boolean declareHierarchy(ClassLibrary library, Set<String> excluded)
            throws IOException, InterruptedException {
        HierarchySender sender = new HierarchySender(Set.copyOf(excluded));
        boolean complete = library.forEachClass(sender);
        if (sender.interrupted || Thread.currentThread().isInterrupted()) {
            throw new InterruptedException();
        }
        if (!complete || sender.refused || !sender.flush()) {
            if (sender.refused) {
                LOGGER.warn("Hiérarchie des classes refusée par le natif : aucune analyse ; "
                        + "le jeu tourne normalement.");
            }
            return false;
        }
        excluded.addAll(sender.mixinTargets);
        hidden = excluded.size();
        if (unreadableMixins > 0) {
            LOGGER.warn("{} classe(s) mixin illisible(s) : leurs cibles sont inconnues, "
                    + "aucun graphe d'appel ne sera construit. Le jeu tourne normalement.",
                    unreadableMixins);
            return false;
        }
        if (definers > 0) {
            LOGGER.warn("{} classe(s) du jeu ou des mods peuvent définir des classes à "
                    + "l'exécution (dont {}) : la hiérarchie relevée ne serait pas "
                    + "complète, aucun graphe d'appel ne sera construit (ADR-059). Le jeu "
                    + "tourne normalement.", definers, definerNames);
            return false;
        }
        hierarchyReady = true;
        return true;
    }

    /** Accumule les en-têtes et les transmet par lots, au rythme du budget. */
    private final class HierarchySender implements ClassLibrary.ClassSink {
        private final Set<String> transformed;
        private final Set<String> mixinTargets = new HashSet<>();
        private final List<Object> batch = new ArrayList<>(HIERARCHY_BATCH);
        private long since = System.nanoTime();
        private boolean refused;
        private boolean interrupted;

        HierarchySender(Set<String> transformed) {
            this.transformed = transformed;
        }

        @Override
        public boolean accept(byte[] bytes, boolean gameLayer) {
            if (MixinTargets.collect(bytes, mixinTargets) == MixinTargets.Outcome.UNREADABLE) {
                unreadableMixins++;
            }
            ClassHeader header = ClassHeader.parse(bytes);
            if (header == null) {
                unreadable++;
                return !Thread.currentThread().isInterrupted();
            }
            if (gameLayer && ClassDefiners.mentions(bytes)) {
                definers++;
                if (definerNames.size() < MAX_NAMED_DEFINERS) {
                    definerNames.add(header.name());
                }
            }
            int access = transformed.contains(header.name())
                    ? header.access() & ~ACC_FINAL
                    : header.access();
            batch.add(List.of(header.name(), header.superName(), access));
            if (batch.size() < HIERARCHY_BATCH) {
                return !Thread.currentThread().isInterrupted();
            }
            if (!flush()) {
                return false;
            }
            try {
                pace(System.nanoTime() - since);
            } catch (InterruptedException e) {
                interrupted = true;
                return false;
            }
            since = System.nanoTime();
            return !Thread.currentThread().isInterrupted();
        }

        boolean flush() {
            if (batch.isEmpty()) {
                return true;
            }
            int code = bridge.analysisHierarchy(handle, Cbor.encode(batch));
            if (code < 0) {
                reject(code);
                refused = true;
                return false;
            }
            declared += batch.size();
            batch.clear();
            return true;
        }
    }

    /**
     * Un pas du natif, et les classes qu'il demande.
     *
     * <p>Visible dans le paquet pour les tests.
     *
     * @param library archives de l'installation
     * @param excluded classes à déclarer absentes
     * @return l'état du pas, ou un code d'erreur négatif
     */
    int step(ClassLibrary library, Set<String> excluded) {
        if (!hierarchyReady) {
            return STEP_IDLE;
        }
        int state = bridge.analysisStep(handle);
        if (state < 0) {
            reject(state);
            return state;
        }
        if (state == STEP_FINISHED) {
            graphs++;
        } else if (state == STEP_NEEDS_CLASSES) {
            for (String name : requested(bridge.analysisRequests(handle))) {
                byte[] bytes = excluded.contains(name) ? null : library.read(name);
                int code = bridge.analysisClass(handle, name.getBytes(StandardCharsets.UTF_8),
                        bytes);
                if (code < 0) {
                    reject(code);
                    return code;
                }
                if (bytes == null) {
                    absent++;
                } else {
                    provided++;
                }
            }
        }
        return state;
    }

    /**
     * @param blob tableau CBOR de noms, ou {@code null}
     * @return les noms demandés
     */
    static List<String> requested(byte[] blob) {
        if (blob == null) {
            return List.of();
        }
        Object decoded;
        try {
            decoded = CborReader.decode(blob);
        } catch (CborReader.InvalidCbor e) {
            throw new IllegalStateException("demandes illisibles", e);
        }
        if (!(decoded instanceof List<?> list)) {
            throw new IllegalStateException("demandes : tableau attendu");
        }
        List<String> names = new ArrayList<>(list.size());
        for (Object element : list) {
            if (element instanceof String name) {
                names.add(name);
            }
        }
        return names;
    }

    /**
     * Pause qui ramène la part du fil à son budget.
     *
     * @param workNanos durée du travail qui vient d'être fait
     * @param budgetPct part d'un cœur accordée, en pourcent
     * @return la pause, en millisecondes
     */
    static long pauseMillis(long workNanos, int budgetPct) {
        return Math.max(0L, workNanos) * (100L - budgetPct) / budgetPct / 1_000_000L;
    }

    private void pace(long workNanos) throws InterruptedException {
        long pause = pauseMillis(workNanos, budgetPct);
        Thread.sleep(Math.max(1L, pause));
    }

    private void reject(int code) {
        rejected++;
        nativeErrors.accept(code);
    }

    /** @return {@code true} si le fil tourne */
    public boolean running() {
        return running && worker != null && worker.isAlive();
    }

    /** @return {@code true} une fois toute la hiérarchie acceptée */
    public boolean hierarchyReady() {
        return hierarchyReady;
    }

    /** @return les classes déclarées dans la hiérarchie */
    public long declared() {
        return declared;
    }

    /** @return les classes à l'en-tête illisible, sautées */
    public long unreadable() {
        return unreadable;
    }

    /** @return les classes mixin illisibles ; une seule empêche toute analyse */
    public long unreadableMixins() {
        return unreadableMixins;
    }

    /**
     * @return les classes de la couche du jeu qui peuvent définir des classes à
     *     l'exécution ; une seule empêche toute analyse (ADR-059)
     */
    public long definers() {
        return definers;
    }

    /** @return les cibles de mixin, d'access transformer ou de coremod, déclarées absentes */
    public long hidden() {
        return hidden;
    }

    /** @return les classes fournies au natif */
    public long provided() {
        return provided;
    }

    /** @return les classes déclarées absentes au natif */
    public long absent() {
        return absent;
    }

    /** @return les graphes terminés */
    public long graphs() {
        return graphs;
    }

    /** @return le nombre d'appels refusés par le natif */
    public long rejected() {
        return rejected;
    }
}
