package dev.rustforgex.launch;

import cpw.mods.modlauncher.api.IEnvironment;
import cpw.mods.modlauncher.api.ITransformationService;
import cpw.mods.modlauncher.api.ITransformer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.nio.file.Path;
import java.util.List;
import java.util.Set;

/**
 * C-04 : point d'entrée du transformateur dans ModLauncher (ADR-019).
 *
 * <p>Composant : C-04. Maturité : {@code STABLE}.
 *
 * <p>Deux rôles, et il faut les distinguer :
 *
 * <ol>
 *   <li><strong>Le laissez-passer.</strong> {@code ModDirTransformerDiscoverer} ne
 *       promeut à la couche de plugins que les JAR déclarant l'un de quatre services,
 *       dont celui-ci. Sans cette déclaration, notre JAR resterait un fichier parmi les
 *       mods, et personne ne nous demanderait rien.
 *   <li><strong>Le travail.</strong> ModLauncher appelle {@link #transformers()} sur les
 *       services ainsi découverts — y compris les nôtres, ce que le journal d'un serveur
 *       de production confirme. C'est la seule voie par laquelle un JAR déposé dans
 *       {@code mods} peut transformer du bytecode.
 * </ol>
 *
 * <p>ADR-017 avait tenté un {@code ILaunchPluginService}, qui voit chaque classe sans
 * avoir à les nommer. Cette interface n'est cherchée que dans la couche d'amorçage,
 * bâtie depuis {@code legacyClassPath}, où un JAR de {@code mods} ne figure jamais. Voir
 * ADR-019 pour le détail de la vérification.
 */
public final class RfxTransformationService implements ITransformationService {

    /** Nom du service dans la piste d'audit de ModLauncher. */
    public static final String NAME = "rustforgex";

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-launch");

    /** Répertoire de jeu, connu après {@link #initialize(IEnvironment)}. */
    private Path gameDirectory;

    /** {@code true} sur un serveur dédié : le code client n'y est pas chargeable. */
    private boolean dedicatedServer;

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
        // Le répertoire de jeu mène au dossier `mods`, d'où viennent la plupart des
        // classes à cibler. Il n'est pas connu à la construction.
        gameDirectory = environment.getProperty(IEnvironment.Keys.GAMEDIR.get()).orElse(null);
        // La cible de lancement dit de quel côté on se trouve. Un serveur dédié ne peut
        // pas charger le code de rendu, et le désigner comme cible l'empêcherait de
        // démarrer.
        dedicatedServer = environment.getProperty(IEnvironment.Keys.LAUNCHTARGET.get())
                .map(target -> target.toLowerCase(java.util.Locale.ROOT).contains("server"))
                .orElse(true);
        if (gameDirectory == null) {
            LOGGER.warn("Répertoire de jeu inconnu : seules les classes de plateforme "
                    + "seront sondées.");
        }
    }

    @Override
    public void onLoad(IEnvironment env, Set<String> otherServices) {
        // Rien. L'énumération a lieu dans transformers(), au plus tard possible.
    }

    @Override
    @SuppressWarnings("rawtypes")
    public List<ITransformer> transformers() {
        try {
            Set<String> targets = TargetScanner.scan(gameDirectory, dedicatedServer);
            if (targets.isEmpty()) {
                LOGGER.warn("Aucune cible énumérée : aucune méthode ne sera sondée. "
                        + "Le jeu tourne normalement.");
                return List.of();
            }
            return List.of(new RfxClassTransformer(targets));
        } catch (RuntimeException | LinkageError e) {
            // Ce code s'exécute avant le jeu : une exception ici empêcherait le
            // démarrage. Aucune sonde vaut mieux qu'aucun jeu.
            LOGGER.error("Énumération des cibles impossible : aucune méthode ne sera "
                    + "sondée. Le jeu tourne normalement.", e);
            return List.of();
        }
    }
}
