package dev.rustforgex.launch;

/**
 * Attribution d'un identifiant de sonde à une méthode (ADR-017).
 *
 * <p>Composant : C-04 (demandeur), C-05 (fournisseur). Cahier des charges :
 * PARTIE 5.4 et PARTIE 5.5. Maturité : {@code STABLE}.
 *
 * <p>Le plugin de lancement vit dans la couche de plugins de ModLauncher, chargée
 * avant les mods : il ne voit pas le code du mod, et ne peut donc appeler ni le
 * runtime natif, ni la liste des mods. C'est le mod qui, une fois prêt, lui remet une
 * implémentation de cette interface via {@link RfxLaunchPlugin#arm(ProbeIdSource)}.
 * Le sens de la dépendance est le seul possible : la couche du jeu voit la couche de
 * plugins, jamais l'inverse.
 *
 * <p>Le partage des rôles suit cette frontière. Le plugin ne connaît que du bytecode :
 * un nom de classe, un nom de méthode, un descripteur. <strong>Déterminer à quel mod
 * appartient une classe est le travail de l'implémentation</strong>, côté jeu, où la
 * liste des mods existe. Le plugin n'a donc aucune règle d'attribution en dur, ce qui
 * le tient à l'écart de INV-12.
 *
 * <p>L'implémentation est appelée depuis le fil qui charge la classe, quel qu'il
 * soit. Elle doit donc être utilisable par plusieurs fils à la fois, et
 * <strong>ne jamais lever d'exception</strong> : une méthode qu'on ne sait pas
 * identifier n'est pas sondée, et le chargement de la classe se poursuit.
 */
@FunctionalInterface
public interface ProbeIdSource {

    /** Valeur rendue quand la méthode ne sera pas sondée. */
    int NO_PROBE = -1;

    /**
     * Attribue un identifiant de sonde à une méthode.
     *
     * @param classInternalName nom interne de la classe, par exemple
     *     {@code net/minecraft/world/entity/Mob}
     * @param methodName nom de la méthode
     * @param methodDescriptor descripteur JVM de la méthode
     * @return l'identifiant de sonde, positif ou nul, ou {@link #NO_PROBE}
     */
    int probeIdFor(String classInternalName, String methodName, String methodDescriptor);
}
