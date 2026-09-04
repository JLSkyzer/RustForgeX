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
    public static final int ABI_ATTENDUE = 1;

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
     * @param tampon tampon <strong>direct</strong> ({@link ByteBuffer#allocateDirect})
     * @param longueur nombre d'octets à lire, au plus la capacité du tampon
     * @return un témoin de lecture, ou un code d'erreur négatif
     */
    public static native long transferProbe(long handle, ByteBuffer tampon, int longueur);

    /**
     * Statut du runtime natif, sérialisé en CBOR.
     *
     * @param handle handle du runtime
     * @return le blob de statut, ou {@code null} en cas d'erreur
     */
    public static native byte[] status(long handle);

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
}
