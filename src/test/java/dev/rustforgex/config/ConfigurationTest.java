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
    private static final java.util.function.UnaryOperator<String> NO_OVERRIDE = key -> null;

    private static java.util.function.UnaryOperator<String> overrides(String... pairs) {
        Map<String, String> table = new HashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            table.put(pairs[i], pairs[i + 1]);
        }
        return table::get;
    }

    @Test
    @DisplayName("T-007 : toute option a un défaut, une plage et une description (R-590)")
    void everyOptionIsFullyDescribed() {
        List<OptionConfig> schema = Configuration.schema();
        assertFalse(schema.isEmpty(), "le schéma ne doit pas être vide");

        for (OptionConfig o : schema) {
            assertNotNull(o.defaultValue(), o.path() + " : défaut manquant");
            assertFalse(o.description().isBlank(), o.path() + " : description manquante");
            assertFalse(o.readableRange().isBlank(), o.path() + " : plage manquante");

            switch (o.type()) {
                case INTEGER -> {
                    assertTrue(o.min() < o.max(), o.path() + " : plage entière vide");
                    long defaultValue = (Long) o.defaultValue();
                    assertTrue(defaultValue >= o.min() && defaultValue <= o.max(),
                            o.path() + " : le défaut est hors de sa propre plage");
                }
                case TEXT -> assertTrue(o.allowedValues().contains((String) o.defaultValue()),
                        o.path() + " : le défaut n'est pas une valeur admise");
                case BOOLEAN -> assertTrue(o.defaultValue() instanceof Boolean,
                        o.path() + " : défaut non booléen");
            }
        }
    }

    @Test
    @DisplayName("T-007 : aucune option n'est déclarée deux fois")
    void pathsAreUnique() {
        List<String> paths = Configuration.schema().stream().map(OptionConfig::path).toList();
        assertEquals(paths.size(), paths.stream().distinct().count(),
                "deux options partagent le même chemin");
    }

    @Test
    @DisplayName("Les défauts correspondent à la PARTIE 28.2")
    void defaultsMatchTheSpecification() {
        Configuration c = Configuration.defaults();
        assertTrue(c.getBoolean("general.enabled"));
        assertEquals("balanced", c.mode());
        assertTrue(c.getBoolean("general.side_client"));
        assertTrue(c.getBoolean("general.side_server"));
        assertEquals(512, c.getLong("memory.max_native_mb"));
        assertTrue(c.getBoolean("telemetry.enabled"));
        assertEquals(3, c.getLong("runtime.panic_threshold"));
    }

    @Test
    @DisplayName("Le fichier est créé avec les valeurs par défaut au premier lancement")
    void fileIsCreatedOnFirstLaunch(@TempDir Path root) throws Exception {
        Path file = root.resolve(Configuration.FILE_PATH);
        assertFalse(Files.exists(file));

        Configuration c = Configuration.load(file, NO_OVERRIDE);

        assertTrue(Files.isRegularFile(file), "le fichier doit avoir été créé");
        String content = Files.readString(file, StandardCharsets.UTF_8);
        assertTrue(content.contains("schema = 1"));
        assertTrue(content.contains("[general]"));
        assertTrue(content.contains("mode = \"balanced\""));
        // Chaque option est commentée dans le fichier généré (PARTIE 28.1).
        for (OptionConfig o : Configuration.schema()) {
            assertTrue(content.contains("# " + o.description()),
                    "description absente du fichier pour " + o.path());
        }
        assertTrue(c.warnings().isEmpty(), "un fichier neuf ne produit aucun avertissement");
    }

    @Test
    @DisplayName("Les valeurs du fichier sont relues telles qu'écrites")
    void fileIsReadBack(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                schema = 1

                [general]
                enabled = false
                mode = "performance"

                [memory]
                max_native_mb = 1024
                """);

        Configuration c = Configuration.load(file, NO_OVERRIDE);

        assertFalse(c.getBoolean("general.enabled"));
        assertEquals("performance", c.mode());
        assertEquals(1024, c.getLong("memory.max_native_mb"));
        // Les options absentes du fichier gardent leur défaut.
        assertEquals(3, c.getLong("runtime.panic_threshold"));
        assertTrue(c.warnings().isEmpty());
    }

    @Test
    @DisplayName("PARTIE 28.5 : une valeur hors plage est rejetée avec un message et remplacée")
    void outOfRangeValueIsRejected(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [memory]
                max_native_mb = 999999
                """);

        Configuration c = Configuration.load(file, NO_OVERRIDE);

        assertEquals(512, c.getLong("memory.max_native_mb"), "le défaut doit s'appliquer");
        assertEquals(1, c.warnings().size());
        String message = c.warnings().get(0);
        assertTrue(message.contains("memory.max_native_mb"), message);
        assertTrue(message.contains("999999"), message);
        assertTrue(message.contains("16 .. 16384"), "la plage doit être rappelée : " + message);
    }

    @Test
    @DisplayName("PARTIE 28.5 : un mode inconnu est rejeté, jamais deviné")
    void unknownModeIsRejected(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [general]
                mode = "turbo"
                """);

        Configuration c = Configuration.load(file, NO_OVERRIDE);

        assertEquals("balanced", c.mode());
        assertTrue(c.warnings().get(0).contains("turbo"));
    }

    @Test
    @DisplayName("PARTIE 28.5 : une valeur de type incorrect est rejetée")
    void wrongTypeIsRejected(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [general]
                enabled = peut-etre

                [runtime]
                panic_threshold = beaucoup
                """);

        Configuration c = Configuration.load(file, NO_OVERRIDE);

        assertTrue(c.getBoolean("general.enabled"), "défaut appliqué");
        assertEquals(3, c.getLong("runtime.panic_threshold"), "défaut appliqué");
        assertEquals(2, c.warnings().size());
    }

    @Test
    @DisplayName("R-591 : une clé inconnue est conservée et signalée, jamais supprimée")
    void unknownKeyIsKeptAndReported(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [scheduler]
                workers = 8
                """);

        Configuration c = Configuration.load(file, NO_OVERRIDE);

        assertEquals(Map.of("scheduler.workers", "8"), c.unknownKeys(),
                "la valeur doit être conservée telle quelle");
        assertTrue(c.warnings().get(0).contains("scheduler.workers"));
        // Elle doit être réécrite pour ne pas être perdue au prochain enregistrement.
        assertTrue(Configuration.renderToml(c).contains("scheduler.workers = 8"));
    }

    @Test
    @DisplayName("PARTIE 28.4 : une propriété système l'emporte sur le fichier")
    void systemPropertyTakesPrecedence(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [general]
                mode = "safe"
                """);

        Configuration c = Configuration.load(
                file, overrides("rustforgex.general.mode", "performance"));

        assertEquals("performance", c.mode(), "la propriété système doit primer");
    }

    @Test
    @DisplayName("PARTIE 28.4 : une surcharge système invalide est rejetée, le fichier subsiste")
    void invalidSystemPropertyIsRejected(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [general]
                mode = "safe"
                """);

        Configuration c = Configuration.load(
                file, overrides("rustforgex.general.mode", "ultra"));

        assertEquals("safe", c.mode(), "la valeur du fichier reste en vigueur");
        assertTrue(c.warnings().get(0).contains("propriété système"));
    }

    @Test
    @DisplayName("R-592 : les options structurelles sont marquées « redémarrage requis »")
    void structuralOptionsAreMarked() {
        assertFalse(Configuration.option("memory.max_native_mb").orElseThrow().hotReloadable(),
                "le plafond mémoire natif ne peut pas changer à chaud");
        assertFalse(Configuration.option("general.enabled").orElseThrow().hotReloadable());
        assertTrue(Configuration.option("general.mode").orElseThrow().hotReloadable());

        String toml = Configuration.renderToml(Configuration.defaults());
        assertTrue(toml.contains("(redemarrage requis)"),
                "le fichier doit signaler les options structurelles");
    }

    @Test
    @DisplayName("Le côté d'activation respecte enabled et side_client / side_server")
    void activationPerSide(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [general]
                side_client = false
                """);
        Configuration c = Configuration.load(file, NO_OVERRIDE);
        assertFalse(c.enabledOn(true), "désactivé côté client");
        assertTrue(c.enabledOn(false), "toujours actif côté serveur");

        Files.writeString(file, """
                [general]
                enabled = false
                """);
        Configuration disabled = Configuration.load(file, NO_OVERRIDE);
        assertFalse(disabled.enabledOn(true));
        assertFalse(disabled.enabledOn(false), "enabled = false désactive les deux côtés");
    }

    @Test
    @DisplayName("Lire une option absente du schéma est une erreur de programmation")
    void readingOutsideTheSchemaIsRefused() {
        Configuration c = Configuration.defaults();
        assertThrows(IllegalArgumentException.class, () -> c.getBoolean("general.inexistante"));
        assertThrows(IllegalArgumentException.class, () -> c.getLong("general.mode"));
    }

    @Test
    @DisplayName("Un fichier illisible n'empêche pas le démarrage")
    void unreadableFileDoesNotBlockStartup(@TempDir Path root) throws Exception {
        // Un répertoire à la place du fichier : la lecture échoue à coup sûr.
        Path file = root.resolve("rustforgex.toml");
        Files.createDirectories(file);

        Configuration c = Configuration.load(file, NO_OVERRIDE);

        assertEquals("balanced", c.mode(), "les défauts doivent s'appliquer");
        assertFalse(c.warnings().isEmpty(), "l'échec doit être signalé");
    }
}
