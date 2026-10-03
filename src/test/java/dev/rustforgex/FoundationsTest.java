package dev.rustforgex;

import dev.rustforgex.diag.ErrorCode;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests de fondations T-003 à T-010 (cahier des charges, PARTIE 20.3.1).
 *
 * <p>Ces tests portent sur le dépôt lui-même plutôt que sur une classe : ils
 * garantissent qu'aucune fiction ne s'installe dans un module déclaré {@code STABLE},
 * que les codes d'erreur restent uniques, et que les métadonnées du projet sont
 * cohérentes entre elles.
 */
class FoundationsTest {

    /** Classe de test citée par une étape de CI, par exemple {@code --tests 'a.b.C'}. */
    private static final Pattern CI_TEST_FILTER =
            Pattern.compile("--tests\\s+'([\\w.$]+)'");

    /**
     * Types dont la seule présence trahirait une connexion sortante (R-560, INV-15).
     *
     * <p>Côté Java comme côté Rust. La liste vise ce qui ouvre une connexion, pas ce qui
     * manipule une adresse : {@code URI} sert à nommer une ressource locale et reste
     * autorisé, {@code URL#openStream} non.
     */
    private static final List<String> NETWORK_TYPES = List.of(
            "java.net.Socket", "ServerSocket", "HttpClient", "HttpURLConnection",
            "URLConnection", "DatagramSocket", "SocketChannel", "InetAddress",
            "std::net::", "TcpStream", "TcpListener", "UdpSocket");

    /** Marqueurs de fiction interdits dans un module STABLE (contrat agent 3.1). */
    private static final List<String> MARKERS =
            List.of("TODO", "FIXME", "todo!(", "unimplemented!(");

    /** Racine du dépôt, le répertoire de travail des tests Gradle. */
    private static Path root() {
        return Path.of("").toAbsolutePath();
    }

    /** Énumère les fichiers source Java et Rust du projet. */
    private static List<Path> sources() throws IOException {
        List<Path> files = new ArrayList<>();
        for (String directory : List.of("src/main/java", "crates")) {
            Path base = root().resolve(directory);
            if (!Files.isDirectory(base)) {
                continue;
            }
            try (Stream<Path> stream = Files.walk(base)) {
                stream.filter(Files::isRegularFile)
                        .filter(p -> {
                            String name = p.getFileName().toString();
                            return name.endsWith(".java") || name.endsWith(".rs");
                        })
                        .forEach(files::add);
            }
        }
        return files;
    }

    /**
     * Indique qu'une occurrence est une citation documentaire, non un vrai marqueur.
     *
     * <p>La documentation doit pouvoir énoncer la règle qu'elle applique — par
     * exemple « ne contient aucun {@code TODO} » — sans déclencher ce test. Une
     * citation est écrite entre accents graves ; un marqueur réel ne l'est jamais.
     */
    private static boolean isQuotation(String line, int position) {
        return position > 0 && line.charAt(position - 1) == '`';
    }

    /** Indique si la ligne est un commentaire de documentation. */
    private static boolean isDocumentation(String line) {
        String t = line.trim();
        return t.startsWith("///") || t.startsWith("//!") || t.startsWith("*") || t.startsWith("/**");
    }

    @Test
    @DisplayName("T-006 : aucun TODO, FIXME, todo!() ou unimplemented!() dans les sources")
    void noFictionInTheSources() throws IOException {
        List<Path> files = sources();
        Assumptions.assumeFalse(files.isEmpty(), "sources introuvables depuis " + root());

        List<String> violations = new ArrayList<>();
        for (Path file : files) {
            List<String> lines = Files.readAllLines(file, StandardCharsets.UTF_8);
            for (int i = 0; i < lines.size(); i++) {
                String line = lines.get(i);
                for (String marker : MARKERS) {
                    int position = line.indexOf(marker);
                    while (position >= 0) {
                        if (!isQuotation(line, position)) {
                            violations.add(root().relativize(file) + ":" + (i + 1)
                                    + " contient « " + marker + " »");
                            break;
                        }
                        position = line.indexOf(marker, position + 1);
                    }
                }
            }
        }
        assertTrue(violations.isEmpty(),
                "Fiction détectée dans des modules STABLE :\n  " + String.join("\n  ", violations));
    }

    @Test
    @DisplayName("T-006 : le mot « placeholder » n'apparaît pas dans du code effectif")
    void noPlaceholderInEffectiveCode() throws IOException {
        List<Path> files = sources();
        Assumptions.assumeFalse(files.isEmpty(), "sources introuvables");

        List<String> violations = new ArrayList<>();
        for (Path file : files) {
            List<String> lines = Files.readAllLines(file, StandardCharsets.UTF_8);
            for (int i = 0; i < lines.size(); i++) {
                String line = lines.get(i);
                // Les lignes de documentation peuvent citer le mot pour énoncer la
                // règle elle-même ; le code effectif, lui, ne le doit jamais.
                if (!isDocumentation(line)
                        && line.toLowerCase(Locale.ROOT).contains("placeholder")) {
                    violations.add(root().relativize(file) + ":" + (i + 1));
                }
            }
        }
        assertTrue(violations.isEmpty(),
                "« placeholder » dans du code effectif :\n  " + String.join("\n  ", violations));
    }

    /**
     * Toute classe de test citée par l'intégration continue doit exister.
     *
     * <p>Ce test naît d'un défaut réel : une classe renommée de {@code FondationsTest}
     * en {@code FoundationsTest} lors du passage du code à l'anglais, sans que
     * {@code ci.yml} suive. Gradle échoue sur un filtre {@code --tests} qui ne
     * sélectionne rien — à juste titre — et l'étape nommée {@code lint-no-fiction} a
     * échoué à chaque exécution depuis. Personne ne l'a vu, parce que les tests
     * passaient en local.
     *
     * <p>Une étape de CI qui cite une classe inexistante est exactement la fiction que
     * {@code lint-no-fiction} traque. Elle relève donc du même test.
     */
    @Test
    @DisplayName("T-006 : les classes de test citées par la CI existent")
    void testClassesNamedByTheCiExist() throws IOException {
        Path workflow = root().resolve(".github/workflows/ci.yml");
        assertTrue(Files.isRegularFile(workflow), "workflow de CI introuvable : " + workflow);

        String content = Files.readString(workflow, StandardCharsets.UTF_8);
        Matcher matcher = CI_TEST_FILTER.matcher(content);
        int found = 0;
        while (matcher.find()) {
            found++;
            String className = matcher.group(1);
            Path source = root()
                    .resolve("src/test/java")
                    .resolve(className.replace('.', '/') + ".java");
            assertTrue(Files.isRegularFile(source),
                    "ci.yml cite « " + className + " », dont la source est absente : " + source);
        }
        assertTrue(found > 0,
                "aucun filtre --tests trouvé dans ci.yml : ce test ne vérifie plus rien");
    }

    /**
     * T-400 : RUSTFORGE-X n'ouvre aucune socket (R-560, INV-15).
     *
     * <p>« Aucune télémétrie externe » est une promesse que personne ne peut vérifier en
     * relisant du code, et que le premier ajout distrait romprait sans bruit. Elle se
     * vérifie donc mécaniquement : aucune source n'a le droit de nommer les types qui
     * ouvrent une connexion.
     *
     * <p>Le test porte sur les <em>noms de types</em>, pas sur les imports : une
     * référence pleinement qualifiée passerait sous un contrôle d'imports.
     */
    @Test
    @DisplayName("T-400 : aucune source ne nomme un type ouvrant une connexion réseau")
    void noSourceNamesANetworkType() throws IOException {
        List<Path> files = sources();
        Assumptions.assumeFalse(files.isEmpty(), "sources introuvables");

        List<String> violations = new ArrayList<>();
        for (Path file : files) {
            List<String> lines = Files.readAllLines(file, StandardCharsets.UTF_8);
            for (int i = 0; i < lines.size(); i++) {
                String line = lines.get(i);
                if (isDocumentation(line) || line.trim().startsWith("//")) {
                    // La documentation doit pouvoir énoncer l'interdiction elle-même.
                    continue;
                }
                for (String type : NETWORK_TYPES) {
                    if (line.contains(type)) {
                        violations.add(root().relativize(file) + ":" + (i + 1)
                                + " nomme « " + type + " »");
                    }
                }
            }
        }
        assertTrue(violations.isEmpty(),
                "R-560 et INV-15 : RUSTFORGE-X n'ouvre aucune connexion réseau."
                        + System.lineSeparator() + "  "
                        + String.join(System.lineSeparator() + "  ", violations));
    }

    @Test
    @DisplayName("T-008 : les codes d'erreur sont uniques et documentés")
    void errorCodesAreUniqueAndDocumented() {
        Set<Integer> numbers = new HashSet<>();
        for (ErrorCode code : ErrorCode.values()) {
            assertTrue(numbers.add(code.number()),
                    "le numéro " + code.number() + " est utilisé deux fois");
            assertFalse(code.description().isBlank(), code + " : description manquante");
            assertTrue(code.id().startsWith("E-"), code + " : identifiant mal formé");
        }
    }

    @Test
    @DisplayName("T-008 : les codes Java correspondent à l'annexe A.2 du cahier des charges")
    void errorCodesMatchTheSpecification() throws IOException {
        Path spec = root().resolve("docs/spec/RUSTFORGE-X_Cahier_des_Charges_v1.0.md");
        Assumptions.assumeTrue(Files.isRegularFile(spec), "copie du cahier des charges absente");

        String content = Files.readString(spec, StandardCharsets.UTF_8);
        for (ErrorCode code : ErrorCode.values()) {
            assertTrue(content.contains("| " + code.id() + " |"),
                    code.id() + " ne figure pas dans l'annexe A.2 (contrat agent 4.7)");
        }
    }

    @Test
    @DisplayName("T-009 : la licence est présente et cohérente avec gradle.properties")
    void licenseIsConsistent() throws IOException {
        Path license = root().resolve("LICENSE");
        assertTrue(Files.isRegularFile(license), "LICENSE absent du dépôt");
        assertFalse(Files.readString(license, StandardCharsets.UTF_8).isBlank(), "LICENSE vide");

        String declared = buildProperty("mod_license");
        assertEquals("Apache-2.0", declared,
                "mod_license a changé : mettre à jour LICENSE, NOTICE et ADR-010");
    }

    @Test
    @DisplayName("Le groupe et l'identifiant du mod restent cohérents avec le code")
    void modIdentityIsConsistent() throws IOException {
        assertEquals(RustForgeX.MODID, buildProperty("mod_id"),
                "mod_id de gradle.properties et RustForgeX.MODID ont divergé");
        assertEquals("dev.rustforgex", buildProperty("mod_group_id"),
                "le groupe doit rester celui du package (ADR-013)");
    }

    @Test
    @DisplayName("Les deux fichiers de langue déclarent exactement les mêmes clés")
    void bothLanguageFilesDeclareTheSameKeys() throws IOException {
        Set<String> french = languageKeys("fr_fr.json");
        Set<String> english = languageKeys("en_us.json");

        Set<String> missingInEnglish = new HashSet<>(french);
        missingInEnglish.removeAll(english);
        Set<String> missingInFrench = new HashSet<>(english);
        missingInFrench.removeAll(french);

        assertTrue(missingInEnglish.isEmpty(), "clés absentes de en_us.json : " + missingInEnglish);
        assertTrue(missingInFrench.isEmpty(), "clés absentes de fr_fr.json : " + missingInFrench);
        assertFalse(french.isEmpty(), "aucune clé de traduction déclarée");
    }

    /** Extrait les clés d'un fichier de langue, sans dépendance JSON. */
    private static Set<String> languageKeys(String name) throws IOException {
        Path file = root().resolve("src/main/resources/assets/rustforgex/lang").resolve(name);
        assertTrue(Files.isRegularFile(file), "fichier de langue introuvable : " + file);

        Set<String> keys = new HashSet<>();
        for (String line : Files.readAllLines(file, StandardCharsets.UTF_8)) {
            String trimmed = line.trim();
            int end = trimmed.indexOf("\":");
            if (trimmed.startsWith("\"") && end > 1) {
                keys.add(trimmed.substring(1, end));
            }
        }
        return keys;
    }

    /** Lit une propriété de {@code gradle.properties}. */
    private static String buildProperty(String key) throws IOException {
        Path properties = root().resolve("gradle.properties");
        assertTrue(Files.isRegularFile(properties), "gradle.properties introuvable");
        return Files.readAllLines(properties, StandardCharsets.UTF_8).stream()
                .map(String::trim)
                .filter(l -> l.startsWith(key + "="))
                .map(l -> l.substring(key.length() + 1).trim())
                .findFirst()
                .orElseThrow(() -> new AssertionError(key + " absent de gradle.properties"));
    }
}
