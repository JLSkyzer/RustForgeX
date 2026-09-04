package dev.rustforgex;

import dev.rustforgex.diag.CodeErreur;
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
class FondationsTest {

    /** Marqueurs de fiction interdits dans un module STABLE (contrat agent 3.1). */
    private static final List<String> MARQUEURS = List.of("TODO", "FIXME", "todo!(", "unimplemented!(");

    /** Racine du dépôt, le répertoire de travail des tests Gradle. */
    private static Path racine() {
        return Path.of("").toAbsolutePath();
    }

    /** Énumère les fichiers source Java et Rust du projet. */
    private static List<Path> sources() throws IOException {
        List<Path> fichiers = new ArrayList<>();
        for (String repertoire : List.of("src/main/java", "crates")) {
            Path base = racine().resolve(repertoire);
            if (!Files.isDirectory(base)) {
                continue;
            }
            try (Stream<Path> flux = Files.walk(base)) {
                flux.filter(Files::isRegularFile)
                        .filter(p -> {
                            String nom = p.getFileName().toString();
                            return nom.endsWith(".java") || nom.endsWith(".rs");
                        })
                        .forEach(fichiers::add);
            }
        }
        return fichiers;
    }

    /**
     * Indique qu'une occurrence est une citation documentaire, non un vrai marqueur.
     *
     * <p>La documentation doit pouvoir énoncer la règle qu'elle applique — par
     * exemple « ne contient aucun {@code TODO} » — sans déclencher ce test. Une
     * citation est écrite entre accents graves ; un marqueur réel ne l'est jamais.
     */
    private static boolean estCitation(String ligne, int position) {
        return position > 0 && ligne.charAt(position - 1) == '`';
    }

    /** Indique si la ligne est un commentaire de documentation. */
    private static boolean estDocumentation(String ligne) {
        String t = ligne.trim();
        return t.startsWith("///") || t.startsWith("//!") || t.startsWith("*") || t.startsWith("/**");
    }

    @Test
    @DisplayName("T-006 : aucun TODO, FIXME, todo!() ou unimplemented!() dans les sources")
    void aucuneFictionDansLesSources() throws IOException {
        List<Path> fichiers = sources();
        Assumptions.assumeFalse(fichiers.isEmpty(), "sources introuvables depuis " + racine());

        List<String> infractions = new ArrayList<>();
        for (Path fichier : fichiers) {
            List<String> lignes = Files.readAllLines(fichier, StandardCharsets.UTF_8);
            for (int i = 0; i < lignes.size(); i++) {
                String ligne = lignes.get(i);
                for (String marqueur : MARQUEURS) {
                    int position = ligne.indexOf(marqueur);
                    while (position >= 0) {
                        if (!estCitation(ligne, position)) {
                            infractions.add(racine().relativize(fichier) + ":" + (i + 1)
                                    + " contient « " + marqueur + " »");
                            break;
                        }
                        position = ligne.indexOf(marqueur, position + 1);
                    }
                }
            }
        }
        assertTrue(infractions.isEmpty(),
                "Fiction détectée dans des modules STABLE :\n  " + String.join("\n  ", infractions));
    }

    @Test
    @DisplayName("T-006 : le mot « placeholder » n'apparaît pas dans du code effectif")
    void aucunPlaceholderDansLeCode() throws IOException {
        List<Path> fichiers = sources();
        Assumptions.assumeFalse(fichiers.isEmpty(), "sources introuvables");

        List<String> infractions = new ArrayList<>();
        for (Path fichier : fichiers) {
            List<String> lignes = Files.readAllLines(fichier, StandardCharsets.UTF_8);
            for (int i = 0; i < lignes.size(); i++) {
                String ligne = lignes.get(i);
                // Les lignes de documentation peuvent citer le mot pour énoncer la
                // règle elle-même ; le code effectif, lui, ne le doit jamais.
                if (!estDocumentation(ligne)
                        && ligne.toLowerCase(Locale.ROOT).contains("placeholder")) {
                    infractions.add(racine().relativize(fichier) + ":" + (i + 1));
                }
            }
        }
        assertTrue(infractions.isEmpty(),
                "« placeholder » dans du code effectif :\n  " + String.join("\n  ", infractions));
    }

    @Test
    @DisplayName("T-008 : les codes d'erreur sont uniques et documentés")
    void codesErreurUniquesEtDocumentes() {
        Set<Integer> numeros = new HashSet<>();
        for (CodeErreur code : CodeErreur.values()) {
            assertTrue(numeros.add(code.numero()),
                    "le numéro " + code.numero() + " est utilisé deux fois");
            assertFalse(code.description().isBlank(), code + " : description manquante");
            assertTrue(code.identifiant().startsWith("E-"), code + " : identifiant mal formé");
        }
    }

    @Test
    @DisplayName("T-008 : les codes Java correspondent à l'annexe A.2 du cahier des charges")
    void codesErreurConformesAuCahierDesCharges() throws IOException {
        Path spec = racine().resolve("docs/spec/RUSTFORGE-X_Cahier_des_Charges_v1.0.md");
        Assumptions.assumeTrue(Files.isRegularFile(spec), "copie du cahier des charges absente");

        String contenu = Files.readString(spec, StandardCharsets.UTF_8);
        for (CodeErreur code : CodeErreur.values()) {
            assertTrue(contenu.contains("| " + code.identifiant() + " |"),
                    code.identifiant() + " ne figure pas dans l'annexe A.2 (contrat agent 4.7)");
        }
    }

    @Test
    @DisplayName("T-009 : la licence est présente et cohérente avec gradle.properties")
    void licenceCoherente() throws IOException {
        Path licence = racine().resolve("LICENSE");
        assertTrue(Files.isRegularFile(licence), "LICENSE absent du dépôt");
        assertFalse(Files.readString(licence, StandardCharsets.UTF_8).isBlank(), "LICENSE vide");

        String declaree = proprieteDuBuild("mod_license");
        assertEquals("All Rights Reserved", declaree,
                "mod_license a changé : mettre à jour LICENSE et rédiger ADR-010");
    }

    @Test
    @DisplayName("Le groupe et l'identifiant du mod restent cohérents avec le code")
    void identiteDuModCoherente() throws IOException {
        assertEquals(RustForgeX.MODID, proprieteDuBuild("mod_id"),
                "mod_id de gradle.properties et RustForgeX.MODID ont divergé");
        assertEquals("dev.rustforgex", proprieteDuBuild("mod_group_id"),
                "le groupe doit rester celui du package (ADR-013)");
    }

    /** Lit une propriété de {@code gradle.properties}. */
    private static String proprieteDuBuild(String cle) throws IOException {
        Path proprietes = racine().resolve("gradle.properties");
        assertTrue(Files.isRegularFile(proprietes), "gradle.properties introuvable");
        return Files.readAllLines(proprietes, StandardCharsets.UTF_8).stream()
                .map(String::trim)
                .filter(l -> l.startsWith(cle + "="))
                .map(l -> l.substring(cle.length() + 1).trim())
                .findFirst()
                .orElseThrow(() -> new AssertionError(cle + " absent de gradle.properties"));
    }
}
