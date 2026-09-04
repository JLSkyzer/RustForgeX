package dev.rustforgex.config;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Lecteur TOML minimal, restreint au format du fichier de configuration.
 *
 * <p>Composant : C-37. Cahier des charges : PARTIE 28.1. Maturité : {@code STABLE}.
 *
 * <p>Le fichier de configuration n'utilise qu'un sous-ensemble de TOML : des sections
 * {@code [nom]}, des affectations {@code clé = valeur} sur une ligne, des commentaires
 * introduits par {@code #}, et des valeurs scalaires. Ce lecteur couvre exactement ce
 * sous-ensemble et rend les valeurs sous forme textuelle : c'est {@link Configuration}
 * qui les convertit et les valide selon le type déclaré au schéma.
 *
 * <p>Ce choix évite d'ajouter une dépendance TOML au JAR du mod pour une poignée de
 * lignes, et garde C-37 indépendant de toute bibliothèque de l'écosystème Forge (P-11).
 *
 * <p>Ce qui n'est pas couvert — tables imbriquées, tableaux, chaînes multilignes,
 * dates — n'apparaît pas dans le fichier généré. Une ligne non reconnue est ignorée
 * plutôt que d'interrompre le chargement : la configuration doit toujours aboutir à un
 * état utilisable (PARTIE 28.5).
 */
final class FlatToml {

    private FlatToml() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Lit un document et rend les valeurs indexées par leur chemin complet.
     *
     * <p>Une clé hors de toute section est indexée par son seul nom, ce qui permet de
     * lire {@code schema} en tête de fichier.
     *
     * @param content contenu du fichier
     * @return les couples chemin/valeur, dans l'ordre du fichier
     */
    static Map<String, String> read(String content) {
        Map<String, String> values = new LinkedHashMap<>();
        String section = "";

        for (String rawLine : content.split("\r?\n", -1)) {
            String line = stripComment(rawLine).trim();
            if (line.isEmpty()) {
                continue;
            }

            if (line.startsWith("[") && line.endsWith("]")) {
                section = line.substring(1, line.length() - 1).trim();
                continue;
            }

            int separator = line.indexOf('=');
            if (separator <= 0) {
                continue;
            }
            String key = line.substring(0, separator).trim();
            String value = line.substring(separator + 1).trim();
            if (key.isEmpty()) {
                continue;
            }
            values.put(section.isEmpty() ? key : section + "." + key, value);
        }
        return values;
    }

    /**
     * Retire un commentaire de fin de ligne, sans toucher aux {@code #} entre
     * guillemets.
     */
    private static String stripComment(String line) {
        boolean inString = false;
        for (int i = 0; i < line.length(); i++) {
            char c = line.charAt(i);
            if (c == '"') {
                inString = !inString;
            } else if (c == '#' && !inString) {
                return line.substring(0, i);
            }
        }
        return line;
    }
}
