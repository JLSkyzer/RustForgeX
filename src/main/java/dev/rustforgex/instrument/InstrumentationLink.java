package dev.rustforgex.instrument;

import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.launch.RfxClassTransformer;

/**
 * Point de contact unique avec le plugin de lancement (ADR-017).
 *
 * <p>Composant : C-04. Maturité : {@code STABLE}.
 *
 * <p>RUSTFORGE-X est livré en deux fichiers : le mod et son transformateur de bytecode,
 * ce dernier étant nécessairement dans un JAR distinct — un JAR déclarant un
 * {@code ITransformationService} est exclu de la découverte des mods. L'utilisateur
 * peut donc n'installer que le mod.
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
        if (!RfxClassTransformer.installed()) {
            // La classe est chargeable, mais ModLauncher n'a jamais réclamé nos cibles :
            // le transformateur n'est pas enregistré. Armer réussirait sans rien
            // produire, et le mod annoncerait une instrumentation qui ne sonde rien.
            return null;
        }
        ProbeRegistry registry = new ProbeRegistry(bridge, handle, new ModOwnerResolver(), clientSide);
        RfxClassTransformer.arm(registry);
        return registry;
    }

    /** Désarme le transformateur : les classes chargées ensuite ne sont plus sondées. */
    static void disarm() {
        RfxClassTransformer.disarm();
    }

    /** @return le nombre de classes vues passer par le transformateur */
    static long classesSeen() {
        return RfxClassTransformer.classesSeen();
    }

    /** @return le nombre de classes passées avant l'armement, donc jamais sondées */
    static long classesMissed() {
        return RfxClassTransformer.classesMissed();
    }

    /** @return le nombre de méthodes effectivement sondées */
    static long methodsProbed() {
        return RfxClassTransformer.methodsProbed();
    }

    /** @return le nombre d'échecs de transformation (FM-09) */
    static long transformFailures() {
        return RfxClassTransformer.transformFailures();
    }
}
