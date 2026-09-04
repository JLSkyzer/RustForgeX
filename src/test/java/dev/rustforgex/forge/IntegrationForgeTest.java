package dev.rustforgex.forge;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Tests T-102 et T-103 de C-01 (Forge Integration). */
class IntegrationForgeTest {

    @Test
    @DisplayName("T-102 : une version de Forge hors plage n'est pas supportée (FM-01)")
    void versionsHorsPlageRefusees() {
        assertTrue(VersionForge.estSupportee("47.4.23"));
        assertTrue(VersionForge.estSupportee("47.0.0"));
        assertTrue(VersionForge.estSupportee("47.999.999"));

        assertFalse(VersionForge.estSupportee("46.0.14"), "Forge 46 vise Minecraft 1.19");
        assertFalse(VersionForge.estSupportee("48.0.0"), "Forge 48 vise Minecraft 1.20.2");
        assertFalse(VersionForge.estSupportee("50.1.0"));
    }

    @Test
    @DisplayName("T-102 : une version illisible est traitée comme non supportée (contrat 4.4)")
    void versionIllisibleTraiteeCommeLePireCas() {
        assertFalse(VersionForge.estSupportee(null));
        assertFalse(VersionForge.estSupportee(""));
        assertFalse(VersionForge.estSupportee("   "));
        assertFalse(VersionForge.estSupportee("inconnue"));
        assertFalse(VersionForge.estSupportee("v47.4.23"), "un préfixe inattendu n'est pas deviné");
        assertEquals(Optional.empty(), VersionForge.majeure("99999999999999999999"));
    }

    @Test
    @DisplayName("La plage supportée en Java correspond à loader_version_range de gradle.properties")
    void plageCoherenteAvecLeBuild() throws IOException {
        Path proprietes = Path.of("gradle.properties").toAbsolutePath();
        assertTrue(Files.isRegularFile(proprietes),
                "gradle.properties introuvable depuis " + Path.of(".").toAbsolutePath());

        String declaree = Files.readAllLines(proprietes, StandardCharsets.UTF_8).stream()
                .map(String::trim)
                .filter(l -> l.startsWith("loader_version_range="))
                .map(l -> l.substring("loader_version_range=".length()).trim())
                .findFirst()
                .orElseThrow(() -> new AssertionError("loader_version_range absent de gradle.properties"));

        // Le build déclare « [47,) » : la borne basse doit être celle du code Java.
        assertTrue(declaree.startsWith("[" + VersionForge.MAJEURE_MINIMALE),
                "loader_version_range = " + declaree + " ne commence pas par ["
                        + VersionForge.MAJEURE_MINIMALE + ". Mettre à jour VersionForge "
                        + "ou gradle.properties, jamais l'un sans l'autre.");
    }

    @Test
    @DisplayName("T-103 : une exception dans un hook est capturée et n'interrompt rien (FM-02)")
    void exceptionDansUnHookCapturee() {
        List<String> journal = new ArrayList<>();
        GardeHook garde = new GardeHook("test", journal::add);

        boolean resultat = garde.executer(() -> {
            throw new IllegalStateException("panne simulée");
        });

        assertFalse(resultat, "l'échec doit être signalé à l'appelant");
        assertEquals(1, garde.echecs());
        assertFalse(garde.desactive(), "un échec isolé ne désactive pas le hook");
        assertTrue(journal.get(0).contains("panne simulée"), journal.get(0));
    }

    @Test
    @DisplayName("T-103 : le hook est désactivé après cinq échecs (FM-02)")
    void hookDesactiveApresCinqEchecs() {
        List<String> journal = new ArrayList<>();
        GardeHook garde = new GardeHook("test", journal::add);

        for (int i = 0; i < GardeHook.SEUIL_DESACTIVATION; i++) {
            assertFalse(garde.desactive(), "désactivation prématurée au tour " + i);
            garde.executer(() -> {
                throw new IllegalStateException("panne " + System.nanoTime());
            });
        }

        assertTrue(garde.desactive());
        assertEquals(GardeHook.SEUIL_DESACTIVATION, garde.echecs());
        assertTrue(journal.get(journal.size() - 1).contains("désactivé"));

        // Une fois désactivé, le hook n'exécute plus rien.
        boolean[] execute = {false};
        assertFalse(garde.executer(() -> execute[0] = true));
        assertFalse(execute[0], "un hook désactivé ne doit plus rien exécuter");
    }

    @Test
    @DisplayName("T-103 : une erreur de liaison native est également capturée")
    void erreurDeLiaisonCapturee() {
        GardeHook garde = new GardeHook("test", message -> { });

        boolean resultat = garde.executer(() -> {
            throw new UnsatisfiedLinkError("symbole absent");
        });

        assertFalse(resultat);
        assertEquals(1, garde.echecs(), "une LinkageError doit compter comme un échec");
    }

    @Test
    @DisplayName("Un hook nominal expose ses métriques (contrat 4.1)")
    void metriquesDuHook() {
        GardeHook garde = new GardeHook("nominal", message -> { });

        assertTrue(garde.executer(() -> { }));
        assertTrue(garde.executer(() -> { }));

        assertEquals(0, garde.echecs());
        assertEquals(2, garde.appels());
        assertTrue(garde.dureeMoyenneNs() >= 0, "la durée moyenne doit être mesurée");
        assertEquals("nominal", garde.nom());
    }
}
