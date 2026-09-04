package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.Bootstrap.Context;
import dev.rustforgex.bootstrap.Bootstrap.State;
import dev.rustforgex.bootstrap.Bootstrap.Report;
import dev.rustforgex.bootstrap.NativeLoader.Platform;
import dev.rustforgex.bridge.CborReader;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.instrument.ProbeSink;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.ErrorCode;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.ByteBuffer;
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
class NativeIntegrationTest {

    @BeforeEach
    void reinitialiser() {
        Bootstrap.resetForTests();
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
    private static Path extractionRoot() throws java.io.IOException {
        Path root = Path.of("build", "test-native").toAbsolutePath();
        Files.createDirectories(root);
        return root;
    }

    @Test
    @DisplayName("T-110 / T-004 : cycle complet avec le binaire natif réel")
    void fullCycleWithTheRealBinary() throws Exception {
        Path root = extractionRoot();
        Optional<Platform> platform = NativeLoader.hostPlatform();
        Assumptions.assumeTrue(platform.isPresent(), "plateforme hôte sans binaire natif prévu");
        Assumptions.assumeTrue(
                NativeLoader.class.getResource(platform.get().resourcePath()) != null,
                "binaire natif absent des ressources : construire avec ./gradlew build");

        Configuration configuration = debugModeConfiguration(root);
        Context context = new Context(
                root,
                configuration,
                false,
                platform,
                new NativeLoader(),
                NativeLoader::load,
                NativeBridge.real());

        // --- Démarrage complet -------------------------------------------------
        Report report = Bootstrap.start(context);
        assertEquals(State.READY, report.state(), report.message());
        assertTrue(report.handle() > 0, "le handle doit être positif");
        assertTrue(Files.isRegularFile(report.library()));

        NativeBridge bridge = NativeBridge.real();
        long handle = report.handle();

        try {
            // --- L'ABI annoncée est bien celle attendue (R-702) ----------------
            assertEquals(dev.rustforgex.bridge.RfxNative.EXPECTED_ABI, bridge.abiVersion());

            // --- Le statut se décode et reflète la configuration transmise ------
            Map<String, Object> status = status(bridge, handle);
            assertEquals(1L, status.get("abi_version"));
            assertEquals("RUNNING", status.get("state"));
            assertEquals(0L, status.get("panics"));

            @SuppressWarnings("unchecked")
            Map<String, Object> hardware = (Map<String, Object>) status.get("hardware");
            assertNotNull(hardware, "la classe matérielle doit être publiée");
            assertTrue((Long) hardware.get("logical_cores") > 0, "cœurs logiques mesurés");
            assertTrue((Long) hardware.get("physical_cores") > 0, "cœurs physiques mesurés");
            assertTrue((Long) hardware.get("mem_total_bytes") > 0, "mémoire totale mesurée");

            // R-660 : les coûts publiés viennent d'une mesure, jamais d'une constante.
            assertTrue((Long) hardware.get("jni_call_ns") > 0,
                    "le coût d'un aller-retour FFI doit avoir été mesuré");

            @SuppressWarnings("unchecked")
            Map<String, Object> coverage = (Map<String, Object>) status.get("probe_coverage");
            assertTrue((Long) hardware.get("ffi_batch_ns_per_kb") > 0,
                    "le coût de transfert doit avoir été mesuré");

            assertEquals(Boolean.TRUE, coverage.get("cores"));
            assertEquals(Boolean.TRUE, coverage.get("mem_total"));
            assertEquals(Boolean.TRUE, coverage.get("ffi_call"));
            // Cette couverture n'était pas vérifiée à l'origine, et un témoin de
            // transfert négatif — une somme dépassant i64::MAX relue comme un code
            // d'erreur — passait donc inaperçu jusqu'à l'exécution dans le jeu.
            assertEquals(Boolean.TRUE, coverage.get("ffi_transfer"));

            // --- T-360 : une panic native ne traverse pas la frontière ----------
            // La configuration est en mode debug, `/rfx panic-test` est donc autorisée.
            int panicCode = bridge.panicTest(handle);
            assertEquals(-ErrorCode.PANIC_CAUGHT.number(), panicCode,
                    "la panic doit être capturée et rendue sous forme de code E-3001");

            Map<String, Object> afterPanic = status(bridge, handle);
            assertEquals(1L, afterPanic.get("panics"), "la panic doit être comptabilisée");
            assertEquals("RUNNING", afterPanic.get("state"),
                    "une panic isolée ne doit pas arrêter le runtime");

            // --- R-521 : un handle inconnu est rejeté ---------------------------
            assertTrue(bridge.noop(handle ^ 0xbeef) < 0, "un handle falsifié doit être rejeté");
            assertNull(bridge.status(handle ^ 0xbeef), "aucun statut pour un handle inconnu");

            // --- IF-03 : le flux de profilage traverse réellement la frontière ---
            // Un DirectByteBuffer pointant sur la mémoire du natif est exactement le
            // genre de chose qui marche en simulation et casse en vrai : rien ne
            // remplace un aller-retour avec le vrai binaire.
            ByteBuffer probeBuffer = bridge.probeBufferAcquire(handle, 0);
            assertNotNull(probeBuffer, "le natif doit fournir un tampon de sondes");
            assertTrue(probeBuffer.isDirect(), "le tampon doit être direct (ADR-004)");
            assertEquals(64 * 1024, probeBuffer.capacity(), "64 Kio par thread (PARTIE 5.5)");

            ProbeSink sink = new ProbeSink(bridge, handle);
            for (int i = 0; i < 10; i++) {
                assertTrue(sink.record(i, ProbeSink.KIND_ENTER, (short) i, 1_000L + i, i * 7L),
                        "enregistrement " + i);
            }
            assertEquals(10 * ProbeSink.RECORD_SIZE, sink.flush());

            Map<String, Object> afterProbes = status(bridge, handle);
            @SuppressWarnings("unchecked")
            Map<String, Object> probes = (Map<String, Object>) afterProbes.get("probes");
            assertNotNull(probes, "les compteurs de profilage doivent être publiés");
            assertEquals(10L, probes.get("records_consumed"),
                    "le natif doit avoir décodé les dix enregistrements écrits par Java");
            assertEquals(0L, probes.get("records_lost"));
            assertTrue((Long) probes.get("native_bytes") >= 64 * 1024,
                    "le tampon doit être compté dans le budget mémoire natif");

            // --- R-520 : la seconde initialisation est refusée -------------------
            long second = bridge.init(configuration.toNativeCbor());
            assertEquals(-ErrorCode.DOUBLE_INIT.number(), second,
                    "la double initialisation doit être refusée avec E-1004");
        } finally {
            assertEquals(0, bridge.shutdown(handle), "l'arrêt doit réussir");
        }

        // Après l'arrêt, le handle n'est plus valide.
        assertTrue(bridge.noop(handle) < 0, "un handle libéré ne doit jamais être revalidé");
    }

    /** Lit et décode le blob de statut publié par le runtime natif. */
    @SuppressWarnings("unchecked")
    private static Map<String, Object> status(NativeBridge bridge, long handle) throws CborReader.InvalidCbor {
        byte[] blob = bridge.status(handle);
        assertNotNull(blob, "le statut doit être disponible");
        Object value = CborReader.decode(blob);
        assertTrue(value instanceof Map, "le statut doit être une table");
        return (Map<String, Object>) value;
    }

    /** Configuration en mode debug, seul mode où {@code panic-test} est autorisé. */
    private static Configuration debugModeConfiguration(Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [general]
                mode = "debug"
                """, StandardCharsets.UTF_8);
        Configuration configuration = Configuration.load(file, key -> null);
        assertEquals("debug", configuration.mode());
        return configuration;
    }
}
