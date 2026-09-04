package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.Bootstrap.Contexte;
import dev.rustforgex.bootstrap.Bootstrap.Etat;
import dev.rustforgex.bootstrap.Bootstrap.Rapport;
import dev.rustforgex.bootstrap.NativeLoader.Plateforme;
import dev.rustforgex.bridge.CborLecteur;
import dev.rustforgex.bridge.PontNatif;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.CodeErreur;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Optional;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Test d'intégration T-110 : la chaîne complète avec le binaire natif réellement
 * construit.
 *
 * <p>Contrairement à {@link BootstrapTest}, qui simule le pont, ce test exerce le
 * parcours entier — extraction, vérification SHA-256, {@code System.load}, handshake
 * ABI, encodage CBOR de la configuration, initialisation, sonde matérielle,
 * calibration, lecture du statut, arrêt. C'est le seul test qui prouve que les
 * représentations Java et Rust du modèle concordent réellement (T-004).
 *
 * <p>Le runtime natif n'admet qu'une instance par processus (R-520) et une
 * bibliothèque ne se charge qu'une fois par JVM : tout le parcours tient donc dans une
 * seule méthode, par nécessité et non par commodité.
 *
 * <p>Si le binaire natif n'a pas été construit — cargo absent, cible indisponible —
 * le test est ignoré plutôt que mis en échec : le cahier des charges prévoit
 * explicitement qu'un JAR puisse être produit sans natif (PARTIE 23.5).
 */
class IntegrationNativeTest {

    @BeforeEach
    void reinitialiser() {
        Bootstrap.reinitialiserPourTests();
    }

    /**
     * Racine d'extraction dediee, hors du repertoire temporaire de JUnit.
     *
     * <p>Une bibliotheque chargee par {@code System.load} reste verrouillee par le
     * systeme jusqu'a la fin du processus sous Windows : JUnit ne pourrait pas
     * supprimer un {@code @TempDir} la contenant, et le test echouerait au nettoyage
     * alors que le parcours teste a reussi. Ce repertoire vit donc sous {@code build/},
     * ou il est supprime par {@code ./gradlew clean}.
     */
    private static Path racineExtraction() throws java.io.IOException {
        Path racine = Path.of("build", "test-native").toAbsolutePath();
        Files.createDirectories(racine);
        return racine;
    }

    @Test
    @DisplayName("T-110 / T-004 : cycle complet avec le binaire natif réel")
    void cycleCompletAvecLeBinaireReel() throws Exception {
        Path racine = racineExtraction();
        Optional<Plateforme> plateforme = NativeLoader.plateformeHote();
        Assumptions.assumeTrue(plateforme.isPresent(), "plateforme hôte sans binaire natif prévu");
        Assumptions.assumeTrue(
                NativeLoader.class.getResource(plateforme.get().cheminRessource()) != null,
                "binaire natif absent des ressources : construire avec ./gradlew build");

        Configuration configuration = configurationEnModeDebug(racine);
        Contexte contexte = new Contexte(
                racine,
                configuration,
                false,
                plateforme,
                new NativeLoader(),
                NativeLoader::charger,
                PontNatif.reel());

        // --- Démarrage complet -------------------------------------------------
        Rapport rapport = Bootstrap.demarrer(contexte);
        assertEquals(Etat.READY, rapport.etat(), rapport.message());
        assertTrue(rapport.handle() > 0, "le handle doit être positif");
        assertTrue(Files.isRegularFile(rapport.bibliotheque()));

        PontNatif pont = PontNatif.reel();
        long handle = rapport.handle();

        try {
            // --- L'ABI annoncée est bien celle attendue (R-702) ----------------
            assertEquals(dev.rustforgex.bridge.RfxNative.ABI_ATTENDUE, pont.abiVersion());

            // --- Le statut se décode et reflète la configuration transmise ------
            Map<String, Object> statut = statut(pont, handle);
            assertEquals(1L, statut.get("abi_version"));
            assertEquals("RUNNING", statut.get("etat"));
            assertEquals(0L, statut.get("panics"));

            @SuppressWarnings("unchecked")
            Map<String, Object> materiel = (Map<String, Object>) statut.get("materiel");
            assertNotNull(materiel, "la classe matérielle doit être publiée");
            assertTrue((Long) materiel.get("logical_cores") > 0, "cœurs logiques mesurés");
            assertTrue((Long) materiel.get("physical_cores") > 0, "cœurs physiques mesurés");
            assertTrue((Long) materiel.get("mem_total_bytes") > 0, "mémoire totale mesurée");

            // R-660 : les coûts publiés viennent d'une mesure, jamais d'une constante.
            assertTrue((Long) materiel.get("jni_call_ns") > 0,
                    "le coût d'un aller-retour FFI doit avoir été mesuré");

            @SuppressWarnings("unchecked")
            Map<String, Object> couverture = (Map<String, Object>) statut.get("couverture_sonde");
            assertEquals(Boolean.TRUE, couverture.get("cores"));
            assertEquals(Boolean.TRUE, couverture.get("mem_total"));
            assertEquals(Boolean.TRUE, couverture.get("ffi_call"));

            // --- T-360 : une panic native ne traverse pas la frontière ----------
            // La configuration est en mode debug, `/rfx panic-test` est donc autorisée.
            int codePanic = pont.panicTest(handle);
            assertEquals(-CodeErreur.PANIC_CAPTUREE.numero(), codePanic,
                    "la panic doit être capturée et rendue sous forme de code E-3001");

            Map<String, Object> apresPanic = statut(pont, handle);
            assertEquals(1L, apresPanic.get("panics"), "la panic doit être comptabilisée");
            assertEquals("RUNNING", apresPanic.get("etat"),
                    "une panic isolée ne doit pas arrêter le runtime");

            // --- R-521 : un handle inconnu est rejeté ---------------------------
            assertTrue(pont.noop(handle ^ 0xbeef) < 0, "un handle falsifié doit être rejeté");
            assertNull(pont.status(handle ^ 0xbeef), "aucun statut pour un handle inconnu");

            // --- R-520 : la seconde initialisation est refusée -------------------
            long second = pont.init(configuration.versCborNatif());
            assertEquals(-CodeErreur.DOUBLE_INIT.numero(), second,
                    "la double initialisation doit être refusée avec E-1004");
        } finally {
            assertEquals(0, pont.shutdown(handle), "l'arrêt doit réussir");
        }

        // Après l'arrêt, le handle n'est plus valide.
        assertTrue(pont.noop(handle) < 0, "un handle libéré ne doit jamais être revalidé");
    }

    /** Lit et décode le blob de statut publié par le runtime natif. */
    @SuppressWarnings("unchecked")
    private static Map<String, Object> statut(PontNatif pont, long handle) throws CborLecteur.CborInvalide {
        byte[] blob = pont.status(handle);
        assertNotNull(blob, "le statut doit être disponible");
        Object valeur = CborLecteur.decoder(blob);
        assertTrue(valeur instanceof Map, "le statut doit être une table");
        return (Map<String, Object>) valeur;
    }

    /** Configuration en mode debug, seul mode où {@code panic-test} est autorisé. */
    private static Configuration configurationEnModeDebug(Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [general]
                mode = "debug"
                """, StandardCharsets.UTF_8);
        Configuration configuration = Configuration.charger(fichier, cle -> null);
        assertEquals("debug", configuration.mode());
        return configuration;
    }
}
