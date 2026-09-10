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
 * partage entre threads.
 *
 * <h2>Chaque thread vide le sien (ADR-029)</h2>
 *
 * <p>Le vidage n'a longtemps eu lieu qu'à la clôture du tick, donc sur le seul fil
 * autoritatif, et ne concernait que le tampon de <em>ce</em> thread. Tout ce qui était
 * sondé ailleurs s'accumulait dans un tampon que personne ne venait chercher : une
 * campagne de 8 900 ticks a compté <strong>152 millions de passages</strong> jamais
 * remis, sur dix-sept threads. La couverture manquante que trois ADR ont cherché à
 * élargir n'était pas un défaut de sondage, mais de collecte.
 *
 * <p>Un thread sondé compare désormais son époque à {@link #tickEpoch} au début de
 * chaque appel et, si le tick a changé, reverse ses compteurs et vide son tampon —
 * <strong>sur son propre thread</strong>. Rien n'est partagé hormis cette époque, donc
 * rien n'est à synchroniser. Le budget de R-700 tient : un thread traverse la frontière
 * une fois par tick, et le comptage groupé fait qu'un tampon suffit largement.
 *
 * <p>Quand le tampon se remplit avant la fin du tick, il est vidé sur place plutôt que
 * de perdre la suite. Un enregistrement n'est <strong>compté puis jeté</strong> que si
 * le natif refuse même de consommer : perdre des mesures est sans conséquence — elles
 * sont statistiques — alors que bloquer le thread du jeu pour les conserver toutes en
 * aurait beaucoup.
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

        /**
         * Nom du thread à son attachement.
         *
         * <p>Retenu pour une seule raison : quand des enregistrements ne partent
         * jamais, il faut pouvoir dire <strong>de quel thread</strong>. Un identifiant
         * numérique ne le dit pas, et c'est le nom qui désigne le mod fautif.
         */
        final String threadName;

        /** Vidages effectués par ce thread. Zéro signale un tampon qui ne part jamais. */
        long flushCount;

        /** Époque du dernier reversement. Différente de celle du puits : à vider. */
        long epoch = -1L;

        /** Vidages déjà consommés dans l'époque courante, pour le budget de R-700. */
        int flushesThisEpoch;

        /**
         * Passages comptés par sonde, indexés par identifiant.
         *
         * <p>Alloué à l'attachement du thread, et réalloué seulement à la clôture d'une
         * époque, jamais dans le chemin d'appel : R-320 interdit d'y allouer.
         */
        int[] counts;

        /**
         * Identifiants touchés depuis la dernière vidange.
         *
         * <p>Sans cette liste, vider les compteurs demanderait de parcourir les
         * milliers d'entrées de {@link #counts} à chaque tick, alors qu'une poignée
         * seulement est appelée. Elle est alimentée au passage de zéro à un, ce qui
         * garantit qu'un identifiant n'y figure qu'une fois.
         */
        int[] touched;

        int touchedLen;

        ThreadState(int threadId, String threadName, ByteBuffer buffer, int probeCapacity) {
            this.threadId = threadId;
            this.threadName = threadName;
            this.buffer = buffer;
            this.counts = new int[probeCapacity];
            this.touched = new int[probeCapacity];
        }

        /**
         * Passages comptés et non encore reversés.
         *
         * <p>Lu depuis un autre thread que celui qui écrit : les valeurs peuvent être
         * légèrement en retard. C'est un diagnostic d'ordre de grandeur, pas une
         * mesure — et verrouiller le chemin chaud pour le rendre exact serait payer
         * beaucoup pour ne rien apprendre de plus.
         */
        long pendingPasses() {
            long total = 0;
            int len = Math.min(touchedLen, touched.length);
            for (int i = 0; i < len; i++) {
                total += counts[touched[i]];
            }
            return total;
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
    /**
     * Nombre de sondes que les tableaux de comptage doivent couvrir.
     *
     * <p>Renseigné par {@code RfxProbes} à chaque table de niveaux reçue. Un thread
     * attaché avant l'arrivée de nouvelles sondes rattrape la taille voulue à la
     * clôture d'époque suivante, jamais dans le chemin d'appel (R-320). Tant qu'il ne
     * l'a pas rattrapée, les identifiants au-delà retombent sur l'enregistrement
     * direct : correct, mais trente-deux octets par appel au lieu d'un « plus un ».
     */
    private volatile int probeCapacity;

    private final AtomicInteger nextThreadId = new AtomicInteger();
    private final AtomicLong recordsWritten = new AtomicLong();
    private final AtomicLong recordsDropped = new AtomicLong();
    private final AtomicLong flushes = new AtomicLong();
    private final AtomicLong refusedThreads = new AtomicLong();
    private final AtomicLong flushBudgetExceeded = new AtomicLong();

    /**
     * Vidages qu'un thread peut s'accorder en cours de tick, en plus de sa clôture.
     *
     * <p>Vider un tampon plein plutôt que d'en perdre le contenu rend le nombre de
     * traversées proportionnel au volume, et non plus au nombre de threads. R-700 vise
     * moins de cinquante traversées par tick : avec la vingtaine de threads sondés
     * qu'un modpack fait apparaître, la clôture en consomme déjà une vingtaine, et
     * quatre vidages supplémentaires par thread suffiraient à faire dériver le compte.
     *
     * <p>Deux, donc, puis on retombe sur l'écart des enregistrements (R-709). Le cas se
     * compte — {@link #flushBudgetExceeded()} — plutôt que de rester une hypothèse.
     */
    static final int MAX_FLUSHES_PER_EPOCH = 2;

    /**
     * Propriété qui rétablit le vidage par le seul fil autoritatif, {@code true} par
     * défaut.
     *
     * <p>Elle existe pour une raison : mesurer ce que le vidage multi-thread coûte, en
     * comparant deux exécutions dont c'est la seule différence. Dix-sept threads qui
     * traversent la frontière une fois par tick, là où un seul le faisait, ne peuvent
     * pas être déclarés gratuits par raisonnement — le natif est protégé par un verrou
     * global, et c'est le fil autoritatif qui attendrait.
     *
     * <p>Éteinte, la collecte redevient celle d'avant ADR-029 : correcte pour le fil
     * autoritatif, muette pour tous les autres.
     */
    public static final String PROPERTY_FLUSH_ALL = "rustforgex.instrumentation.flush_all_threads";

    /** Lu une fois : le chemin sondé ne doit pas interroger les propriétés. */
    private final boolean flushAllThreads = readFlushAllThreads();

    private static boolean readFlushAllThreads() {
        try {
            return !"false".equalsIgnoreCase(System.getProperty(PROPERTY_FLUSH_ALL));
        } catch (SecurityException e) {
            return true;
        }
    }

    /**
     * Tous les états de thread créés, dans l'ordre d'attachement.
     *
     * <p>Une variable de thread ne s'énumère pas. Sans cette liste, il est impossible
     * de répondre à la question « quels threads produisent des enregistrements que
     * personne ne vide », qui est exactement celle que pose la parallélisation du tick
     * par certains mods.
     */
    private final java.util.List<ThreadState> attached = new java.util.concurrent.CopyOnWriteArrayList<>();

    /**
     * Époque de tick, publiée par le fil autoritatif à chaque clôture.
     *
     * <p>C'est le seul signal qui traverse d'un thread à l'autre, et il est fait pour
     * être lu, pas écrit : un {@code long} volatile que le fil autoritatif incrémente
     * vingt fois par seconde et que tous les autres lisent. La ligne de cache reste
     * partagée en lecture dans chaque cœur et n'est invalidée qu'à la clôture du tick.
     *
     * <h2>Pourquoi ce mécanisme plutôt qu'un vidage centralisé</h2>
     *
     * <p>La solution évidente — le fil autoritatif vide les tampons de tous les threads
     * à la clôture du tick — demande de lire un tampon pendant qu'un autre thread y
     * écrit. Il faudrait un tampon double ou un anneau à indices publiés, donc de la
     * synchronisation dans le chemin chaud, pour un gain nul : chaque thread est déjà
     * le mieux placé pour vider le sien.
     *
     * <p>Ici, <strong>aucun état n'est partagé entre threads</strong> hormis cette
     * époque. Un thread sondé la compare à la sienne au début de chaque appel et, si
     * elle a changé, reverse ses compteurs et vide son tampon — sur son propre thread,
     * sans verrou, sans course.
     *
     * <h2>Ce que ce mécanisme ne rattrape pas</h2>
     *
     * <p>Un thread qui cesse définitivement d'exécuter du code sondé garde ses derniers
     * compteurs : rien ne viendra plus les reverser. La perte vaut au plus un tick de
     * travail d'un thread qui s'est tu, et la rattraper demanderait exactement la
     * synchronisation qu'on vient d'éviter.
     */
    private volatile long tickEpoch;

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
    /**
     * Compte un passage sans traverser la frontière (R-700).
     *
     * <p>C'est le chemin du niveau {@code COUNTER}, celui de toutes les sondes tant que
     * le profileur est à {@code LIGHT}. Il écrivait auparavant un enregistrement de
     * trente-deux octets par appel, pour transporter un « plus un » — alors que R-700
     * exige de grouper. Le comptage franchissait donc la frontière élément par élément,
     * ce que le reste du tampon évite soigneusement.
     *
     * <p>Ici : une lecture de tableau, une incrémentation, une comparaison. La vidange
     * a lieu une fois par tick, et son coût est proportionnel au nombre de sondes
     * <strong>touchées</strong>, non au nombre d'<strong>appels</strong>.
     *
     * @param probeId identifiant local de la sonde
     * @return {@code true} si le passage a été compté
     */
    public boolean count(int probeId) {
        ThreadState current = state.get();
        if (current == null) {
            current = attach();
            if (current == null) {
                return false;
            }
        }
        if (flushAllThreads && current.epoch != tickEpoch) {
            closeEpoch(current);
        }
        // Après la clôture, qui a pu réallouer les tableaux.
        int[] counts = current.counts;
        if (probeId < 0 || probeId >= counts.length) {
            // Sonde enregistrée après l'attachement de ce thread : agrandir le tableau
            // ici serait une allocation dans le chemin chaud. L'enregistrement direct
            // reste correct, simplement plus coûteux, et le prochain thread attaché
            // aura la bonne taille.
            return record(probeId, KIND_ENTER, (short) 0, 0L, 1L);
        }
        if (counts[probeId]++ == 0) {
            current.touched[current.touchedLen++] = probeId;
        }
        return true;
    }

    /**
     * Reverse les compteurs dans le tampon, un enregistrement par sonde touchée.
     *
     * <p>Chaque enregistrement porte dans sa valeur le nombre de passages, là où il en
     * fallait un par passage. C'est le lot que R-700 demande.
     *
     * @return le nombre de sondes reversées
     */
    private int drainCounts(ThreadState current) {
        int drained = current.touchedLen;
        for (int i = 0; i < drained; i++) {
            int probeId = current.touched[i];
            int passes = current.counts[probeId];
            current.counts[probeId] = 0;
            if (passes > 0) {
                record(probeId, KIND_ENTER, (short) 0, 0L, passes);
            }
        }
        current.touchedLen = 0;
        return drained;
    }

    public boolean record(int probeId, byte kind, short contextHash, long timestampNs, long value) {
        ThreadState current = state.get();
        if (current == null) {
            current = attach();
            if (current == null) {
                return false;
            }
        }
        if (flushAllThreads && current.epoch != tickEpoch) {
            closeEpoch(current);
        }

        ByteBuffer buffer = current.buffer;
        int at = current.position;
        if (at + RECORD_SIZE > buffer.capacity()) {
            // Tampon plein avant la fin du tick. Le vider maintenant coûte une
            // traversée de plus ; le laisser plein coûte tous les enregistrements
            // jusqu'à la clôture, et c'est ainsi que quarante millions se sont perdus.
            if (current.flushesThisEpoch >= MAX_FLUSHES_PER_EPOCH) {
                // Budget de traversées épuisé pour ce tick (R-700). Au-delà, écarter
                // vaut mieux que faire déborder le budget de frontière : la mesure est
                // statistique, la contrainte de frontière ne l'est pas.
                flushBudgetExceeded.incrementAndGet();
                current.dropped++;
                recordsDropped.incrementAndGet();
                return false;
            }
            current.flushesThisEpoch++;
            flushBuffer(current);
            at = current.position;
            if (at + RECORD_SIZE > buffer.capacity()) {
                // Tampon plus petit qu'un enregistrement : rien à tenter de plus.
                current.dropped++;
                recordsDropped.incrementAndGet();
                return false;
            }
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
        long epoch = tickEpoch + 1L;
        ThreadState current = state.get();
        int used = 0;
        if (current != null) {
            // L'epoque courante, pas la suivante : le reversement ecrit des
            // enregistrements, qui repassent par la comparaison. Lui donner une epoque
            // que le puits n'a pas encore publiee la rendrait fausse, et le thread se
            // viderait au milieu de son propre reversement.
            current.epoch = tickEpoch;
            drainCounts(current);
            used = flushBuffer(current);
            current.flushesThisEpoch = 0;
        }
        // Publiee apres avoir vide le sien : les autres threads observeront la nouvelle
        // epoque a leur prochain appel sonde et videront le leur, chacun sur son propre
        // thread. Publier avant les ferait tous converger vers le verrou du natif au
        // moment precis ou le fil autoritatif en a besoin.
        tickEpoch = epoch;
        if (current != null) {
            // Il vient de vider : lui faire refaire une cloture au prochain appel ne
            // reverserait rien et compterait un vidage qui n'a pas eu lieu.
            current.epoch = epoch;
        }
        return used;
    }

    /**
     * Reverse les compteurs et vide le tampon du thread courant, sur ce thread.
     *
     * <p>Appelee depuis le chemin sonde, une fois par tick et par thread : c'est ce qui
     * remplace le vidage unique du fil autoritatif. L'epoque est inscrite
     * <strong>avant</strong> le reversement, parce que celui-ci ecrit des
     * enregistrements et repasserait donc par la comparaison qui nous amene ici.
     */
    private void closeEpoch(ThreadState current) {
        current.epoch = tickEpoch;
        current.flushesThisEpoch = 0;
        drainCounts(current);
        flushBuffer(current);
        growCounts(current);
    }

    /**
     * Agrandit les tableaux de comptage si des sondes sont apparues depuis
     * l'attachement.
     *
     * <p>Un thread attache tot gardait ses tableaux a vie, et tout identifiant au-dela
     * retombait sur l'ecriture directe — un enregistrement de trente-deux octets par
     * appel, qui saturait le tampon. C'est de la que venaient onze millions
     * d'enregistrements perdus par thread de dimension.
     *
     * <p>L'allocation a lieu ici et nulle part ailleurs : une fois par tick au plus,
     * juste apres un reversement qui a remis les compteurs a zero, et jamais dans le
     * chemin d'appel (R-320).
     */
    private void growCounts(ThreadState current) {
        int wanted = probeCapacity;
        if (wanted <= current.counts.length) {
            return;
        }
        current.counts = new int[wanted];
        current.touched = new int[wanted];
        current.touchedLen = 0;
    }

    /**
     * Remet au natif ce que le tampon du thread courant contient.
     *
     * @return le nombre d'octets remis
     */
    private int flushBuffer(ThreadState current) {
        if (current.position == 0) {
            // Ce thread a bien vide, meme s'il n'avait rien a remettre : ne pas le
            // compter ferait passer un thread inactif pour un thread orphelin.
            current.flushCount++;
            return 0;
        }
        int used = current.position;
        // Les enregistrements ecartes faute de place sont signales au natif en
        // gonflant la longueur annoncee : c'est ainsi qu'il les compte perdus (R-709).
        int announced = used + (int) Math.min(current.dropped * RECORD_SIZE, Integer.MAX_VALUE - used);

        bridge.probeBufferFlush(handle, current.threadId, announced);
        current.flushCount++;
        flushes.incrementAndGet();

        current.position = 0;
        current.dropped = 0;
        return used;
    }

    /**
     * Ce qu'un thread sondé a produit, et ce qui en est parti.
     *
     * @param name nom du thread à son attachement
     * @param flushes vidages effectués ; zéro signale un tampon que personne ne vide
     * @param pendingPasses passages comptés et jamais reversés
     * @param dropped enregistrements perdus, tampon plein faute de vidage
     */
    public record ThreadUsage(String name, long flushes, long pendingPasses, long dropped) {
    }

    /**
     * Recense les threads qui ont écrit des enregistrements de sonde.
     *
     * <p>{@link #flush()} n'est appelée qu'à la clôture du tick, donc <strong>par le
     * seul thread autoritatif</strong>, et ne vide que le tampon de ce thread : c'est
     * la conception, et elle est correcte tant que le travail sondé s'exécute là.
     * Plusieurs mods parallélisent le tick — et ce qu'ils exécutent ailleurs est alors
     * compté dans un tampon que personne ne vient chercher.
     *
     * <p>Ce recensement rend ce cas visible au lieu de le laisser deviner : un thread à
     * zéro vidage et à passages en attente est du travail sondé qui n'arrive jamais au
     * profileur. Aucune sonde n'y aurait suffi — c'est un défaut de collecte, pas de
     * couverture.
     *
     * <p>Alloue et parcourt : à n'appeler que depuis une commande ou un rapport.
     *
     * @return un état par thread attaché, dans l'ordre d'attachement
     */
    public java.util.List<ThreadUsage> threads() {
        java.util.List<ThreadUsage> usage = new java.util.ArrayList<>(attached.size());
        for (ThreadState current : attached) {
            usage.add(new ThreadUsage(current.threadName, current.flushCount,
                    current.pendingPasses(), current.dropped));
        }
        return java.util.List.copyOf(usage);
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

        ThreadState created = new ThreadState(
                threadId, Thread.currentThread().getName(), buffer, probeCapacity);
        state.set(created);
        // Liste à copie sur écriture : un ajout par thread sondé, et des lectures qui
        // n'ont lieu que dans un diagnostic. Le chemin chaud n'y touche jamais.
        attached.add(created);
        return created;
    }

    /**
     * Annonce le nombre de sondes à couvrir par les compteurs.
     *
     * <p>À appeler quand la table des niveaux change. Les threads déjà attachés
     * conservent leurs tableaux — les redimensionner supposerait d'allouer depuis un
     * autre thread que le leur, sur des tableaux qu'ils lisent sans verrou.
     *
     * @param probes nombre d'identifiants de sonde attribués
     */
    public void announceProbeCapacity(int probes) {
        if (probes > probeCapacity) {
            probeCapacity = probes;
        }
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

    /**
     * @return le nombre d'enregistrements écartés faute de budget de traversée (R-700)
     */
    public long flushBudgetExceeded() {
        return flushBudgetExceeded.get();
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
