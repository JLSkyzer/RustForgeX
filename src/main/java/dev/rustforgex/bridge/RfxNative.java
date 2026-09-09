package dev.rustforgex.bridge;

import java.nio.ByteBuffer;

/**
 * IF-01 : déclaration Java des points d'entrée du runtime natif.
 *
 * <p>Composant : C-27 (frontière). Décision : ADR-004 (JNI classique,
 * {@code DirectByteBuffer} comme mode de transfert), ADR-015 (points d'entrée du
 * jalon M0). Maturité : {@code STABLE}.
 *
 * <p><strong>Aucune méthode de cette classe ne doit être appelée avant que C-03 n'ait
 * chargé la bibliothèque</strong> : la JVM lèverait une {@link UnsatisfiedLinkError}.
 * L'ordre d'appel est fixé par C-02 : charger, puis {@link #abiVersion()}, puis
 * {@link #init(byte[])} (R-702).
 *
 * <p>Convention de retour (R-701) : {@code 0} en cas de succès, une valeur négative
 * porte un code d'erreur de l'annexe A.2. {@link #init(byte[])} fait exception et
 * renvoie le handle, strictement positif.
 */
public final class RfxNative {

    /**
     * Version d'ABI attendue par ce code Java.
     *
     * <p>Un binaire natif annonçant une autre version est refusé : aucun autre appel
     * n'est émis et le runtime passe en {@code DISABLED} avec {@code E-1002} (R-702,
     * R-703).
     */
    public static final int EXPECTED_ABI = 1;

    private RfxNative() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Version de l'ABI implémentée par le binaire chargé.
     *
     * @return la version, strictement positive
     */
    public static native int abiVersion();

    /**
     * Initialise l'instance unique du runtime natif.
     *
     * @param configCbor configuration validée, sérialisée en CBOR
     * @return le handle opaque (positif), ou un code d'erreur négatif
     */
    public static native long init(byte[] configCbor);

    /**
     * Détruit l'instance du runtime. Le handle devient définitivement invalide.
     *
     * @param handle handle obtenu par {@link #init(byte[])}
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int shutdown(long handle);

    /**
     * Appel sans effet, servant de cible à la calibration du coût FFI (C-45).
     *
     * @param handle handle du runtime
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int noop(long handle);

    /**
     * Déclenche ou rejoue la sonde matérielle native (C-45, R-661).
     *
     * @param handle handle du runtime
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int hwProbe(long handle);

    /**
     * Publie dans la classe matérielle les coûts mesurés depuis Java (C-45, R-660).
     *
     * @param handle handle du runtime
     * @param jniCallNs coût moyen d'un aller-retour FFI, en nanosecondes
     * @param ffiBatchNsPerKb coût de transfert, en nanosecondes par kibioctet
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int hwSetFfiCosts(long handle, int jniCallNs, int ffiBatchNsPerKb);

    /**
     * Fait lire au natif un tampon direct, pour mesurer le débit Java vers natif.
     *
     * @param handle handle du runtime
     * @param buffer tampon <strong>direct</strong> ({@link ByteBuffer#allocateDirect})
     * @param length nombre d'octets à lire, au plus la capacité du tampon
     * @return un témoin de lecture, ou un code d'erreur négatif
     */
    public static native long transferProbe(long handle, ByteBuffer buffer, int length);

    /**
     * Statut du runtime natif, sérialisé en CBOR.
     *
     * @param handle handle du runtime
     * @return le blob de statut, ou {@code null} en cas d'erreur
     */
    public static native byte[] status(long handle);

    /**
     * Classement des unités de travail les plus coûteuses (C-35).
     *
     * <p>Le classement ne porte aucun nom de classe ni de méthode : le natif ne les
     * retient pas. Il rend des identifiants de sonde, que le mod sait résoudre puisque
     * c'est lui qui les a déclarés.
     *
     * @param handle handle du runtime
     * @param limit nombre maximal d'entrées rendues
     * @return le blob CBOR, ou {@code null} en cas d'échec
     */
    public static native byte[] profilerTop(long handle, int limit);

    /**
     * Ouvre une session de diagnostic à la demande de l'opérateur (C-35).
     *
     * @param handle handle du runtime
     * @param levelCode profondeur demandée, selon le codage des niveaux de sonde
     * @param ticks durée de la session, en ticks
     * @return {@code 0} en cas de succès, un code de l'annexe A.2 sinon
     */
    public static native int profilerRequestDepth(long handle, int levelCode, int ticks);

    /**
     * Ouvre la fenêtre de tick (IF-02).
     *
     * <p>Toujours appelée depuis le thread autoritatif. Si le tick précédent n'a pas
     * été fermé — un autre mod ayant interrompu le tick —, il l'est implicitement et
     * l'anomalie est comptée (R-706).
     *
     * @param handle handle du runtime
     * @param tick numéro du tick
     * @param side {@code 0} client, {@code 1} serveur, {@code 2} commun
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int tickBegin(long handle, long tick, int side);

    /**
     * Déclare une transition de phase (IF-02).
     *
     * <p>Une transition qui ne suit pas SM-04 est comptée et refusée, sans erreur.
     *
     * @param handle handle du runtime
     * @param phase {@code 0} PRE, {@code 1} VANILLA, {@code 2} DRAIN, {@code 3} POST
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int tickPhase(long handle, int phase);

    /**
     * Ferme la fenêtre de tick (IF-02).
     *
     * @param handle handle du runtime
     * @return les drapeaux du tick écoulé (positifs), ou un code d'erreur négatif
     */
    public static native long tickEnd(long handle);

    /**
     * Acquiert le tampon de profilage d'un thread (IF-03).
     *
     * <p>Le tampon appartient au natif : Java y écrit et ne le libère
     * <strong>jamais</strong> (R-708). Il reste valide tant que le handle vit.
     *
     * @param handle handle du runtime
     * @param threadId identifiant du thread, stable pour la durée de la partie
     * @return un tampon direct, ou {@code null} si ce thread ne peut pas être sondé
     */
    public static native ByteBuffer probeBufferAcquire(long handle, int threadId);

    /**
     * Consomme les enregistrements écrits par un thread (IF-03).
     *
     * @param handle handle du runtime
     * @param threadId identifiant du thread
     * @param used nombre d'octets écrits depuis le début du tampon
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int probeBufferFlush(long handle, int threadId, int used);

    /**
     * Vérifie le confinement des panics : provoque une panic dans le natif.
     *
     * <p>Sert la commande {@code /rfx panic-test}, disponible uniquement en mode
     * {@code debug} (PARTIE 5.36). Un succès serait un échec : cette méthode renvoie
     * toujours une erreur, {@code E-3001} si la panic a bien été confinée.
     *
     * @param handle handle du runtime
     * @return {@code -3001} si le confinement fonctionne
     */
    public static native int panicTest(long handle);

    /**
     * Enregistre une unité de travail et rend son identifiant de sonde (C-05, DM-01).
     *
     * <p>Le {@code WorkId} est calculé côté natif, jamais ici : deux implémentations
     * d'un même hachage finiraient par diverger sur un détail d'encodage.
     *
     * @param handle handle du runtime
     * @param descriptorCbor {@code WorkDescriptor} sérialisé en CBOR
     * @return l'identifiant de sonde, positif ou nul ; {@code -1} si l'unité ne sera
     *     pas sondée ; un code d'erreur de l'annexe A.2 sinon, donc inférieur ou égal
     *     à {@code -1000}. Les trois domaines sont disjoints : un identifiant de sonde
     *     est borné par {@code profiler.max_workloads}, très en deçà de mille.
     */
    public static native int workloadRegister(long handle, byte[] descriptorCbor);

    /**
     * Récupère la table des niveaux de sonde si elle a changé (ADR-016).
     *
     * @param handle handle du runtime
     * @return le niveau de chaque sonde, indexé par identifiant, ou {@code null} si
     *     la table n'a pas changé depuis le dernier appel
     */
    public static native byte[] probeLevels(long handle);

    /**
     * Démarre le profilage (C-05).
     *
     * @param handle handle du runtime
     * @return {@code 0} ou un code d'erreur négatif
     */
    public static native int profilerStart(long handle);
}
