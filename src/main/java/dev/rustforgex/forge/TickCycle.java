package dev.rustforgex.forge;

import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.instrument.ProbeSink;
import dev.rustforgex.instrument.RfxProbes;
import dev.rustforgex.instrument.StackFrameIndex;
import dev.rustforgex.instrument.StackSampler;

import java.util.concurrent.atomic.AtomicLong;

/**
 * C-01, IF-02 : pilotage de la fenêtre de tick côté Java.
 *
 * <p>Cahier des charges : PARTIE 3.5 (cycle de vie d'un tick instrumenté), PARTIE 6.3.
 * Maturité : {@code STABLE}.
 *
 * <p>Forge n'expose que deux points d'accroche par tick, {@code PRE} et {@code POST}.
 * Les quatre phases de SM-04 s'y répartissent ainsi :
 *
 * <pre>{@code
 * TickEvent PRE  (priorité HIGHEST) : tickBegin ──▶ PRE ──▶ VANILLA
 *                                     le tick du jeu et des mods se déroule
 * TickEvent POST (priorité LOWEST)  : DRAIN ──▶ POST ──▶ tickEnd
 * }</pre>
 *
 * <p>La transition vers {@code VANILLA} est émise à la fin du hook {@code PRE} : c'est
 * précisément l'instant où le tick du jeu commence. Sans elle, la machine à états
 * refuserait le passage direct de {@code PRE} à {@code DRAIN}, et à juste titre — un
 * commit ne doit jamais avoir lieu avant que le jeu ait tiqué.
 *
 * <p>Le vidage des tampons de sondes a lieu pendant la phase de drain : c'est le
 * moment où les mesures du tick sont complètes et où le natif peut les agréger. Une
 * seule traversée de frontière par tick et par thread (R-700).
 *
 * <p>Les snapshots, la soumission de tâches et le commit viendront s'insérer dans la
 * même fenêtre aux jalons qui les implémentent ; à ce stade, le tick est mesuré, rien
 * n'est transformé.
 *
 * <p>Aucune méthode ne lève d'exception : un code d'erreur du natif est compté et
 * ignoré. Interrompre un tick de Minecraft parce qu'un compteur a refusé une
 * transition serait hors de proportion.
 */
public final class TickCycle {

    /** Codes de phase transmis à travers la frontière (IF-02). */
    private static final int PHASE_PRE = 0;

    private static final int PHASE_VANILLA = 1;
    private static final int PHASE_DRAIN = 2;
    private static final int PHASE_POST = 3;

    /** Codes de côté transmis à travers la frontière. */
    public static final int SIDE_CLIENT = 0;

    /** Côté serveur. */
    public static final int SIDE_SERVER = 1;

    /** Côté commun. */
    public static final int SIDE_COMMON = 2;

    private final NativeBridge bridge;
    private final long handle;
    private final int side;
    private final ProbeSink probeSink;
    private final AtomicLong tick = new AtomicLong();
    private final AtomicLong levelUpdates = new AtomicLong();

    private final AtomicLong rejectedCalls = new AtomicLong();
    private volatile boolean windowOpen;

    /**
     * Correspondance trame vers sonde, ou {@code null} si l'échantillonnage est inactif.
     */
    private volatile StackFrameIndex frameIndex;

    /** Échantillonneur, créé au premier tick — le seul moment où le fil est connu. */
    private volatile StackSampler sampler;

    /**
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @param side côté d'exécution, parmi {@link #SIDE_CLIENT}, {@link #SIDE_SERVER}
     *     et {@link #SIDE_COMMON}
     */
    public TickCycle(NativeBridge bridge, long handle, int side) {
        this(bridge, handle, side, null);
    }

    /**
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @param side côté d'exécution
     * @param probeSink puits de sondes à vider en fin de tick, ou {@code null} si le
     *     profilage n'est pas actif
     */
    public TickCycle(NativeBridge bridge, long handle, int side, ProbeSink probeSink) {
        this.bridge = bridge;
        this.handle = handle;
        this.side = side;
        this.probeSink = probeSink;
    }

    /**
     * Ouvre la fenêtre de tick et déclare la phase du jeu.
     *
     * <p>Appelée depuis {@code TickEvent.PRE}, en priorité la plus haute, de sorte que
     * la fenêtre encadre le travail de tous les autres mods.
     */
    public void onTickPre() {
        startSamplingIfRequested();
        long current = tick.incrementAndGet();
        if (!ok(bridge.tickBegin(handle, current, side))) {
            return;
        }
        windowOpen = true;
        // La phase PRE est déjà posée par tickBegin ; on la redéclare pour que la
        // séquence soit explicite dans les journaux et les traces.
        ok(bridge.tickPhase(handle, PHASE_PRE));
        ok(bridge.tickPhase(handle, PHASE_VANILLA));
    }

    /**
     * Ferme la fenêtre de tick.
     *
     * <p>Appelée depuis {@code TickEvent.POST}, en priorité la plus basse, de sorte
     * que RUSTFORGE-X se retire après tous les autres mods.
     *
     * @return les drapeaux du tick écoulé, ou {@code 0} si la fenêtre n'était pas
     *     ouverte
     */
    public long onTickPost() {
        if (!windowOpen) {
            // Un POST sans PRE : un autre mod a annulé l'événement, ou le tick a été
            // interrompu. Le natif fermera implicitement la fenêtre au prochain PRE
            // (R-706) ; il n'y a rien à faire ici.
            return 0;
        }
        ok(bridge.tickPhase(handle, PHASE_DRAIN));
        if (probeSink != null && sampler != null) {
            // Les échantillons prélevés par le fil de service partent avec le reste,
            // depuis ce fil-ci : le puits est attaché par fil (R-700).
            sampler.drainInto(probeSink);
        }
        if (probeSink != null) {
            // Les mesures du tick sont complètes : c'est le moment de les remettre au
            // natif, en une seule traversée.
            probeSink.flush();
        }
        ok(bridge.tickPhase(handle, PHASE_POST));

        long flags = bridge.tickEnd(handle);
        windowOpen = false;
        refreshProbeLevels();
        if (flags < 0) {
            rejectedCalls.incrementAndGet();
            return 0;
        }
        return flags;
    }

    /** @return le numéro du dernier tick ouvert */
    public long currentTick() {
        return tick.get();
    }

    /** @return {@code true} si une fenêtre de tick est ouverte */
    public boolean windowOpen() {
        return windowOpen;
    }

    /**
     * @return le nombre d'appels au runtime natif ayant renvoyé une erreur
     *     ({@code rfx.tick.rejected_calls})
     */
    public long rejectedCalls() {
        return rejectedCalls.get();
    }

    /** Compte un code d'erreur et indique si l'appel a réussi. */
    private boolean ok(int code) {
        if (code < 0) {
            rejectedCalls.incrementAndGet();
            return false;
        }
        return true;
    }

    /**
     * Applique la table des niveaux de sonde si le profiler l'a modifiée (ADR-016).
     *
     * <p>Appelée une fois par tick, après la fermeture de la fenêtre. Le natif ne rend
     * la table que lorsqu'elle a changé : le cas courant, de très loin, est un
     * {@code null} qui ne coûte qu'un aller-retour.
     *
     * <p>Changer un niveau ne demande aucune retransformation de classe — c'est tout
     * l'intérêt d'ADR-016 : une écriture dans un tableau, et les sondes déjà posées
     * changent de profondeur au prochain appel.
     */
    /**
     * Arme l'échantillonnage de pile.
     *
     * <p>Le fil autoritatif n'est pas connu à la construction : il ne l'est qu'au
     * premier tick. L'échantillonneur est donc créé là, et pas avant.
     *
     * @param index correspondance trame vers sonde, jamais {@code null}
     */
    public void enableSampling(StackFrameIndex index) {
        this.frameIndex = index;
    }

    /** Arrête l'échantillonnage, sans échouer s'il n'a jamais démarré. */
    public void stopSampling() {
        StackSampler current = sampler;
        if (current != null) {
            current.stop();
        }
    }

    /** @return l'échantillonneur, ou {@code null} s'il n'a pas démarré */
    public StackSampler sampler() {
        return sampler;
    }

    /** Crée l'échantillonneur au premier tick, sur le fil qui exécute ce tick. */
    private void startSamplingIfRequested() {
        if (sampler != null || frameIndex == null || probeSink == null) {
            return;
        }
        StackSampler created = new StackSampler(frameIndex, Thread.currentThread());
        sampler = created;
        created.start();
    }

    private void refreshProbeLevels() {
        byte[] levels = bridge.probeLevels(handle);
        if (levels == null) {
            return;
        }
        RfxProbes.setLevels(levels);
        levelUpdates.incrementAndGet();
    }

    /** @return le nombre de fois que la table des niveaux a été appliquée */
    public long levelUpdates() {
        return levelUpdates.get();
    }
}
