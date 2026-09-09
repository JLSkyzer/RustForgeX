package dev.rustforgex.instrument;

import dev.rustforgex.bridge.Cbor;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.launch.ProbeIdSource;

import java.util.Map;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
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
     * Sondes posées sur une classe qu'on n'a pas su rattacher à un mod (C-41).
     *
     * <p>Comptée ici parce que c'est le seul endroit qui voit passer chaque classe
     * sondée avec son propriétaire résolu. La PARTIE 5.39 en fait une métrique,
     * {@code rfx.discovery.unknown_owner_ratio}, et une condition d'acceptance : moins
     * de 5 % des classes chaudes doivent être en {@code unknown}.
     *
     * <p>Ce compteur en est une <strong>approximation par excès</strong> : il compte des
     * méthodes sondées, pas des classes, et toutes, pas seulement les chaudes. Il ne
     * suffit donc pas à prononcer l'acceptance — mais il dit si l'attribution marche.
     */
    private final AtomicLong unattributed = new AtomicLong();

    /**
     * Nom lisible de chaque méthode sondée, indexé par identifiant de sonde.
     *
     * <p>Le natif ne retient <strong>aucun</strong> nom : il calcule le {@code WorkId}
     * à l'enregistrement et n'en garde que l'entier. Quand il rend un classement, il
     * rend donc des identifiants de sonde, et c'est ici qu'on retrouve à quoi ils
     * correspondent — chez celui qui les a déclarés.
     *
     * <p>Garder les noms des deux côtés créerait deux sources de vérité pour la même
     * information, qui divergeraient au premier rechargement.
     *
     * <p>Les identifiants sont denses et attribués dans l'ordre : une liste
     * synchronisée suffit, et l'accès par indice est direct.
     */
    private final List<String> namesByProbeId =
            Collections.synchronizedList(new ArrayList<>(4096));

    /**
     * Mod propriétaire de chaque sonde, indexé par identifiant.
     *
     * <p>Retenu ici pour la même raison que le nom : le natif ne garde que le
     * {@code WorkId}. Et c'est l'information la plus utile de tout le classement — qui
     * exploite un serveur veut savoir <strong>quel mod</strong> lui coûte, pas quelle
     * classe.
     */
    private final List<String> ownersByProbeId =
            Collections.synchronizedList(new ArrayList<>(4096));

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
            String owner = owners.ownerOf(classInternalName);
            if (ModOwnerResolver.UNKNOWN.equals(owner)) {
                unattributed.incrementAndGet();
            }
            descriptor.put("owner_id", owner);
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
                rememberName(probeId, classInternalName, methodName, owner);
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

    /**
     * @return les sondes demandées sur une classe non rattachée à un mod (C-41)
     */
    public long unattributed() {
        return unattributed.get();
    }

    /**
     * Part des sondes demandées sur une classe non rattachée, en pourcent.
     *
     * <p>Approximation par excès de {@code rfx.discovery.unknown_owner_ratio} : voir
     * {@link #unattributed()}.
     *
     * @return le taux, ou {@code 0} si aucune sonde n'a encore été demandée
     */
    public double unknownOwnerPct() {
        long asked = requested.get();
        return asked == 0L ? 0.0 : unattributed.get() * 100.0 / asked;
    }

    /**
     * Retrouve la méthode que désigne un identifiant de sonde.
     *
     * @param probeId identifiant rendu par le natif
     * @return {@code classe#methode}, ou {@code null} si cet identifiant est inconnu
     */
    public String nameOf(int probeId) {
        return at(namesByProbeId, probeId);
    }

    /**
     * Retrouve le mod propriétaire d'une sonde.
     *
     * @param probeId identifiant rendu par le natif
     * @return l'identifiant du mod, ou {@code null} si cette sonde est inconnue
     */
    public String ownerOf(int probeId) {
        return at(ownersByProbeId, probeId);
    }

    /**
     * Retrouve le mod propriétaire d'une classe, sondée ou non.
     *
     * <p>La découverte des trames inconnues (C-05) désigne des méthodes qui n'ont
     * justement <strong>pas</strong> de sonde : il n'y a donc pas d'identifiant par où
     * passer, et c'est le résolveur qu'il faut interroger directement.
     *
     * @param classInternalName nom interne, par exemple {@code net/minecraft/…/Mob}
     * @return l'identifiant du mod, ou {@link ModOwnerResolver#UNKNOWN}
     */
    public String ownerOfClass(String classInternalName) {
        return owners.ownerOf(classInternalName);
    }

    /** Lecture bornée d'une des listes indexées par identifiant de sonde. */
    private static String at(List<String> list, int probeId) {
        if (probeId < 0) {
            return null;
        }
        synchronized (list) {
            return probeId < list.size() ? list.get(probeId) : null;
        }
    }

    /**
     * Retient le nom d'une sonde, à la position de son identifiant.
     *
     * <p>Les identifiants sont denses mais deux fils peuvent en obtenir deux dans un
     * ordre quelconque : la liste est donc comblée jusqu'à la position voulue plutôt
     * que simplement allongée. Un trou vaut mieux qu'un décalage — un décalage
     * attribuerait un coût à la mauvaise méthode, ce qui est pire que ne rien dire.
     */
    private void rememberName(int probeId, String classInternalName, String methodName,
            String owner) {
        String name = classInternalName.replace('/', '.') + '#' + methodName;
        put(namesByProbeId, probeId, name);
        put(ownersByProbeId, probeId, owner);
    }

    /** Écrit à la position de l'identifiant, en comblant les trous éventuels. */
    private static void put(List<String> list, int probeId, String value) {
        synchronized (list) {
            while (list.size() <= probeId) {
                list.add(null);
            }
            list.set(probeId, value);
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
