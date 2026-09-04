package dev.rustforgex.config;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests de C-37 (Configuration), dont T-007 : toutes les options ont un défaut, une
 * plage et une description.
 */
class ConfigurationTest {

    /** Aucune propriété système de surcharge. */
    private static final java.util.function.UnaryOperator<String> SANS_SURCHARGE = cle -> null;

    private static java.util.function.UnaryOperator<String> surcharges(String... couples) {
        Map<String, String> table = new HashMap<>();
        for (int i = 0; i < couples.length; i += 2) {
            table.put(couples[i], couples[i + 1]);
        }
        return table::get;
    }

    @Test
    @DisplayName("T-007 : toute option a un défaut, une plage et une description (R-590)")
    void touteOptionEstCompletementDecrite() {
        List<OptionConfig> schema = Configuration.schema();
        assertFalse(schema.isEmpty(), "le schéma ne doit pas être vide");

        for (OptionConfig o : schema) {
            assertNotNull(o.defaut(), o.chemin() + " : défaut manquant");
            assertFalse(o.description().isBlank(), o.chemin() + " : description manquante");
            assertFalse(o.plageLisible().isBlank(), o.chemin() + " : plage manquante");

            switch (o.type()) {
                case ENTIER -> {
                    assertTrue(o.min() < o.max(), o.chemin() + " : plage entière vide");
                    long defaut = (Long) o.defaut();
                    assertTrue(defaut >= o.min() && defaut <= o.max(),
                            o.chemin() + " : le défaut est hors de sa propre plage");
                }
                case TEXTE -> assertTrue(o.valeursAdmises().contains((String) o.defaut()),
                        o.chemin() + " : le défaut n'est pas une valeur admise");
                case BOOLEEN -> assertTrue(o.defaut() instanceof Boolean,
                        o.chemin() + " : défaut non booléen");
            }
        }
    }

    @Test
    @DisplayName("T-007 : aucune option n'est déclarée deux fois")
    void lesCheminsSontUniques() {
        List<String> chemins = Configuration.schema().stream().map(OptionConfig::chemin).toList();
        assertEquals(chemins.size(), chemins.stream().distinct().count(),
                "deux options partagent le même chemin");
    }

    @Test
    @DisplayName("Les défauts correspondent à la PARTIE 28.2")
    void lesDefautsSontCeuxDuCahierDesCharges() {
        Configuration c = Configuration.parDefaut();
        assertTrue(c.booleen("general.enabled"));
        assertEquals("balanced", c.mode());
        assertTrue(c.booleen("general.side_client"));
        assertTrue(c.booleen("general.side_server"));
        assertEquals(512, c.entier("memory.max_native_mb"));
        assertTrue(c.booleen("telemetry.enabled"));
        assertEquals(3, c.entier("runtime.panic_threshold"));
    }

    @Test
    @DisplayName("Le fichier est créé avec les valeurs par défaut au premier lancement")
    void fichierCreeAuPremierLancement(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve(Configuration.CHEMIN_FICHIER);
        assertFalse(Files.exists(fichier));

        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);

        assertTrue(Files.isRegularFile(fichier), "le fichier doit avoir été créé");
        String contenu = Files.readString(fichier, StandardCharsets.UTF_8);
        assertTrue(contenu.contains("schema = 1"));
        assertTrue(contenu.contains("[general]"));
        assertTrue(contenu.contains("mode = \"balanced\""));
        // Chaque option est commentée dans le fichier généré (PARTIE 28.1).
        for (OptionConfig o : Configuration.schema()) {
            assertTrue(contenu.contains("# " + o.description()),
                    "description absente du fichier pour " + o.chemin());
        }
        assertTrue(c.avertissements().isEmpty(), "un fichier neuf ne produit aucun avertissement");
    }

    @Test
    @DisplayName("Les valeurs du fichier sont relues telles qu'écrites")
    void relectureDuFichier(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                schema = 1

                [general]
                enabled = false
                mode = "performance"

                [memory]
                max_native_mb = 1024
                """);

        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);

        assertFalse(c.booleen("general.enabled"));
        assertEquals("performance", c.mode());
        assertEquals(1024, c.entier("memory.max_native_mb"));
        // Les options absentes du fichier gardent leur défaut.
        assertEquals(3, c.entier("runtime.panic_threshold"));
        assertTrue(c.avertissements().isEmpty());
    }

    @Test
    @DisplayName("PARTIE 28.5 : une valeur hors plage est rejetée avec un message et remplacée")
    void valeurHorsPlageRejetee(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [memory]
                max_native_mb = 999999
                """);

        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);

        assertEquals(512, c.entier("memory.max_native_mb"), "le défaut doit s'appliquer");
        assertEquals(1, c.avertissements().size());
        String message = c.avertissements().get(0);
        assertTrue(message.contains("memory.max_native_mb"), message);
        assertTrue(message.contains("999999"), message);
        assertTrue(message.contains("16 .. 16384"), "la plage doit être rappelée : " + message);
    }

    @Test
    @DisplayName("PARTIE 28.5 : un mode inconnu est rejeté, jamais deviné")
    void modeInconnuRejete(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [general]
                mode = "turbo"
                """);

        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);

        assertEquals("balanced", c.mode());
        assertTrue(c.avertissements().get(0).contains("turbo"));
    }

    @Test
    @DisplayName("PARTIE 28.5 : une valeur de type incorrect est rejetée")
    void typeIncorrectRejete(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [general]
                enabled = peut-etre

                [runtime]
                panic_threshold = beaucoup
                """);

        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);

        assertTrue(c.booleen("general.enabled"), "défaut appliqué");
        assertEquals(3, c.entier("runtime.panic_threshold"), "défaut appliqué");
        assertEquals(2, c.avertissements().size());
    }

    @Test
    @DisplayName("R-591 : une clé inconnue est conservée et signalée, jamais supprimée")
    void cleInconnueConserveeEtSignalee(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [scheduler]
                workers = 8
                """);

        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);

        assertEquals(Map.of("scheduler.workers", "8"), c.clesInconnues(),
                "la valeur doit être conservée telle quelle");
        assertTrue(c.avertissements().get(0).contains("scheduler.workers"));
        // Elle doit être réécrite pour ne pas être perdue au prochain enregistrement.
        assertTrue(Configuration.rendreToml(c).contains("scheduler.workers = 8"));
    }

    @Test
    @DisplayName("PARTIE 28.4 : une propriété système l'emporte sur le fichier")
    void surchargeSystemePrioritaire(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [general]
                mode = "safe"
                """);

        Configuration c = Configuration.charger(
                fichier, surcharges("rustforgex.general.mode", "performance"));

        assertEquals("performance", c.mode(), "la propriété système doit primer");
    }

    @Test
    @DisplayName("PARTIE 28.4 : une surcharge système invalide est rejetée, le fichier subsiste")
    void surchargeSystemeInvalideRejetee(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [general]
                mode = "safe"
                """);

        Configuration c = Configuration.charger(
                fichier, surcharges("rustforgex.general.mode", "ultra"));

        assertEquals("safe", c.mode(), "la valeur du fichier reste en vigueur");
        assertTrue(c.avertissements().get(0).contains("propriété système"));
    }

    @Test
    @DisplayName("R-592 : les options structurelles sont marquées « redémarrage requis »")
    void optionsStructurellesMarquees() {
        assertFalse(Configuration.option("memory.max_native_mb").orElseThrow().rechargeableAChaud(),
                "le plafond mémoire natif ne peut pas changer à chaud");
        assertFalse(Configuration.option("general.enabled").orElseThrow().rechargeableAChaud());
        assertTrue(Configuration.option("general.mode").orElseThrow().rechargeableAChaud());

        String toml = Configuration.rendreToml(Configuration.parDefaut());
        assertTrue(toml.contains("(redemarrage requis)"),
                "le fichier doit signaler les options structurelles");
    }

    @Test
    @DisplayName("Le côté d'activation respecte enabled et side_client / side_server")
    void activationParCote(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [general]
                side_client = false
                """);
        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);
        assertFalse(c.activeSur(true), "désactivé côté client");
        assertTrue(c.activeSur(false), "toujours actif côté serveur");

        Files.writeString(fichier, """
                [general]
                enabled = false
                """);
        Configuration desactive = Configuration.charger(fichier, SANS_SURCHARGE);
        assertFalse(desactive.activeSur(true));
        assertFalse(desactive.activeSur(false), "enabled = false désactive les deux côtés");
    }

    @Test
    @DisplayName("Lire une option absente du schéma est une erreur de programmation")
    void lectureHorsSchemaRefusee() {
        Configuration c = Configuration.parDefaut();
        assertThrows(IllegalArgumentException.class, () -> c.booleen("general.inexistante"));
        assertThrows(IllegalArgumentException.class, () -> c.entier("general.mode"));
    }

    @Test
    @DisplayName("Un fichier illisible n'empêche pas le démarrage")
    void fichierIllisibleNeBloquePas(@TempDir Path racine) throws Exception {
        // Un répertoire à la place du fichier : la lecture échoue à coup sûr.
        Path fichier = racine.resolve("rustforgex.toml");
        Files.createDirectories(fichier);

        Configuration c = Configuration.charger(fichier, SANS_SURCHARGE);

        assertEquals("balanced", c.mode(), "les défauts doivent s'appliquer");
        assertFalse(c.avertissements().isEmpty(), "l'échec doit être signalé");
    }
}
