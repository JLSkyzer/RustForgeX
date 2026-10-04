package dev.rustforgex.bench;

import net.minecraft.server.level.ServerLevel;

import java.util.List;

/**
 * C-36 : passe d'un test qui juge une liste d'éléments, un par ligne — recettes
 * d'atelier de G-12, commandes de G-11.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Ses verdicts s'écrivent dans un fichier à part, voisin de l'empreinte de l'ouvrage
 * et suffixé ({@link #suffix()}) : ses éléments ne sont pas des chunks, et un élément
 * bruité ne doit pas rendre bruitée la comparaison de l'ouvrage. Chaque élément est une
 * clé du fichier ; ses composantes sont jugées comme celles d'un chunk.
 */
interface ResultSweep {

    /** Suffixe du fichier, par exemple {@code "-craft"}. */
    String suffix();

    /** Noms des composantes, dans l'ordre des empreintes de chaque élément. */
    List<String> components();

    /** Relève les éléments à juger. */
    void start(ServerLevel level);

    /**
     * Juge les éléments suivants, par petits lots pour rester loin du chien de garde.
     *
     * @return {@code true} quand tous l'ont été
     */
    boolean step(ServerLevel level);

    /** Éléments à juger. */
    int expected();

    /** Éléments jugés. */
    int judged();

    /** {@code true} quand tous les éléments ont été jugés. */
    boolean finished();

    /** Table JSON : une ligne par élément, ses empreintes dans l'ordre des composantes. */
    String json();

    /**
     * Ce qui a été haché, en clair, pour comprendre un écart ; vide si la passe n'en
     * garde rien. N'est écrit qu'en diagnostic ({@code rustforgex.bench.digest.details}).
     */
    default String details() {
        return "";
    }

    /**
     * Empreinte FNV-1a sur 64 bits des caractères d'une description, en hexadécimal :
     * stable d'un lancement de la JVM à l'autre, contrairement à l'identité d'un objet.
     */
    static String hash(String text) {
        long hash = 0xcbf29ce484222325L;
        for (int i = 0; i < text.length(); i++) {
            hash ^= text.charAt(i);
            hash *= 0x100000001b3L;
        }
        return String.format(java.util.Locale.ROOT, "%016x", hash);
    }

    /** Table JSON d'éléments : {@code "clé": ["empreinte", …]}, une ligne par élément. */
    static String table(List<String> keys, List<List<String>> rows) {
        StringBuilder table = new StringBuilder(keys.size() * 64);
        for (int i = 0; i < keys.size(); i++) {
            table.append(i == 0 ? "\n" : ",\n").append("    \"")
                    .append(keys.get(i).replace("\\", "\\\\").replace("\"", "\\\""))
                    .append("\": [");
            List<String> row = rows.get(i);
            for (int k = 0; k < row.size(); k++) {
                table.append(k == 0 ? "\"" : ", \"").append(row.get(k)).append('"');
            }
            table.append(']');
        }
        return table.toString();
    }
}
