package dev.rustforgex.forge;

import java.util.Optional;

/**
 * C-01, FM-01 : vérification de la version de Forge.
 *
 * <p>Cahier des charges : PARTIE 5.1. Test : T-102. Maturité : {@code STABLE}.
 *
 * <p>Une version hors de la plage supportée n'empêche pas le jeu de démarrer : elle
 * place RUSTFORGE-X en mode observation seule, avec {@code E-1001}, et aucune
 * transformation n'est entreprise.
 */
public final class VersionForge {

    /**
     * Version majeure minimale supportée, incluse.
     *
     * <p>DOIT rester cohérente avec {@code loader_version_range} de
     * {@code gradle.properties}, qui alimente {@code mods.toml}. La cohérence des deux
     * est vérifiée par un test, pour qu'une mise à jour de l'un sans l'autre échoue au
     * build plutôt qu'à l'exécution.
     */
    public static final int MAJEURE_MINIMALE = 47;

    /** Première version majeure non supportée. */
    public static final int MAJEURE_EXCLUE = 48;

    private VersionForge() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Extrait la version majeure d'une version de Forge.
     *
     * @param version version complète, par exemple {@code 47.4.23}
     * @return la majeure, ou un résultat vide si la chaîne n'est pas exploitable
     */
    public static Optional<Integer> majeure(String version) {
        if (version == null || version.isBlank()) {
            return Optional.empty();
        }
        int fin = 0;
        while (fin < version.length() && Character.isDigit(version.charAt(fin))) {
            fin++;
        }
        if (fin == 0) {
            return Optional.empty();
        }
        try {
            return Optional.of(Integer.parseInt(version.substring(0, fin)));
        } catch (NumberFormatException e) {
            // Une majeure ne tenant pas dans un int ne correspond à aucune version
            // réelle de Forge : on la traite comme illisible plutôt que de la deviner.
            return Optional.empty();
        }
    }

    /**
     * Indique si une version de Forge est dans la plage supportée.
     *
     * <p>Une version illisible est considérée comme non supportée : le principe
     * conservateur veut qu'une information absente soit traitée comme le pire cas
     * (contrat agent 4.4).
     *
     * @param version version complète rapportée par Forge
     * @return {@code true} si la version est supportée
     */
    public static boolean estSupportee(String version) {
        return majeure(version)
                .filter(m -> m >= MAJEURE_MINIMALE && m < MAJEURE_EXCLUE)
                .isPresent();
    }

    /** @return la plage supportée, sous forme lisible, par exemple {@code [47,48)} */
    public static String plageLisible() {
        return "[" + MAJEURE_MINIMALE + "," + MAJEURE_EXCLUE + ")";
    }
}
