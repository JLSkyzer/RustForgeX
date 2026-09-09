package dev.rustforgex.bridge;

import java.nio.ByteBuffer;

/**
 * Pont natif inerte, base commune des ponts simulés des tests.
 *
 * <p>Chaque méthode rend une réponse neutre : succès pour ce qui renvoie un code,
 * {@code null} pour ce qui renvoie un objet. Un test n'a plus qu'à redéfinir les
 * points d'entrée qui l'intéressent.
 *
 * <p>Sans cette classe, tout ajout à {@link NativeBridge} cassait la compilation de
 * chaque test qui simulait le pont, pour des méthodes qu'aucun d'eux n'utilisait.
 * L'interface, elle, reste sans implémentation par défaut : le pont réel doit
 * continuer à échouer à la compilation s'il oublie un point d'entrée.
 */
public class FakeNativeBridge implements NativeBridge {

    /** Handle rendu par défaut, du même motif que celui du runtime natif. */
    public static final long HANDLE = 0x5246_5800_0000_0001L;

    @Override
    public int abiVersion() {
        return RfxNative.EXPECTED_ABI;
    }

    @Override
    public long init(byte[] configCbor) {
        return HANDLE;
    }

    @Override
    public int shutdown(long handle) {
        return 0;
    }

    @Override
    public int noop(long handle) {
        return 0;
    }

    @Override
    public int hwProbe(long handle) {
        return 0;
    }

    @Override
    public int hwSetFfiCosts(long handle, int jniCallNs, int ffiBatchNsPerKb) {
        return 0;
    }

    @Override
    public long transferProbe(long handle, ByteBuffer buffer, int length) {
        return length;
    }

    @Override
    public byte[] status(long handle) {
        return new byte[0];
    }

    /**
     * Aucun classement : ce double n'a pas de profileur derrière lui.
     *
     * <p>{@code null} plutôt qu'un tableau vide — un tableau vide serait un CBOR
     * illisible, donc une erreur de décodage, là où {@code null} dit exactement ce
     * qu'il en est : le classement n'est pas disponible.
     */
    @Override
    public byte[] profilerTop(long handle, int limit) {
        return null;
    }

    /** Aucun profileur derrière ce double : la demande est refusée, pas ignorée. */
    @Override
    public int profilerRequestDepth(long handle, int levelCode, int ticks) {
        return -1001;
    }

    @Override
    public int panicTest(long handle) {
        return -3001;
    }

    @Override
    public int tickBegin(long handle, long tick, int side) {
        return 0;
    }

    @Override
    public int tickPhase(long handle, int phase) {
        return 0;
    }

    @Override
    public long tickEnd(long handle) {
        return 0;
    }

    @Override
    public ByteBuffer probeBufferAcquire(long handle, int threadId) {
        return null;
    }

    @Override
    public int probeBufferFlush(long handle, int threadId, int used) {
        return 0;
    }

    @Override
    public int workloadRegister(long handle, byte[] descriptorCbor) {
        // Chaque descripteur reçoit un identifiant distinct, dans l'ordre où il
        // arrive : c'est le comportement du natif, en beaucoup plus simple.
        return nextProbeId++;
    }

    /** Prochain identifiant de sonde attribué par {@link #workloadRegister}. */
    protected int nextProbeId;

    @Override
    public byte[] probeLevels(long handle) {
        // Aucune table à appliquer : le pont simulé ne profile rien.
        return null;
    }

    @Override
    public int profilerStart(long handle) {
        return 0;
    }
}
