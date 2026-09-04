package dev.rustforgex.bridge;

import java.nio.ByteBuffer;

/**
 * Abstraction du runtime natif, vue depuis le reste du mod.
 *
 * <p>Composant : C-27 (frontière). Interface : IF-01. Maturité : {@code STABLE}.
 *
 * <p>C-02 et C-45 dialoguent avec le natif exclusivement à travers cette interface,
 * jamais avec {@link RfxNative} directement. Cela permet d'injecter un pont simulé
 * dans les tests — notamment pour vérifier le refus d'une ABI incompatible (T-112) ou
 * le passage en mode dégradé quand le natif est absent (T-111), situations qu'on ne
 * peut pas provoquer avec le vrai binaire.
 *
 * <p>Convention de retour, identique à l'ABI (R-701) : {@code 0} pour un succès, une
 * valeur négative pour un code d'erreur de l'annexe A.2.
 */
public interface PontNatif {

    /** @return la version d'ABI du binaire chargé (R-702) */
    int abiVersion();

    /**
     * @param configCbor configuration sérialisée en CBOR
     * @return le handle opaque, positif, ou un code d'erreur négatif
     */
    long init(byte[] configCbor);

    /**
     * @param handle handle du runtime
     * @return {@code 0} ou un code d'erreur négatif
     */
    int shutdown(long handle);

    /**
     * @param handle handle du runtime
     * @return {@code 0} ou un code d'erreur négatif
     */
    int noop(long handle);

    /**
     * @param handle handle du runtime
     * @return {@code 0} ou un code d'erreur négatif
     */
    int hwProbe(long handle);

    /**
     * @param handle handle du runtime
     * @param jniCallNs coût moyen d'un aller-retour FFI, en nanosecondes
     * @param ffiBatchNsPerKb coût de transfert, en nanosecondes par kibioctet
     * @return {@code 0} ou un code d'erreur négatif
     */
    int hwSetFfiCosts(long handle, int jniCallNs, int ffiBatchNsPerKb);

    /**
     * @param handle handle du runtime
     * @param tampon tampon direct à faire lire au natif
     * @param longueur nombre d'octets à lire
     * @return un témoin de lecture, ou un code d'erreur négatif
     */
    long transferProbe(long handle, ByteBuffer tampon, int longueur);

    /**
     * @param handle handle du runtime
     * @return le blob de statut CBOR, ou {@code null} en cas d'erreur
     */
    byte[] status(long handle);

    /**
     * @param handle handle du runtime
     * @return {@code -3001} si le confinement des panics fonctionne
     */
    int panicTest(long handle);

    /**
     * Pont vers le binaire réellement chargé.
     *
     * <p>Ne doit être instancié qu'après le chargement de la bibliothèque par C-03 :
     * tout appel antérieur lèverait une {@link UnsatisfiedLinkError}.
     *
     * @return le pont s'appuyant sur {@link RfxNative}
     */
    static PontNatif reel() {
        return new PontNatif() {

            @Override
            public int abiVersion() {
                return RfxNative.abiVersion();
            }

            @Override
            public long init(byte[] configCbor) {
                return RfxNative.init(configCbor);
            }

            @Override
            public int shutdown(long handle) {
                return RfxNative.shutdown(handle);
            }

            @Override
            public int noop(long handle) {
                return RfxNative.noop(handle);
            }

            @Override
            public int hwProbe(long handle) {
                return RfxNative.hwProbe(handle);
            }

            @Override
            public int hwSetFfiCosts(long handle, int jniCallNs, int ffiBatchNsPerKb) {
                return RfxNative.hwSetFfiCosts(handle, jniCallNs, ffiBatchNsPerKb);
            }

            @Override
            public long transferProbe(long handle, ByteBuffer tampon, int longueur) {
                return RfxNative.transferProbe(handle, tampon, longueur);
            }

            @Override
            public byte[] status(long handle) {
                return RfxNative.status(handle);
            }

            @Override
            public int panicTest(long handle) {
                return RfxNative.panicTest(handle);
            }
        };
    }
}
