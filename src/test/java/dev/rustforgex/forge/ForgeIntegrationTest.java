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
class ForgeIntegrationTest {

    @Test
    @DisplayName("T-102 : une version de Forge hors plage n'est pas supportée (FM-01)")
    void versionsOutsideRangeAreRefused() {
        assertTrue(ForgeVersions.isSupported("47.4.23"));
        assertTrue(ForgeVersions.isSupported("47.0.0"));
        assertTrue(ForgeVersions.isSupported("47.999.999"));

        assertFalse(ForgeVersions.isSupported("46.0.14"), "Forge 46 vise Minecraft 1.19");
        assertFalse(ForgeVersions.isSupported("48.0.0"), "Forge 48 vise Minecraft 1.20.2");
        assertFalse(ForgeVersions.isSupported("50.1.0"));
    }

    @Test
    @DisplayName("T-102 : une version illisible est traitée comme non supportée (contrat 4.4)")
    void unreadableVersionIsTreatedAsWorstCase() {
        assertFalse(ForgeVersions.isSupported(null));
        assertFalse(ForgeVersions.isSupported(""));
        assertFalse(ForgeVersions.isSupported("   "));
        assertFalse(ForgeVersions.isSupported("inconnue"));
        assertFalse(ForgeVersions.isSupported("v47.4.23"), "un préfixe inattendu n'est pas deviné");
        assertEquals(Optional.empty(), ForgeVersions.major("99999999999999999999"));
    }

    @Test
    @DisplayName("La plage supportée en Java correspond à loader_version_range de gradle.properties")
    void rangeMatchesTheBuild() throws IOException {
        Path properties = Path.of("gradle.properties").toAbsolutePath();
        assertTrue(Files.isRegularFile(properties),
                "gradle.properties introuvable depuis " + Path.of(".").toAbsolutePath());

        String declared = Files.readAllLines(properties, StandardCharsets.UTF_8).stream()
                .map(String::trim)
                .filter(l -> l.startsWith("loader_version_range="))
                .map(l -> l.substring("loader_version_range=".length()).trim())
                .findFirst()
                .orElseThrow(() -> new AssertionError("loader_version_range absent de gradle.properties"));

        // Le build déclare « [47,) » : la borne basse doit être celle du code Java.
        assertTrue(declared.startsWith("[" + ForgeVersions.MINIMUM_MAJOR),
                "loader_version_range = " + declared + " ne commence pas par ["
                        + ForgeVersions.MINIMUM_MAJOR + ". Mettre à jour ForgeVersions "
                        + "ou gradle.properties, jamais l'un sans l'autre.");
    }

    @Test
    @DisplayName("T-103 : une exception dans un hook est capturée et n'interrompt rien (FM-02)")
    void exceptionInHookIsCaught() {
        List<String> log = new ArrayList<>();
        HookGuard guard = new HookGuard("test", log::add);

        boolean result = guard.run(() -> {
            throw new IllegalStateException("panne simulée");
        });

        assertFalse(result, "l'échec doit être signalé à l'appelant");
        assertEquals(1, guard.failures());
        assertFalse(guard.disabled(), "un échec isolé ne désactive pas le hook");
        assertTrue(log.get(0).contains("panne simulée"), log.get(0));
    }

    @Test
    @DisplayName("T-103 : le hook est désactivé après cinq échecs (FM-02)")
    void hookIsDisabledAfterFiveFailures() {
        List<String> log = new ArrayList<>();
        HookGuard guard = new HookGuard("test", log::add);

        for (int i = 0; i < HookGuard.DISABLE_THRESHOLD; i++) {
            assertFalse(guard.disabled(), "désactivation prématurée au tour " + i);
            guard.run(() -> {
                throw new IllegalStateException("panne " + System.nanoTime());
            });
        }

        assertTrue(guard.disabled());
        assertEquals(HookGuard.DISABLE_THRESHOLD, guard.failures());
        assertTrue(log.get(log.size() - 1).contains("désactivé"));

        // Une fois désactivé, le hook n'exécute plus rien.
        boolean[] executed = {false};
        assertFalse(guard.run(() -> executed[0] = true));
        assertFalse(executed[0], "un hook désactivé ne doit plus rien exécuter");
    }

    @Test
    @DisplayName("T-103 : une erreur de liaison native est également capturée")
    void linkageErrorIsCaught() {
        HookGuard guard = new HookGuard("test", message -> { });

        boolean result = guard.run(() -> {
            throw new UnsatisfiedLinkError("symbole absent");
        });

        assertFalse(result);
        assertEquals(1, guard.failures(), "une LinkageError doit compter comme un échec");
    }

    @Test
    @DisplayName("Un hook nominal expose ses métriques (contrat 4.1)")
    void hookExposesItsMetrics() {
        HookGuard guard = new HookGuard("nominal", message -> { });

        assertTrue(guard.run(() -> { }));
        assertTrue(guard.run(() -> { }));

        assertEquals(0, guard.failures());
        assertEquals(2, guard.calls());
        assertTrue(guard.averageDurationNs() >= 0, "la durée moyenne doit être mesurée");
        assertEquals("nominal", guard.name());
    }
}
