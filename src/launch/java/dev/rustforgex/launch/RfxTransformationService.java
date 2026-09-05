package dev.rustforgex.launch;

import cpw.mods.modlauncher.api.IEnvironment;
import cpw.mods.modlauncher.api.ITransformationService;
import cpw.mods.modlauncher.api.ITransformer;

import java.util.List;
import java.util.Set;

/**
 * Laissez-passer vers la couche de plugins de ModLauncher (ADR-017).
 *
 * <p>Composant : C-04. Maturité : {@code STABLE}.
 *
 * <p>Ce service ne transforme rien et n'a pas vocation à le faire. Il existe pour une
 * seule raison, purement mécanique : {@code ModDirTransformerDiscoverer} ne promeut à
 * la couche de plugins que les JAR déclarant l'un de quatre services, dont
 * {@code ITransformationService}. Sans cette déclaration,
 * {@link RfxLaunchPlugin} — qui, lui, fait le travail — ne serait jamais découvert par
 * le {@code ServiceLoader}, puisque celui-ci ne cherche les {@code ILaunchPluginService}
 * que dans cette couche.
 *
 * <p>Le vrai transformateur n'est pas ici parce qu'un {@code ITransformer} énumère ses
 * cibles à l'enregistrement, avant tout chargement de classe : il faudrait déclarer par
 * avance chaque classe du modpack pour en sonder quelques-unes. C'est
 * {@code ILaunchPluginService} qui voit passer les classes une à une.
 *
 * <p>Aucune méthode ci-dessous ne fait quoi que ce soit d'observable. C'est
 * délibéré : ce code s'exécute avant le jeu, avant les mods, avant la configuration.
 */
public final class RfxTransformationService implements ITransformationService {

    /** Nom du service dans la piste d'audit de ModLauncher. */
    public static final String NAME = "rustforgex";

    /** Constructeur sans argument, exigé par {@code ServiceLoader}. */
    public RfxTransformationService() {
        // Intentionnellement vide : rien de ce dont ce mod a besoin n'existe encore.
    }

    @Override
    public String name() {
        return NAME;
    }

    @Override
    public void initialize(IEnvironment environment) {
        // Rien. L'initialisation réelle a lieu quand le mod arme RfxLaunchPlugin.
    }

    @Override
    public void onLoad(IEnvironment env, Set<String> otherServices) {
        // Rien. Ce service n'a pas d'état.
    }

    @Override
    @SuppressWarnings("rawtypes")
    public List<ITransformer> transformers() {
        // Aucun : voir la Javadoc de la classe. Le travail est fait par
        // RfxLaunchPlugin, qui n'a pas besoin de connaître ses cibles à l'avance.
        return List.of();
    }
}
