package dev.rustforgex.analysis;

import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Classes qu'un access transformer ou un coremod de Forge modifie au chargement.
 *
 * <p>Composant : C-07 (ADR-051). Maturité : {@code STABLE}.
 *
 * <p>Le bytecode lu dans l'archive ne dit pas la vérité sur ces classes, qui sont donc
 * déclarées absentes, comme les cibles de mixin :
 * <ul>
 *   <li>un <strong>access transformer</strong> ({@code META-INF/accesstransformer.cfg})
 *       peut retirer {@code final} ou rendre publique une méthode privée ; or l'analyse
 *       tient un appel vers une méthode finale ou privée pour monomorphe ;</li>
 *   <li>un <strong>coremod</strong> ({@code META-INF/coremods.json}, scripts JS) peut
 *       tout réécrire. Ses cibles sont calculées par le script : on retient tout
 *       littéral qui a la forme d'un nom de classe, dans le script et dans les
 *       fichiers de données {@code .json} qu'il nomme (Forge lit ainsi les cibles de
 *       {@code method_redirector.js}, ADR-059). C'est une surapproximation, donc
 *       prudente ; un nom construit par concaténation échapperait, risque résiduel
 *       consigné dans l'ADR-051 et l'ADR-059.</li>
 * </ul>
 */
public final class TransformedClasses {

    /** Littéral JS de la forme {@code a.b.C} ou {@code a/b/C}. */
    private static final Pattern CLASS_LITERAL = Pattern.compile(
            "([\"'])([A-Za-z_$][\\w$]*(?:[./][A-Za-z_$][\\w$]*)+)\\1");

    /** Chemin d'un script dans {@code coremods.json}. */
    private static final Pattern SCRIPT = Pattern.compile("\"([^\"]+\\.js)\"");

    /** Littéral JS qui nomme un fichier de données {@code .json}. */
    private static final Pattern DATA_FILE = Pattern.compile("([\"'])([^\"'\\s]+\\.json)\\1");

    private TransformedClasses() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Relève les classes nommées par un fichier d'access transformers.
     *
     * <p>Format d'une ligne : {@code <accès>[-f|+f] <classe> [<membre>] [# commentaire]},
     * la classe en notation pointée.
     *
     * @param text contenu du fichier
     * @param classes ensemble à compléter, en noms internes
     */
    public static void accessTransformer(String text, Set<String> classes) {
        for (String line : text.split("\r?\n")) {
            int comment = line.indexOf('#');
            String entry = (comment >= 0 ? line.substring(0, comment) : line).trim();
            if (entry.isEmpty()) {
                continue;
            }
            String[] tokens = entry.split("\\s+");
            if (tokens.length >= 2) {
                classes.add(tokens[1].replace('.', '/'));
            }
        }
    }

    /**
     * @param json contenu de {@code META-INF/coremods.json}
     * @return les chemins des scripts déclarés, relatifs à la racine de l'archive
     */
    public static List<String> coreModScripts(String json) {
        List<String> scripts = new ArrayList<>();
        Matcher matcher = SCRIPT.matcher(json);
        while (matcher.find()) {
            scripts.add(matcher.group(1));
        }
        return scripts;
    }

    /**
     * Relève tout littéral d'un script de coremod qui a la forme d'un nom de classe.
     *
     * @param script contenu du script JS
     * @param classes ensemble à compléter, en noms internes
     */
    public static void coreModScript(String script, Set<String> classes) {
        Matcher matcher = CLASS_LITERAL.matcher(script);
        while (matcher.find()) {
            classes.add(matcher.group(2).replace('.', '/'));
        }
    }

    /**
     * @param script contenu d'un script de coremod
     * @return les fichiers de données {@code .json} que le script nomme, relatifs à la
     *     racine de l'archive ; leurs littéraux en forme de nom de classe sont des cibles
     */
    public static List<String> coreModDataFiles(String script) {
        List<String> files = new ArrayList<>();
        Matcher matcher = DATA_FILE.matcher(script);
        while (matcher.find()) {
            files.add(matcher.group(2));
        }
        return files;
    }

    /**
     * @param script contenu d'un script de coremod
     * @return {@code true} si le script charge des données ({@code ASMAPI.loadData}) :
     *     sans fichier nommé, ses cibles sont inconnues
     */
    public static boolean loadsData(String script) {
        return script.contains("loadData");
    }
}
