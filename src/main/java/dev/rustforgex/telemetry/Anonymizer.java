package dev.rustforgex.telemetry;

import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Anonymisation des chemins dans les rapports exportables (R-571).
 *
 * <p>Composant : C-35. Cahier des charges : PARTIE 5.33. Test : T-412.
 * Maturité : {@code STABLE}.
 *
 * <h2>Ce qu'un rapport ne doit pas contenir</h2>
 *
 * <p>Un rapport est fait pour être envoyé à quelqu'un — un mainteneur de mod, un
 * canal d'entraide. Le chemin d'installation d'un joueur y révèle son nom de session,
 * parfois son nom réel, parfois le nom de son employeur. R-571 l'interdit.
 *
 * <p>L'anonymisation est faite par <strong>remplacement de préfixe</strong>, du plus
 * long au plus court. Le chemin reste lisible et exploitable — {@code <gameDir>/mods/x.jar}
 * dit tout ce qu'un diagnostic demande — sans dire où il est.
 *
 * <h2>Pourquoi pas une expression régulière sur les noms</h2>
 *
 * <p>Chercher « un nom d'utilisateur » dans un texte suppose de savoir à quoi il
 * ressemble. Remplacer des racines connues ne suppose rien : on sait exactement quels
 * préfixes sont sensibles, puisqu'on les a en main.
 */
public final class Anonymizer {

    /** Remplacement du répertoire de jeu. */
    public static final String GAME_DIR = "<gameDir>";

    /** Remplacement du répertoire personnel de l'utilisateur. */
    public static final String HOME = "<home>";

    /** Racines à remplacer, de la plus longue à la plus courte. */
    private final Map<String, String> roots;

    private Anonymizer(Map<String, String> roots) {
        this.roots = Map.copyOf(roots);
    }

    /**
     * Construit un anonymiseur pour une installation donnée.
     *
     * @param gameDir répertoire de jeu, ou {@code null} s'il est inconnu
     * @param userHome répertoire personnel, ou {@code null}
     * @return l'anonymiseur
     */
    public static Anonymizer of(Path gameDir, Path userHome) {
        Map<String, String> roots = new LinkedHashMap<>();
        // Le répertoire de jeu d'abord : il est souvent SOUS le répertoire personnel,
        // et c'est le remplacement le plus informatif des deux.
        put(roots, gameDir, GAME_DIR);
        put(roots, userHome, HOME);
        return new Anonymizer(roots);
    }

    /** @return un anonymiseur pour l'installation courante */
    public static Anonymizer ofSystem(Path gameDir) {
        String home = System.getProperty("user.home");
        return of(gameDir, home == null ? null : Path.of(home));
    }

    /**
     * Remplace toute racine sensible dans un texte.
     *
     * <p>Les deux séparateurs sont traités : un chemin Windows peut apparaître avec des
     * barres obliques dans une trace, et l'inverse est vrai aussi.
     *
     * @param text texte à assainir, éventuellement {@code null}
     * @return le texte assaini, ou {@code null} si l'entrée l'était
     */
    public String clean(String text) {
        if (text == null || text.isEmpty() || roots.isEmpty()) {
            return text;
        }
        String cleaned = text;
        // Du plus long au plus court : un répertoire de jeu situé sous le répertoire
        // personnel doit être reconnu avant lui, sinon il ne le serait jamais.
        for (Map.Entry<String, String> root : roots.entrySet()
                .stream()
                .sorted((a, b) -> Integer.compare(b.getKey().length(), a.getKey().length()))
                .toList()) {
            cleaned = replaceBothSeparators(cleaned, root.getKey(), root.getValue());
        }
        return cleaned;
    }

    /**
     * Assainit un chemin.
     *
     * @param path chemin, éventuellement {@code null}
     * @return la forme assainie, ou {@code null} si l'entrée l'était
     */
    public String clean(Path path) {
        return path == null ? null : clean(path.toString());
    }

    /** @return le nombre de racines reconnues */
    public int rootCount() {
        return roots.size();
    }

    /**
     * Enregistre une racine telle qu'elle est donnée.
     *
     * <p>Surtout pas {@code toAbsolutePath()} : sur une racine qui n'est pas absolue
     * pour la plateforme courante, il la résout contre le répertoire de travail et
     * fabrique un préfixe qui ne correspond à rien. L'appelant fournit des chemins
     * absolus — le répertoire de jeu et {@code user.home} le sont toujours — et c'est
     * sa responsabilité, pas celle de l'anonymiseur de la deviner.
     */
    private static void put(Map<String, String> roots, Path path, String replacement) {
        if (path == null) {
            return;
        }
        String text = path.toString();
        if (!text.isBlank()) {
            roots.put(text, replacement);
        }
    }

    private static String replaceBothSeparators(String text, String root, String replacement) {
        String replaced = text.replace(root, replacement);
        String slashed = root.replace('\\', '/');
        if (!slashed.equals(root)) {
            replaced = replaced.replace(slashed, replacement);
        }
        String backslashed = root.replace('/', '\\');
        if (!backslashed.equals(root)) {
            replaced = replaced.replace(backslashed, replacement);
        }
        return replaced;
    }
}
