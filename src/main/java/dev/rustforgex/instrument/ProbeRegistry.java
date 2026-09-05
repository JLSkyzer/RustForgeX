package dev.rustforgex.instrument;

import dev.rustforgex.bridge.Cbor;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.launch.ProbeIdSource;

import java.util.Map;
import java.util.concurrent.atomic.AtomicLong;

/**
 * Attribution des identifiants de sonde, côté jeu (ADR-017).
 *
 * <p>Composant : C-04. Cahier des charges : PARTIE 5.4, PARTIE 4.1. Exigences :
 * R-200 (identifiant reproductible d'un lancement à l'autre), R-700 (aucune traversée
 * de frontière dans la fenêtre de tick). Maturité : {@code STABLE}.
 *
 * <p>C'est l'implémentation que le mod remet au plugin de lancement. Elle décrit
 * l'unité de travail, la transmet au runtime natif, et rend l'identifiant que celui-ci
 * attribue. Le {@code WorkId} n'est calculé qu'à un seul endroit — dans le natif — et
 * l'espace des identifiants est donc unique, sans table de correspondance.
 *
 * <p>Un franchissement de frontière par méthode sondée, au chargement de sa classe.
 * Jamais pendant une fenêtre de tick : le budget de R-700 n'est pas concerné.
 *
 * <p><strong>Cette classe ne lève jamais.</strong> Elle est appelée depuis le
 * transformateur de bytecode, lui-même appelé par le chargeur de classes : une
 * exception échappée d'ici empêcherait une classe de se charger, donc le jeu de
 * démarrer, pour un défaut de profilage. Toute défaillance est comptée et rend
 * {@link ProbeIdSource#NO_PROBE}.
 */
public final class ProbeRegistry implements ProbeIdSource {

    /** Étiquette de côté transmise au natif : nom de variante de {@code DM-01 Side}. */
    private static final String SIDE_CLIENT = "Client";

    private static final String SIDE_SERVER = "Server";

    private final NativeBridge bridge;
    private final long handle;
    private final ModOwnerResolver owners;
    private final String side;

    /**
     * Correspondance trame de pile vers sonde, alimentée au fil des attributions.
     *
     * <p>C'est ici qu'elle se construit, et nulle part ailleurs : c'est le seul endroit
     * qui connaisse à la fois le nom de la méthode et l'identifiant qui lui a été
     * attribué.
     */
    private final StackFrameIndex frameIndex = new StackFrameIndex();

    private final AtomicLong requested = new AtomicLong();
    private final AtomicLong granted = new AtomicLong();
    private final AtomicLong refused = new AtomicLong();
    private final AtomicLong failed = new AtomicLong();

    /**
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @param owners résolveur du mod propriétaire
     * @param clientSide {@code true} côté client, {@code false} côté serveur dédié
     */
    public ProbeRegistry(
            NativeBridge bridge, long handle, ModOwnerResolver owners, boolean clientSide) {
        this.bridge = bridge;
        this.handle = handle;
        this.owners = owners;
        this.side = clientSide ? SIDE_CLIENT : SIDE_SERVER;
    }

    @Override
    public int probeIdFor(String classInternalName, String methodName, String methodDescriptor) {
        requested.incrementAndGet();
        try {
            Map<String, Object> descriptor = Cbor.map();
            descriptor.put("owner_id", owners.ownerOf(classInternalName));
            descriptor.put("class_internal_name", classInternalName);
            descriptor.put("method_name", methodName);
            descriptor.put("method_descriptor", methodDescriptor);
            // DM-02 n'est pas implémenté : le contexte d'appel n'est pas discriminant,
            // et zéro le dit explicitement. Une valeur inventée fausserait le WorkId.
            descriptor.put("call_context_hash", 0);
            descriptor.put("side", side);

            int probeId = bridge.workloadRegister(handle, Cbor.encode(descriptor));
            if (probeId >= 0) {
                granted.incrementAndGet();
                frameIndex.declare(classInternalName, methodName, probeId);
                return probeId;
            }
            // -1 : plafond d'unités suivies atteint, la méthode n'est pas sondée.
            // Au-delà : un code d'erreur de l'annexe A.2. Les deux mènent au même
            // comportement — pas de sonde — mais ne se comptent pas ensemble.
            if (probeId == NO_PROBE) {
                refused.incrementAndGet();
            } else {
                failed.incrementAndGet();
            }
            return NO_PROBE;
        } catch (RuntimeException | LinkageError e) {
            failed.incrementAndGet();
            return NO_PROBE;
        }
    }

    /** @return la correspondance entre trame de pile et identifiant de sonde */
    public StackFrameIndex frameIndex() {
        return frameIndex;
    }

    /** @return le nombre d'identifiants demandés */
    public long requested() {
        return requested.get();
    }

    /** @return le nombre d'identifiants attribués */
    public long granted() {
        return granted.get();
    }

    /** @return le nombre de refus, plafond d'unités suivies atteint (R-321) */
    public long refused() {
        return refused.get();
    }

    /** @return le nombre d'échecs : code d'erreur natif, ou exception absorbée */
    public long failed() {
        return failed.get();
    }
}
