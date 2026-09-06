package dev.rustforgex.forge;

import java.nio.file.Path;
import java.util.List;
import java.util.Set;

/**
 * Ce que RUSTFORGE-X a besoin de savoir d'un mod, et rien de plus (C-41).
 *
 * <p>Cahier des charges : PARTIE 5.39. Maturité : {@code STABLE}.
 *
 * <p>Cette interface existe pour une raison précise : {@link ModDiscovery} doit être
 * vérifiable sans Forge. La liste des mods n'est disponible qu'à l'intérieur d'une
 * instance de jeu lancée, et un composant qu'on ne peut tester que là est un composant
 * qu'on ne teste pas.
 *
 * <p>Elle ne décrit <strong>aucun mod en particulier</strong> et ne contient aucun nom
 * de mod (INV-12) : elle décrit la forme d'un mod, quel qu'il soit.
 */
public interface ModSource {

    /**
     * Énumère les mods présents.
     *
     * @return la liste, jamais {@code null}, éventuellement vide si le chargeur n'a pas
     *     encore dressé la sienne
     */
    List<RawMod> mods();

    /**
     * Un mod tel que le chargeur le décrit.
     *
     * @param modId identifiant déclaré, unique dans l'instance
     * @param version version déclarée, ou {@code "inconnue"}
     * @param file chemin du JAR ou du répertoire, ou {@code null} si le chargeur n'en
     *     expose pas — un mod en développement n'a pas toujours de fichier
     * @param moduleName nom du module Java portant ce mod, ou {@code null}
     * @param packages paquets déclarés par le fichier, jamais {@code null}
     */
    record RawMod(String modId, String version, Path file, String moduleName,
            Set<String> packages) {

        /** Valide les invariants d'un mod découvert. */
        public RawMod {
            if (modId == null || modId.isBlank()) {
                throw new IllegalArgumentException("un mod sans identifiant n'est pas un mod");
            }
            version = version == null || version.isBlank() ? "inconnue" : version;
            packages = packages == null ? Set.of() : Set.copyOf(packages);
        }
    }
}
