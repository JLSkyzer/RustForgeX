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
public interface NativeBridge {

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
     * @param buffer tampon direct à faire lire au natif
     * @param length nombre d'octets à lire
     * @return un témoin de lecture positif, ou un code d'erreur négatif
     */
    long transferProbe(long handle, ByteBuffer buffer, int length);

    /**
     * @param handle handle du runtime
     * @return le blob de statut CBOR, ou {@code null} en cas d'erreur
     */
    byte[] status(long handle);

    /**
     * Classement des unités de travail les plus coûteuses (C-35, {@code /rfx top}).
     *
     * @param handle handle du runtime
     * @param limit nombre maximal d'entrées
     * @return le blob CBOR, ou {@code null} si le classement est indisponible
     */
    byte[] profilerTop(long handle, int limit);

    /**
     * Ouvre une session de diagnostic à la demande de l'opérateur (C-35).
     *
     * @param handle handle du runtime
     * @param levelCode profondeur demandée
     * @param ticks durée de la session, en ticks
     * @return {@code 0} en cas de succès
     */
    int profilerRequestDepth(long handle, int levelCode, int ticks);

    /**
     * @param handle handle du runtime
     * @return {@code -3001} si le confinement des panics fonctionne
     */
    int panicTest(long handle);

    /**
     * Ouvre la fenêtre de tick (IF-02).
     *
     * @param handle handle du runtime
     * @param tick numéro du tick
     * @param side {@code 0} client, {@code 1} serveur, {@code 2} commun
     * @return {@code 0} ou un code d'erreur négatif
     */
    int tickBegin(long handle, long tick, int side);

    /**
     * Déclare une transition de phase (IF-02).
     *
     * @param handle handle du runtime
     * @param phase {@code 0} PRE, {@code 1} VANILLA, {@code 2} DRAIN, {@code 3} POST
     * @return {@code 0} ou un code d'erreur négatif
     */
    int tickPhase(long handle, int phase);

    /**
     * Ferme la fenêtre de tick (IF-02).
     *
     * @param handle handle du runtime
     * @return les drapeaux du tick écoulé, ou un code d'erreur négatif
     */
    long tickEnd(long handle);

    /**
     * Acquiert le tampon de profilage d'un thread (IF-03).
     *
     * @param handle handle du runtime
     * @param threadId identifiant du thread
     * @return un tampon direct appartenant au natif, ou {@code null}
     */
    ByteBuffer probeBufferAcquire(long handle, int threadId);

    /**
     * Consomme les enregistrements écrits par un thread (IF-03).
     *
     * @param handle handle du runtime
     * @param threadId identifiant du thread
     * @param used nombre d'octets écrits
     * @return {@code 0} ou un code d'erreur négatif
     */
    int probeBufferFlush(long handle, int threadId, int used);

    /**
     * Enregistre une unité de travail et rend son identifiant de sonde (C-05, DM-01).
     *
     * @param handle handle du runtime
     * @param descriptorCbor {@code WorkDescriptor} sérialisé en CBOR
     * @return l'identifiant de sonde, {@code -1} si l'unité ne sera pas sondée, ou un
     *     code d'erreur de l'annexe A.2, inférieur ou égal à {@code -1000}
     */
    int workloadRegister(long handle, byte[] descriptorCbor);

    /**
     * Récupère la table des niveaux de sonde si elle a changé (ADR-016).
     *
     * @param handle handle du runtime
     * @return le niveau de chaque sonde, ou {@code null} si rien n'a changé
     */
    byte[] probeLevels(long handle);

    /**
     * Démarre le profilage (C-05).
     *
     * @param handle handle du runtime
     * @return {@code 0} ou un code d'erreur négatif
     */
    int profilerStart(long handle);

    /**
     * Pont vers le binaire réellement chargé.
     *
     * <p>Ne doit être instancié qu'après le chargement de la bibliothèque par C-03 :
     * tout appel antérieur lèverait une {@link UnsatisfiedLinkError}.
     *
     * @return le pont s'appuyant sur {@link RfxNative}
     */
    static NativeBridge real() {
        return new NativeBridge() {

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
            public long transferProbe(long handle, ByteBuffer buffer, int length) {
                return RfxNative.transferProbe(handle, buffer, length);
            }

            @Override
            public byte[] profilerTop(long handle, int limit) {
                return RfxNative.profilerTop(handle, limit);
            }

            @Override
            public int profilerRequestDepth(long handle, int levelCode, int ticks) {
                return RfxNative.profilerRequestDepth(handle, levelCode, ticks);
            }

            @Override
            public byte[] status(long handle) {
                return RfxNative.status(handle);
            }

            @Override
            public int panicTest(long handle) {
                return RfxNative.panicTest(handle);
            }

            @Override
            public int tickBegin(long handle, long tick, int side) {
                return RfxNative.tickBegin(handle, tick, side);
            }

            @Override
            public int tickPhase(long handle, int phase) {
                return RfxNative.tickPhase(handle, phase);
            }

            @Override
            public long tickEnd(long handle) {
                return RfxNative.tickEnd(handle);
            }

            @Override
            public ByteBuffer probeBufferAcquire(long handle, int threadId) {
                return RfxNative.probeBufferAcquire(handle, threadId);
            }

            @Override
            public int probeBufferFlush(long handle, int threadId, int used) {
                return RfxNative.probeBufferFlush(handle, threadId, used);
            }

            @Override
            public int workloadRegister(long handle, byte[] descriptorCbor) {
                return RfxNative.workloadRegister(handle, descriptorCbor);
            }

            @Override
            public byte[] probeLevels(long handle) {
                return RfxNative.probeLevels(handle);
            }

            @Override
            public int profilerStart(long handle) {
                return RfxNative.profilerStart(handle);
            }
        };
    }
}
