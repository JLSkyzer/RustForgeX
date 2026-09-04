package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.NativeLoader.LoadFailure;
import dev.rustforgex.bootstrap.NativeLoader.Platform;
import dev.rustforgex.diag.ErrorCode;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.io.ByteArrayInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests T-120 à T-123 de C-03 (Native Loader).
 *
 * <p>Les tests n'utilisent pas le binaire natif réel : ils injectent une source de
 * ressources simulée, ce qui permet de vérifier le rejet d'un binaire altéré sans
 * avoir à corrompre le vrai fichier, et rend la suite indépendante de la plateforme.
 */
class NativeLoaderTest {

    private static final Platform PLATFORM = new Platform("windows-x86_64", "rfx_native.dll");

    /** Source de ressources simulée, alimentée en mémoire. */
    private static final class FakeSource implements NativeLoader.ResourceSource {

        private final Map<String, byte[]> contents = new HashMap<>();

        void put(String path, byte[] content) {
            contents.put(path, content);
        }

        @Override
        public InputStream open(String path) {
            byte[] c = contents.get(path);
            return c == null ? null : new ByteArrayInputStream(c);
        }
    }

    /** Construit une source contenant un binaire et l'empreinte correspondante. */
    private static FakeSource validSource(byte[] binary) {
        FakeSource source = new FakeSource();
        source.put(PLATFORM.resourcePath(), binary);
        source.put(
                PLATFORM.digestPath(),
                (NativeLoader.digest(binary) + "\n").getBytes(StandardCharsets.UTF_8));
        return source;
    }

    @Test
    @DisplayName("T-120 : le binaire est extrait dans un chemin versionné par son empreinte")
    void extractionAndDigest(@TempDir Path root) throws Exception {
        byte[] binary = "contenu-natif-simule".getBytes(StandardCharsets.UTF_8);
        String expected = NativeLoader.digest(binary);

        Path extracted = new NativeLoader(validSource(binary)).prepare(PLATFORM, root);

        assertTrue(Files.isRegularFile(extracted), "le binaire doit avoir été extrait");
        assertArrayEquals(binary, Files.readAllBytes(extracted), "contenu identique à la source");
        assertEquals(PLATFORM.library(), extracted.getFileName().toString());
        // R-301 : le répertoire parent est nommé d'après le condensé.
        assertEquals(expected, extracted.getParent().getFileName().toString());
        assertTrue(extracted.startsWith(root), "l'extraction doit rester sous la racine fournie");
    }

    @Test
    @DisplayName("T-120 : une extraction déjà valide est réutilisée telle quelle")
    void extractionIsIdempotent(@TempDir Path root) throws Exception {
        byte[] binary = "contenu-natif-simule".getBytes(StandardCharsets.UTF_8);
        NativeLoader loader = new NativeLoader(validSource(binary));

        Path first = loader.prepare(PLATFORM, root);
        long timestamp = Files.getLastModifiedTime(first).toMillis();
        Path second = loader.prepare(PLATFORM, root);

        assertEquals(first, second, "le même chemin doit être réutilisé");
        assertEquals(timestamp, Files.getLastModifiedTime(second).toMillis(),
                "le fichier valide ne doit pas être réécrit");
    }

    @Test
    @DisplayName("T-121 : un binaire altéré est rejeté avant tout chargement (R-300, E-1003)")
    void tamperedBinaryIsRejected(@TempDir Path root) {
        FakeSource source = new FakeSource();
        source.put(PLATFORM.resourcePath(), "binaire-altere".getBytes(StandardCharsets.UTF_8));
        // Empreinte d'un tout autre contenu : c'est exactement le cas d'une
        // substitution de binaire.
        source.put(
                PLATFORM.digestPath(),
                (NativeLoader.digest("binaire-legitime".getBytes(StandardCharsets.UTF_8)) + "\n")
                        .getBytes(StandardCharsets.UTF_8));

        LoadFailure failure = assertThrows(LoadFailure.class,
                () -> new NativeLoader(source).prepare(PLATFORM, root));

        assertEquals(ErrorCode.INVALID_NATIVE_DIGEST, failure.code());
        assertTrue(Files.notExists(root.resolve("native")),
                "aucun fichier ne doit avoir été écrit avant la vérification");
    }

    @Test
    @DisplayName("T-121 : une empreinte illisible est rejetée")
    void unreadableDigestIsRejected(@TempDir Path root) {
        FakeSource source = new FakeSource();
        source.put(PLATFORM.resourcePath(), "peu importe".getBytes(StandardCharsets.UTF_8));
        source.put(PLATFORM.digestPath(), "pas-un-condense".getBytes(StandardCharsets.UTF_8));

        LoadFailure failure = assertThrows(LoadFailure.class,
                () -> new NativeLoader(source).prepare(PLATFORM, root));
        assertEquals(ErrorCode.INVALID_NATIVE_DIGEST, failure.code());
    }

    @Test
    @DisplayName("T-121 : un binaire absent donne E-1005 et non une erreur de hash")
    void missingBinaryIsReported(@TempDir Path root) {
        LoadFailure failure = assertThrows(LoadFailure.class,
                () -> new NativeLoader(new FakeSource()).prepare(PLATFORM, root));
        assertEquals(ErrorCode.NATIVE_MISSING, failure.code());
    }

    @Test
    @DisplayName("T-122 : une racine non inscriptible bascule sur le répertoire temporaire (R-302)")
    void fallsBackToTemporaryDirectory(@TempDir Path root) throws Exception {
        byte[] binary = "contenu-pour-repli".getBytes(StandardCharsets.UTF_8);
        // Une racine occupée par un fichier régulier rend impossible la création du
        // sous-répertoire « native » : c'est le comportement qu'oppose aussi un
        // montage en lecture seule, reproduit ici de façon portable.
        Path blocked = root.resolve("blocked");
        Files.writeString(blocked, "ceci est un fichier, pas un repertoire");

        Path extracted = new NativeLoader(validSource(binary)).prepare(PLATFORM, blocked);

        Path fallback = Path.of(System.getProperty("java.io.tmpdir")).resolve("rustforgex");
        assertTrue(extracted.toAbsolutePath().startsWith(fallback.toAbsolutePath()),
                "l'extraction aurait dû basculer sous " + fallback + ", obtenu " + extracted);
        assertArrayEquals(binary, Files.readAllBytes(extracted));

        Files.deleteIfExists(extracted);
    }

    @Test
    @DisplayName("T-122 : aucun emplacement inscriptible donne un échec propre, jamais une exception brute")
    void noWritableLocationFailsCleanly(@TempDir Path root) throws Exception {
        byte[] binary = "contenu".getBytes(StandardCharsets.UTF_8);
        Path blocked = root.resolve("blocked");
        Files.writeString(blocked, "fichier");

        // En pointant le repli sur le même chemin bloqué, plus aucun emplacement
        // n'est utilisable : l'échec doit rester contrôlé.
        String previous = System.getProperty("java.io.tmpdir");
        try {
            System.setProperty("java.io.tmpdir", blocked.toString());
            LoadFailure failure = assertThrows(LoadFailure.class,
                    () -> new NativeLoader(validSource(binary)).prepare(PLATFORM, blocked));
            assertEquals(ErrorCode.LOAD_FAILED, failure.code());
        } finally {
            System.setProperty("java.io.tmpdir", previous);
        }
    }

    @Test
    @DisplayName("T-123 : deux versions du binaire coexistent sans conflit (R-301)")
    void twoVersionsCoexist(@TempDir Path root) throws Exception {
        byte[] version1 = "binaire-version-1".getBytes(StandardCharsets.UTF_8);
        byte[] version2 = "binaire-version-2-plus-longue".getBytes(StandardCharsets.UTF_8);

        Path first = new NativeLoader(validSource(version1)).prepare(PLATFORM, root);
        Path second = new NativeLoader(validSource(version2)).prepare(PLATFORM, root);

        assertNotEquals(first, second, "chaque version doit avoir son propre chemin");
        assertTrue(Files.isRegularFile(first), "la première version doit subsister");
        assertArrayEquals(version1, Files.readAllBytes(first));
        assertArrayEquals(version2, Files.readAllBytes(second));
    }

    @Test
    @DisplayName("Les plateformes cibles du build sont reconnues, les autres refusées")
    void platformDetection() {
        assertEquals(Optional.of(new Platform("windows-x86_64", "rfx_native.dll")),
                NativeLoader.platform("Windows 11", "amd64"));
        assertEquals(Optional.of(new Platform("linux-x86_64", "librfx_native.so")),
                NativeLoader.platform("Linux", "x86_64"));
        assertEquals(Optional.of(new Platform("linux-aarch64", "librfx_native.so")),
                NativeLoader.platform("Linux", "aarch64"));

        // Aucune plateforme non prévue par le build ne doit être devinée.
        assertEquals(Optional.empty(), NativeLoader.platform("Mac OS X", "aarch64"));
        assertEquals(Optional.empty(), NativeLoader.platform("Windows 10", "x86"));
        assertEquals(Optional.empty(), NativeLoader.platform("SunOS", "sparc"));
    }

    @Test
    @DisplayName("Le condensé est celui de SHA-256, en hexadécimal minuscule")
    void digestMatchesSha256() {
        // Vecteur de test public de SHA-256 pour la chaîne vide.
        assertEquals(
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                NativeLoader.digest(new byte[0]));
    }
}
