package dev.rustforgex.instrument;

import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.launch.RfxLaunchPlugin;

/**
 * Point de contact unique avec le plugin de lancement (ADR-017).
 *
 * <p>Composant : C-04. Maturité : {@code STABLE}.
 *
 * <p>RUSTFORGE-X est livré en deux fichiers : le mod et son plugin de lancement, ce
 * dernier étant nécessairement dans un JAR distinct — un JAR promu à la couche de
 * plugins de ModLauncher est exclu de la découverte des mods. L'utilisateur peut donc
 * n'installer que le mod.
 *
 * <p><strong>Toutes</strong> les références aux classes de {@code dev.rustforgex.launch}
 * sont rassemblées ici, et nulle part ailleurs. C'est ce qui rend l'absence du plugin
 * détectable : charger cette classe échoue alors avec un {@link NoClassDefFoundError},
 * capturé par l'appelant, tandis que le reste du mod continue de fonctionner. Disperser
 * ces références rendrait l'échec imprévisible, et probablement fatal.
 *
 * <p>Cette classe n'est jamais chargée avant d'être utilisée : elle n'a ni constante
 * ni état statique auquel un autre code ferait référence.
 */
final class InstrumentationLink {

    private InstrumentationLink() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Arme le transformateur de bytecode.
     *
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @param clientSide {@code true} côté client
     * @return le registre installé, pour ses compteurs
     */
    static ProbeRegistry arm(NativeBridge bridge, long handle, boolean clientSide) {
        if (!RfxLaunchPlugin.installed()) {
            // La classe est chargeable, mais ModLauncher ne l'a jamais instanciée : le
            // JAR n'a pas rejoint la couche d'amorçage. Armer réussirait sans rien
            // produire, et le mod annoncerait une instrumentation qui ne sonde rien.
            return null;
        }
        ProbeRegistry registry = new ProbeRegistry(bridge, handle, new ModOwnerResolver(), clientSide);
        RfxLaunchPlugin.arm(registry);
        return registry;
    }

    /** Désarme le transformateur : les classes chargées ensuite ne sont plus sondées. */
    static void disarm() {
        RfxLaunchPlugin.disarm();
    }

    /** @return le nombre de classes vues passer par le transformateur */
    static long classesSeen() {
        return RfxLaunchPlugin.classesSeen();
    }

    /** @return le nombre de méthodes effectivement sondées */
    static long methodsProbed() {
        return RfxLaunchPlugin.methodsProbed();
    }

    /** @return le nombre d'échecs de transformation (FM-09) */
    static long transformFailures() {
        return RfxLaunchPlugin.transformFailures();
    }
}
