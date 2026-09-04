package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.Bootstrap.Context;
import dev.rustforgex.bootstrap.Bootstrap.Report;
import dev.rustforgex.bootstrap.Bootstrap.State;
import dev.rustforgex.bootstrap.NativeLoader.Platform;
import dev.rustforgex.bridge.FakeNativeBridge;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.ErrorCode;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.io.ByteArrayInputStream;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Tests T-110 à T-114 de C-02 (Bootstrap). */
class BootstrapTest {

    private static final Platform PLATFORM = new Platform("windows-x86_64", "rfx_native.dll");
    private static final byte[] BINARY = "binaire-natif-simule".getBytes(StandardCharsets.UTF_8);

    /** Pont simulé : consigne les appels et rend des codes contrôlés. */
    private static class FakeBridge extends FakeNativeBridge {

        int abi = 1;
        long handleToReturn = 0x5246_5800_0000_0001L;
        int probes;
        int noopCalls;
        int jniCallNs = -1;
        int ffiBatchNsPerKb = -1;
        byte[] receivedConfig;

        @Override
        public int abiVersion() {
            return abi;
        }

        @Override
        public long init(byte[] configCbor) {
            receivedConfig = configCbor;
            return handleToReturn;
        }


        @Override
        public int noop(long handle) {
            noopCalls++;
            return 0;
        }

        @Override
        public int hwProbe(long handle) {
            probes++;
            return 0;
        }

        @Override
        public int hwSetFfiCosts(long handle, int callCost, int batchCost) {
            jniCallNs = callCost;
            ffiBatchNsPerKb = batchCost;
            return 0;
        }






    }

    /** Source de ressources simulée contenant un binaire et son empreinte. */
    private static NativeLoader.ResourceSource validSource() {
        Map<String, byte[]> contents = new HashMap<>();
        contents.put(PLATFORM.resourcePath(), BINARY);
        contents.put(
                PLATFORM.digestPath(),
                NativeLoader.digest(BINARY).getBytes(StandardCharsets.UTF_8));
        return path -> {
            byte[] c = contents.get(path);
            return c == null ? null : new ByteArrayInputStream(c);
        };
    }

    /** Source vide : le binaire natif est absent du JAR. */
    private static NativeLoader.ResourceSource emptySource() {
        return path -> (InputStream) null;
    }

    private static Context context(
            Path root,
            Configuration configuration,
            NativeLoader.ResourceSource source,
            NativeBridge bridge) {
        return new Context(
                root,
                configuration,
                false,
                Optional.of(PLATFORM),
                new NativeLoader(source),
                library -> { /* chargement simulé : rien à faire */ },
                bridge);
    }

    @BeforeEach
    void reset() {
        Bootstrap.resetForTests();
    }

    @Test
    @DisplayName("T-110 : démarrage nominal jusqu'à READY")
    void nominalStartup(@TempDir Path root) {
        FakeBridge bridge = new FakeBridge();

        Report r = Bootstrap.start(context(root, Configuration.defaults(), validSource(), bridge));

        assertEquals(State.READY, r.state(), r.message());
        assertTrue(r.ready());
        assertNull(r.code(), "un démarrage nominal ne porte aucun code d'erreur");
        assertEquals(bridge.handleToReturn, r.handle());
        assertTrue(Files.isRegularFile(r.library()), "le binaire doit avoir été extrait");

        // La séquence a bien sondé le matériel et publié des coûts mesurés (C-45).
        assertEquals(1, bridge.probes);
        assertTrue(bridge.noopCalls >= FfiCalibration.MEASURED_CALLS,
                "la calibration doit exécuter au moins " + FfiCalibration.MEASURED_CALLS + " appels");
        assertTrue(bridge.jniCallNs >= 0, "le coût d'appel doit avoir été publié");
        assertTrue(bridge.ffiBatchNsPerKb >= 0, "le coût de transfert doit avoir été publié");
        assertNotEquals(0, bridge.receivedConfig.length, "la configuration doit avoir été transmise");
    }

    @Test
    @DisplayName("T-110 : le démarrage tient dans son budget de temps")
    void startupIsFast(@TempDir Path root) {
        Report r = Bootstrap.start(
                context(root, Configuration.defaults(), validSource(), new FakeBridge()));

        assertEquals(State.READY, r.state());
        // Critère d'acceptation de C-02 : moins de 300 ms hors extraction initiale.
        // L'extraction ayant lieu ici, la marge retenue reste large.
        assertTrue(r.durationMs() < 3_000, "démarrage anormalement long : " + r.durationMs() + " ms");
    }

    @Test
    @DisplayName("T-111 : binaire natif absent, le mod passe en DEGRADED (FM-04, E-1005)")
    void missingNativeDegrades(@TempDir Path root) {
        Report r = Bootstrap.start(
                context(root, Configuration.defaults(), emptySource(), new FakeBridge()));

        assertEquals(State.DEGRADED, r.state());
        assertEquals(ErrorCode.NATIVE_MISSING, r.code());
        assertEquals(0, r.handle(), "aucun handle ne doit être publié");
    }

    @Test
    @DisplayName("T-111 : plateforme non supportée, DEGRADED avec un message explicite")
    void unsupportedPlatformDegrades(@TempDir Path root) {
        Context c = new Context(
                root,
                Configuration.defaults(),
                false,
                Optional.empty(),
                new NativeLoader(validSource()),
                library -> { },
                new FakeBridge());

        Report r = Bootstrap.start(c);

        assertEquals(State.DEGRADED, r.state());
        assertEquals(ErrorCode.NATIVE_MISSING, r.code());
        assertTrue(r.message().contains("Plateforme non supportée"), r.message());
    }

    @Test
    @DisplayName("T-112 : ABI incompatible, DISABLED sans aucun appel natif (FM-06, E-1002)")
    void incompatibleAbiDisables(@TempDir Path root) {
        FakeBridge bridge = new FakeBridge();
        bridge.abi = 2;

        Report r = Bootstrap.start(context(root, Configuration.defaults(), validSource(), bridge));

        assertEquals(State.DISABLED, r.state());
        assertEquals(ErrorCode.ABI_INCOMPATIBLE, r.code());
        assertNull(bridge.receivedConfig, "R-702 : aucun appel après un handshake refusé");
        assertEquals(0, bridge.probes);
        assertEquals(0, r.handle());
    }

    @Test
    @DisplayName("T-113 : répertoire non inscriptible, le mod reste jouable")
    void unwritableDirectoryDegrades(@TempDir Path root) throws Exception {
        // Racine occupée par un fichier : ni la racine ni le repli ne sont utilisables.
        Path blocked = root.resolve("blocked");
        Files.writeString(blocked, "fichier");

        String previousTmp = System.getProperty("java.io.tmpdir");
        try {
            System.setProperty("java.io.tmpdir", blocked.toString());
            Report r = Bootstrap.start(
                    context(blocked, Configuration.defaults(), validSource(), new FakeBridge()));

            assertEquals(State.DEGRADED, r.state());
            assertEquals(ErrorCode.LOAD_FAILED, r.code());
        } finally {
            System.setProperty("java.io.tmpdir", previousTmp);
        }
    }

    @Test
    @DisplayName("T-114 : une seconde initialisation est refusée (E-1004)")
    void doubleInitializationIsRefused(@TempDir Path root) {
        Context c = context(root, Configuration.defaults(), validSource(), new FakeBridge());

        assertEquals(State.READY, Bootstrap.start(c).state());

        Report second = Bootstrap.start(c);
        assertEquals(State.DISABLED, second.state());
        assertEquals(ErrorCode.DOUBLE_INIT, second.code());
    }

    @Test
    @DisplayName("Un binaire altéré interdit l'activation (FM-08, E-1003)")
    void tamperedBinaryDisables(@TempDir Path root) {
        Map<String, byte[]> contents = new HashMap<>();
        contents.put(PLATFORM.resourcePath(), "binaire-substitue".getBytes(StandardCharsets.UTF_8));
        contents.put(PLATFORM.digestPath(), NativeLoader.digest(BINARY).getBytes(StandardCharsets.UTF_8));
        NativeLoader.ResourceSource tampered = path -> {
            byte[] c = contents.get(path);
            return c == null ? null : new ByteArrayInputStream(c);
        };

        Report r = Bootstrap.start(context(root, Configuration.defaults(), tampered, new FakeBridge()));

        assertEquals(State.DISABLED, r.state(), "un binaire altéré ne doit jamais être activé");
        assertEquals(ErrorCode.INVALID_NATIVE_DIGEST, r.code());
    }

    @Test
    @DisplayName("enabled = false désactive tout sans toucher au natif")
    void disabledByConfiguration(@TempDir Path root) throws Exception {
        Path file = root.resolve("rustforgex.toml");
        Files.writeString(file, """
                [general]
                enabled = false
                """);
        Configuration disabled = Configuration.load(file, key -> null);
        FakeBridge bridge = new FakeBridge();

        Report r = Bootstrap.start(context(root, disabled, validSource(), bridge));

        assertEquals(State.DISABLED, r.state());
        assertNull(r.code(), "une désactivation volontaire n'est pas une erreur");
        assertNull(r.library(), "aucun binaire ne doit avoir été extrait");
        assertNull(bridge.receivedConfig, "aucun appel natif ne doit avoir eu lieu");
    }

    @Test
    @DisplayName("Une erreur inattendue dégrade au lieu de remonter jusqu'à Forge")
    void unexpectedErrorIsContained(@TempDir Path root) {
        NativeBridge faultyBridge = new FakeBridge() {
            @Override
            public int abiVersion() {
                throw new IllegalStateException("panne simulée du pont");
            }
        };

        Report r = Bootstrap.start(
                context(root, Configuration.defaults(), validSource(), faultyBridge));

        assertEquals(State.DEGRADED, r.state(), "le jeu doit rester jouable");
        assertTrue(r.message().contains("panne simulée"), r.message());
    }

    @Test
    @DisplayName("Le rapport journalise chaque étape franchie")
    void journalRecordsEveryStep(@TempDir Path root) {
        Report r = Bootstrap.start(
                context(root, Configuration.defaults(), validSource(), new FakeBridge()));

        assertFalse(r.journal().isEmpty());
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("INIT")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("PROBE")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("LOAD_NATIVE")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("HANDSHAKE")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("CONFIGURE")));
        assertTrue(r.journal().get(r.journal().size() - 1).startsWith("READY"));
    }
}
