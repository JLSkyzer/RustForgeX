package dev.rustforgex.instrument;

import java.lang.management.ManagementFactory;
import java.lang.management.ThreadInfo;
import java.lang.management.ThreadMXBean;
import java.util.Arrays;
import java.util.function.BooleanSupplier;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * C-05 : producteur d'échantillons de pile.
 *
 * <p>Cahier des charges : PARTIE 5.5, « échantillonnage périodique ». Exigences : R-320
 * (aucune allocation dans le chemin chaud), R-322 (le profiler fonctionne sans aucune
 * sonde — l'échantillonnage est ce qui reste quand les sondes sont jugées trop chères),
 * R-700 (par lot, jamais par élément). Invariant : INV-14. Maturité : {@code STABLE}.
 *
 * <h2>Ce qu'il fait</h2>
 *
 * <p>Un fil de service prend, cent fois par seconde au plus et pendant le tick
 * seulement (ADR-040), la pile du <strong>fil autoritatif</strong> — celui qui exécute
 * le tick — et attribue la période écoulée à la première trame qu'il sait reconnaître.
 * C'est du temps <em>propre</em> approché : la trame la plus haute est celle qui
 * s'exécutait.
 *
 * <p>Il prend aussi, dans la même lecture, la pile des fils qui exécutent du code sondé
 * pour le tick (ADR-041). Certains mods tiquent les dimensions sur leurs propres fils
 * pendant que le fil autoritatif les attend : ne prélever que lui ne voyait de ce
 * travail qu'une attente. Le choix des fils ne repose que sur ce qu'ils exécutent,
 * jamais sur un nom (INV-12). Lire une pile ne mute rien (INV-02).
 *
 * <h2>Pourquoi il n'écrit pas lui-même dans le puits</h2>
 *
 * <p>{@link ProbeSink} attache un tampon <strong>par fil</strong>, et c'est le fil
 * autoritatif qui le vide à la fin du tick. Un échantillonneur qui écrirait dans son
 * propre tampon écrirait dans un tampon que personne ne vide : ses mesures seraient
 * perdues, silencieusement.
 *
 * <p>Il dépose donc ses identifiants dans une file à producteur et consommateur uniques,
 * que le fil autoritatif draine à la clôture du tick, juste avant le vidage. Toute
 * interaction avec le natif reste ainsi sur un seul fil, et les échantillons partent
 * par lot, avec le reste (R-700).
 *
 * <h2>Pourquoi il s'arrête pendant une pause de mesure</h2>
 *
 * <p>La PARTIE 12.4 mesure le coût du runtime en éteignant le profilage vingt ticks. Un
 * échantillonneur qui continuerait pendant la pause rendrait cette mesure fausse : elle
 * attribuerait au jeu un coût qui est le nôtre. Il s'aligne donc sur la table des
 * niveaux — entièrement éteinte, il ne prélève rien. Aucun signal supplémentaire n'a
 * besoin de traverser la frontière : la table dit déjà tout.
 *
 * <h2>Son coût</h2>
 *
 * <p>Lire la pile d'un autre fil n'est pas gratuit : le fil lu est suspendu le temps de
 * la lecture. D'où une cadence réglée sur ce coût (ADR-037), une profondeur bornée dès la
 * lecture, un nombre de fils borné, et surtout : ce coût entre dans la ligne de base
 * de la PARTIE 12.4. S'il fait bouger la mesure, il est trop cher, et cela se verra sans
 * qu'on ait à le supposer.
 *
 * <h2>Ce qu'il découvre en plus de ce qu'il attribue</h2>
 *
 * <p>Une pile porte deux informations, et il n'en exploitait qu'une. La première est
 * l'attribution : à quelle sonde revient cette période. La seconde est la
 * <strong>découverte</strong> : quelle méthode s'exécutait vraiment, sondée ou non.
 *
 * <p>Les deux ne coïncident pas. Quand une méthode non sondée s'exécute au-dessus d'une
 * méthode sondée, la période est attribuée à la sonde du dessous — ce qui en fait un
 * temps <em>inclusif</em>, pas propre — et la méthode réellement chaude reste invisible.
 * C'est ce trou que {@link UnknownFrameIndex} comble, sans dépenser une sonde de plus.
 */
public final class StackSampler {

    /**
     * Fréquence d'échantillonnage maximale, en hertz (PARTIE 5.5). La fréquence
     * effective est réglée par {@link SamplingPacer} sur ce que coûte un prélèvement.
     */
    public static final int SAMPLES_PER_SECOND = 100;

    /**
     * Trames examinées au plus, du sommet vers la base.
     *
     * <p>Une pile de serveur moddé dépasse couramment la centaine de trames. Au-delà de
     * quelques dizaines, on n'est plus dans ce qui s'exécute mais dans ce qui a appelé :
     * la sonde d'entrée le dit déjà, et mieux.
     */
    public static final int MAX_DEPTH = 64;

    /**
     * Capacité de la file d'attente, en échantillons.
     *
     * <p>Cinq échantillons par tick suffisent à cent hertz. Deux cent cinquante-six
     * couvrent cinquante ticks de retard : au-delà, ce n'est plus un retard, c'est un
     * serveur qui ne tourne plus, et perdre des échantillons est alors le moindre mal.
     */
    static final int CAPACITY = 256;

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex");

    private final StackFrameIndex index;
    private final Thread target;

    /**
     * Recensement des méthodes chaudes non sondées, alimenté par le même prélèvement.
     *
     * <p>Détenu ici parce que ce fil en est l'unique producteur : lui donner un autre
     * propriétaire multiplierait les chemins d'écriture sans rien simplifier.
     */
    private final UnknownFrameIndex discovery = new UnknownFrameIndex();

    /**
     * Propriété qui éteint le recensement, {@code true} par défaut.
     *
     * <p>Elle n'existe que pour <strong>mesurer le coût du recensement lui-même</strong>,
     * en comparant deux exécutions dont c'est la seule différence. Ce n'est pas un
     * réglage d'exploitation, et c'est pourquoi elle ne figure pas dans
     * {@code rustforgex.toml} : une option de configuration invite à être réglée, et
     * personne n'a de raison de régler celle-ci.
     *
     * <p>Sans elle, affirmer que consigner les trames ne coûte rien resterait un
     * raisonnement — le fil d'échantillonnage est distinct du fil autoritatif, donc
     * <em>a priori</em> gratuit pour le tick. Un raisonnement de cette forme s'est déjà
     * révélé faux dans ce projet.
     */
    public static final String PROPERTY_DISCOVER = "rustforgex.instrumentation.discover_frames";

    /**
     * Propriété qui empêche l'échantillonnage de démarrer, {@code true} par défaut.
     *
     * <p>Même raison d'être que {@link #PROPERTY_DISCOVER} : mesurer le coût de
     * l'échantillonnage en comparant deux exécutions dont c'est la seule différence. Le
     * 1,1 ms par tick relevé sous `heavy` synthétique (ADR-036) ne s'attribue pas sans
     * elle. Ce n'est pas un réglage d'exploitation.
     */
    public static final String PROPERTY_SAMPLING = "rustforgex.instrumentation.stack_sampling";

    /**
     * @return {@code false} si {@link #PROPERTY_SAMPLING} demande de ne pas échantillonner
     */
    public static boolean samplingAllowed() {
        try {
            return !"false".equalsIgnoreCase(System.getProperty(PROPERTY_SAMPLING));
        } catch (SecurityException e) {
            return true;
        }
    }

    /** Lu une fois : le fil de prélèvement ne doit pas interroger les propriétés. */
    private final boolean discovering = readDiscovering();

    private static boolean readDiscovering() {
        try {
            return !"false".equalsIgnoreCase(System.getProperty(PROPERTY_DISCOVER));
        } catch (SecurityException e) {
            return true;
        }
    }

    /**
     * File à producteur unique et consommateur unique.
     *
     * <p>Le fil d'échantillonnage écrit la case puis publie {@code writeIndex}, qui est
     * volatile ; le fil autoritatif lit {@code writeIndex} puis la case. C'est la
     * publication sûre du modèle mémoire Java, sans verrou et sans allocation.
     */
    private final int[] pending = new int[CAPACITY];

    private volatile long writeIndex;
    private long readIndex;

    private volatile Thread worker;
    private volatile boolean running;

    private volatile long samplesTaken;
    private volatile long samplesQueued;
    private volatile long samplesUnattributed;
    private volatile long samplesDropped;
    private volatile long samplesOutsideTick;
    private volatile long samplesWorkerThreads;
    private long samplesDrained;

    /**
     * Prépare un échantillonneur, sans le démarrer.
     *
     * @param index correspondance trame vers sonde
     * @param target fil autoritatif à observer
     */
    public StackSampler(StackFrameIndex index, Thread target) {
        this(index, target, SamplingPacer.DEFAULT_BUDGET_FRACTION);
    }

    /**
     * Prépare un échantillonneur, sans le démarrer.
     *
     * @param index correspondance trame vers sonde
     * @param target fil autoritatif à observer
     * @param budgetFraction part du temps que l'échantillonnage peut prendre au fil
     *     observé, voir {@link SamplingPacer}
     */
    public StackSampler(StackFrameIndex index, Thread target, double budgetFraction) {
        this(index, target, budgetFraction, () -> true);
    }

    /**
     * Prépare un échantillonneur, sans le démarrer.
     *
     * @param index correspondance trame vers sonde
     * @param target fil autoritatif à observer
     * @param budgetFraction part du temps que l'échantillonnage peut prendre au fil
     *     observé, voir {@link SamplingPacer}
     * @param inGameTick vrai pendant que le jeu exécute son tick (ADR-040) ; lu sans
     *     verrou à chaque prélèvement, il doit donc être peu coûteux
     */
    public StackSampler(StackFrameIndex index, Thread target, double budgetFraction,
            BooleanSupplier inGameTick) {
        this(index, target, budgetFraction, inGameTick, NO_WORKERS);
    }

    /**
     * Prépare un échantillonneur, sans le démarrer.
     *
     * @param index correspondance trame vers sonde
     * @param target fil autoritatif à observer
     * @param budgetFraction part du temps que l'échantillonnage peut prendre au jeu,
     *     voir {@link SamplingPacer}
     * @param inGameTick vrai pendant que le jeu exécute son tick (ADR-040)
     * @param workers fils qui exécutent du code sondé pour le tick (ADR-041)
     */
    public StackSampler(StackFrameIndex index, Thread target, double budgetFraction,
            BooleanSupplier inGameTick, WorkerThreads workers) {
        this.index = index;
        this.target = target;
        this.targetId = target.getId();
        this.pacer = new SamplingPacer(budgetFraction);
        this.inGameTick = inGameTick;
        this.workers = workers;
    }

    /**
     * Source des fils à échantillonner en plus du fil autoritatif (ADR-041).
     *
     * <p>Appelée par le fil d'échantillonnage à chaque prélèvement.
     */
    @FunctionalInterface
    public interface WorkerThreads {

        /**
         * @param out tableau à remplir d'identifiants Java de thread
         * @param from premier indice à écrire
         * @param exclude identifiant à ne pas retenir, celui du fil autoritatif
         * @return le nombre d'identifiants écrits, sans dépasser la fin de {@code out}
         */
        int fill(long[] out, int from, long exclude);
    }

    /** Aucun fil en plus du fil autoritatif. */
    public static final WorkerThreads NO_WORKERS = (out, from, exclude) -> 0;

    /**
     * Fils lus au plus par prélèvement, fil autoritatif compris (ADR-041).
     *
     * <p>Tous sont suspendus le temps d'une même lecture : la borne limite la durée d'une
     * pause du jeu, que la cadence seule n'étale pas. Elle n'est dérivée d'aucune valeur
     * du cahier des charges ; c'est la campagne d'ADR-041 qui la juge.
     */
    public static final int MAX_THREADS_PER_SAMPLE = 16;

    private static final ThreadMXBean THREADS = ManagementFactory.getThreadMXBean();

    private final long targetId;
    private final WorkerThreads workers;

    /** Identifiants à lire, réutilisés d'un prélèvement à l'autre. */
    private final long[] threadIds = new long[MAX_THREADS_PER_SAMPLE];

    /**
     * Vrai pendant que le jeu exécute son tick (ADR-040).
     *
     * <p>Hors du tick, le fil autoritatif attend le suivant : sur le modpack réel sous
     * `medium`, 61 % des prélèvements tombaient dans cette attente. Ils ne disent rien du
     * tick, coûtent quand même, et ralentissent la cadence que le budget autorise.
     */
    private final BooleanSupplier inGameTick;

    /** Cadence réglée sur le coût mesuré des prélèvements (ADR-037). */
    private final SamplingPacer pacer;

    /** Démarre le fil d'échantillonnage. Sans effet s'il tourne déjà. */
    public synchronized void start() {
        if (running) {
            return;
        }
        running = true;
        Thread thread = new Thread(this::loop, "RFX-StackSampler");
        // Démon : un échantillonneur ne doit jamais retarder l'arrêt du serveur.
        thread.setDaemon(true);
        // Sous la priorité normale : mesurer ne passe jamais avant jouer.
        thread.setPriority(Thread.MIN_PRIORITY);
        worker = thread;
        thread.start();
        LOGGER.debug("Échantillonnage de pile démarré sur « {} », {} Hz au plus, cadence "
                + "réglée sur son coût.", target.getName(), SAMPLES_PER_SECOND);
    }

    /** Arrête le fil d'échantillonnage et attend brièvement sa fin. */
    public synchronized void stop() {
        running = false;
        Thread thread = worker;
        worker = null;
        if (thread == null) {
            return;
        }
        thread.interrupt();
        try {
            thread.join(200L);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    /**
     * Verse les échantillons en attente dans le puits du fil autoritatif.
     *
     * <p>À appeler depuis ce fil et depuis lui seul, à la clôture du tick, avant le
     * vidage. N'alloue pas.
     *
     * @param sink puits d'enregistrements du fil autoritatif
     * @return le nombre d'échantillons versés
     */
    public int drainInto(ProbeSink sink) {
        long published = writeIndex;
        int drained = 0;
        while (readIndex < published) {
            int probeId = pending[(int) (readIndex % CAPACITY)];
            readIndex++;
            // La valeur portée est la période que cet échantillon représente : le natif
            // la cumule en `sampled_ns_this_tick`. C'est la période courante : elle varie
            // avec le coût des prélèvements, et une période fixe sur- ou sous-attribuerait.
            sink.record(probeId, ProbeSink.KIND_SAMPLE, (short) 0,
                    System.nanoTime(), pacer.periodNs());
            drained++;
        }
        samplesDrained += drained;
        return drained;
    }

    /** Boucle de prélèvement. Ne lève jamais : un défaut ici n'arrête pas le jeu. */
    private void loop() {
        while (running) {
            try {
                java.util.concurrent.locks.LockSupport.parkNanos(pacer.periodNs());
                if (!running || Thread.currentThread().isInterrupted()) {
                    return;
                }
                sampleOnce();
            } catch (RuntimeException | LinkageError e) {
                // Un échec d'échantillonnage se compte, il n'interrompt rien.
                samplesUnattributed++;
            }
        }
    }

    /**
     * Prélève une pile et met en file la sonde correspondante.
     *
     * <p>Visible dans le paquet pour les tests ; la boucle, elle, ne se teste pas.
     */
    void sampleOnce() {
        if (!RfxProbes.anyProbeArmed() || !target.isAlive()) {
            // Sondes toutes éteintes : pause de mesure de la PARTIE 12.4, ou profiler à
            // l'arrêt. Dans les deux cas, prélever fausserait la mesure.
            return;
        }
        if (!inGameTick.getAsBoolean()) {
            // Entre deux ticks : le fil attend, il n'y a rien à attribuer (ADR-040).
            samplesOutsideTick++;
            return;
        }
        threadIds[0] = targetId;
        int count = 1 + workers.fill(threadIds, 1, targetId);
        long start = System.nanoTime();
        // Une seule lecture pour tous les fils, profondeur bornée : les trames au-delà
        // de MAX_DEPTH ne servent pas, inutile de les faire remonter.
        ThreadInfo[] infos =
                THREADS.getThreadInfo(Arrays.copyOf(threadIds, count), MAX_DEPTH);
        // Les fils observés sont suspendus le temps de la lecture : c'est le prix payé
        // par le jeu, et ce qui règle la cadence.
        pacer.record(System.nanoTime() - start);

        for (int i = 0; i < infos.length; i++) {
            ThreadInfo info = infos[i];
            if (info == null) {
                // Terminé entre la sélection et la lecture.
                continue;
            }
            if (i > 0) {
                samplesWorkerThreads++;
            }
            attribute(info.getStackTrace());
        }
    }

    /** Attribue une pile prélevée à sa sonde, et la met en file. */
    private void attribute(StackTraceElement[] stack) {
        samplesTaken++;
        int probeId = topmostKnownProbe(stack, index, discovering ? discovery : null);
        if (probeId < 0) {
            samplesUnattributed++;
            return;
        }
        enqueue(probeId);
    }

    /** Met un identifiant en file, ou le compte perdu si elle est pleine. */
    private void enqueue(int probeId) {
        long write = writeIndex;
        if (write - readIndex >= CAPACITY) {
            // Le fil autoritatif ne draine plus : le serveur est bloqué, pas nous.
            samplesDropped++;
            return;
        }
        pending[(int) (write % CAPACITY)] = probeId;
        // Publication : la case est écrite avant que l'indice ne la rende visible.
        writeIndex = write + 1;
        samplesQueued++;
    }

    /**
     * Première trame, du sommet vers la base, qui désigne une sonde sans ambiguïté.
     *
     * @param stack pile prélevée
     * @param index correspondance trame vers sonde
     * @return l'identifiant, ou une valeur négative si aucune trame n'est attribuable
     */
    static int topmostKnownProbe(StackTraceElement[] stack, StackFrameIndex index) {
        return topmostKnownProbe(stack, index, null);
    }

    /**
     * Attribue la pile à une sonde, et consigne au passage ce qui n'en a pas.
     *
     * <p>Les deux se font en une seule traversée, et il n'y a pas d'autre choix
     * raisonnable : la découverte doit s'arrêter exactement là où l'attribution
     * s'arrête, sans quoi elle consignerait des appelants au lieu d'appelés.
     *
     * <p>La trame consignée est la plus haute qui soit à la fois <em>candidate au
     * sondage</em> et <em>non sondée</em>. Elle l'est même quand une sonde finit par
     * être trouvée plus bas : c'est précisément le cas intéressant, celui où la période
     * est attribuée à une sonde qui n'exécutait pas ce temps-là.
     *
     * @param stack pile prélevée
     * @param index correspondance trame vers sonde
     * @param discovery recensement à alimenter, ou {@code null} pour ne rien consigner
     * @return l'identifiant, ou une valeur négative si aucune trame n'est attribuable
     */
    static int topmostKnownProbe(StackTraceElement[] stack, StackFrameIndex index,
            UnknownFrameIndex discovery) {
        int depth = Math.min(stack.length, MAX_DEPTH);
        StackTraceElement candidate = null;
        int probeId = StackFrameIndex.NO_PROBE;
        for (int i = 0; i < depth; i++) {
            StackTraceElement frame = stack[i];
            int found = index.probeFor(frame);
            if (found >= 0) {
                probeId = found;
                break;
            }
            // Une trame ambiguë n'est pas franchie : la méthode qui s'exécutait est
            // bien celle-là, et attribuer son temps à son appelant serait faux.
            if (found == StackFrameIndex.AMBIGUOUS) {
                probeId = StackFrameIndex.AMBIGUOUS;
                break;
            }
            if (candidate == null && UnknownFrameIndex.isCandidate(frame)) {
                candidate = frame;
            }
        }
        if (discovery != null && candidate != null) {
            discovery.record(candidate);
        }
        return probeId;
    }

    /** @return {@code true} si le fil d'échantillonnage tourne */
    public boolean running() {
        return running;
    }

    /** @return la période d'échantillonnage courante, en nanosecondes (ADR-037) */
    public long periodNs() {
        return pacer.periodNs();
    }

    /** @return le nombre de piles prélevées */
    public long samplesTaken() {
        return samplesTaken;
    }

    /** @return le nombre d'échantillons mis en file */
    public long samplesQueued() {
        return samplesQueued;
    }

    /** @return le nombre d'échantillons versés au puits */
    public long samplesDrained() {
        return samplesDrained;
    }

    /** @return le nombre d'échantillons qu'aucune trame connue n'a permis d'attribuer */
    public long samplesUnattributed() {
        return samplesUnattributed;
    }

    /** @return le nombre d'échantillons perdus faute de place dans la file */
    public long samplesDropped() {
        return samplesDropped;
    }

    /** @return le nombre de prélèvements évités parce qu'aucun tick ne s'exécutait */
    public long samplesOutsideTick() {
        return samplesOutsideTick;
    }

    /** @return le nombre de piles prélevées sur d'autres fils que le fil autoritatif */
    public long samplesWorkerThreads() {
        return samplesWorkerThreads;
    }

    /** @return le recensement des méthodes chaudes non sondées, jamais {@code null} */
    public UnknownFrameIndex discovery() {
        return discovery;
    }
}
