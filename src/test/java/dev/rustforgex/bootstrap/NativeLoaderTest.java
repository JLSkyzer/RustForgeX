package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.NativeLoader.EchecChargement;
import dev.rustforgex.bootstrap.NativeLoader.Plateforme;
import dev.rustforgex.diag.CodeErreur;
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

    private static final Plateforme PLATEFORME = new Plateforme("windows-x86_64", "rfx_native.dll");

    /** Source de ressources simulée, alimentée en mémoire. */
    private static final class SourceSimulee implements NativeLoader.SourceRessources {

        private final Map<String, byte[]> contenus = new HashMap<>();

        void ajouter(String chemin, byte[] contenu) {
            contenus.put(chemin, contenu);
        }

        @Override
        public InputStream ouvrir(String chemin) {
            byte[] c = contenus.get(chemin);
            return c == null ? null : new ByteArrayInputStream(c);
        }
    }

    /** Construit une source contenant un binaire et l'empreinte correspondante. */
    private static SourceSimulee sourceValide(byte[] binaire) {
        SourceSimulee source = new SourceSimulee();
        source.ajouter(PLATEFORME.cheminRessource(), binaire);
        source.ajouter(
                PLATEFORME.cheminEmpreinte(),
                (NativeLoader.condense(binaire) + "\n").getBytes(StandardCharsets.UTF_8));
        return source;
    }

    @Test
    @DisplayName("T-120 : le binaire est extrait dans un chemin versionné par son empreinte")
    void extractionEtEmpreinte(@TempDir Path racine) throws Exception {
        byte[] binaire = "contenu-natif-simule".getBytes(StandardCharsets.UTF_8);
        String empreinte = NativeLoader.condense(binaire);

        Path extrait = new NativeLoader(sourceValide(binaire)).preparer(PLATEFORME, racine);

        assertTrue(Files.isRegularFile(extrait), "le binaire doit avoir été extrait");
        assertArrayEquals(binaire, Files.readAllBytes(extrait), "contenu identique à la source");
        assertEquals(PLATEFORME.bibliotheque(), extrait.getFileName().toString());
        // R-301 : le répertoire parent est nommé d'après le condensé.
        assertEquals(empreinte, extrait.getParent().getFileName().toString());
        assertTrue(extrait.startsWith(racine), "l'extraction doit rester sous la racine fournie");
    }

    @Test
    @DisplayName("T-120 : une extraction déjà valide est réutilisée telle quelle")
    void extractionIdempotente(@TempDir Path racine) throws Exception {
        byte[] binaire = "contenu-natif-simule".getBytes(StandardCharsets.UTF_8);
        NativeLoader loader = new NativeLoader(sourceValide(binaire));

        Path premier = loader.preparer(PLATEFORME, racine);
        long horodatage = Files.getLastModifiedTime(premier).toMillis();
        Path second = loader.preparer(PLATEFORME, racine);

        assertEquals(premier, second, "le même chemin doit être réutilisé");
        assertEquals(horodatage, Files.getLastModifiedTime(second).toMillis(),
                "le fichier valide ne doit pas être réécrit");
    }

    @Test
    @DisplayName("T-121 : un binaire altéré est rejeté avant tout chargement (R-300, E-1003)")
    void binaireAltereRejete(@TempDir Path racine) {
        SourceSimulee source = new SourceSimulee();
        source.ajouter(PLATEFORME.cheminRessource(), "binaire-altere".getBytes(StandardCharsets.UTF_8));
        // Empreinte d'un tout autre contenu : c'est exactement le cas d'une
        // substitution de binaire.
        source.ajouter(
                PLATEFORME.cheminEmpreinte(),
                (NativeLoader.condense("binaire-legitime".getBytes(StandardCharsets.UTF_8)) + "\n")
                        .getBytes(StandardCharsets.UTF_8));

        EchecChargement echec = assertThrows(EchecChargement.class,
                () -> new NativeLoader(source).preparer(PLATEFORME, racine));

        assertEquals(CodeErreur.HASH_NATIF_INVALIDE, echec.code());
        assertTrue(Files.notExists(racine.resolve("native")),
                "aucun fichier ne doit avoir été écrit avant la vérification");
    }

    @Test
    @DisplayName("T-121 : une empreinte illisible est rejetée")
    void empreinteIllisibleRejetee(@TempDir Path racine) {
        SourceSimulee source = new SourceSimulee();
        source.ajouter(PLATEFORME.cheminRessource(), "peu importe".getBytes(StandardCharsets.UTF_8));
        source.ajouter(PLATEFORME.cheminEmpreinte(), "pas-un-condense".getBytes(StandardCharsets.UTF_8));

        EchecChargement echec = assertThrows(EchecChargement.class,
                () -> new NativeLoader(source).preparer(PLATEFORME, racine));
        assertEquals(CodeErreur.HASH_NATIF_INVALIDE, echec.code());
    }

    @Test
    @DisplayName("T-121 : un binaire absent donne E-1005 et non une erreur de hash")
    void binaireAbsentSignale(@TempDir Path racine) {
        EchecChargement echec = assertThrows(EchecChargement.class,
                () -> new NativeLoader(new SourceSimulee()).preparer(PLATEFORME, racine));
        assertEquals(CodeErreur.NATIF_ABSENT, echec.code());
    }

    @Test
    @DisplayName("T-122 : une racine non inscriptible bascule sur le répertoire temporaire (R-302)")
    void repliSurLeRepertoireTemporaire(@TempDir Path racine) throws Exception {
        byte[] binaire = "contenu-pour-repli".getBytes(StandardCharsets.UTF_8);
        // Une racine occupée par un fichier régulier rend impossible la création du
        // sous-répertoire « native » : c'est le comportement qu'oppose aussi un
        // montage en lecture seule, reproduit ici de façon portable.
        Path racineBloquee = racine.resolve("bloquee");
        Files.writeString(racineBloquee, "ceci est un fichier, pas un repertoire");

        Path extrait = new NativeLoader(sourceValide(binaire)).preparer(PLATEFORME, racineBloquee);

        Path repli = Path.of(System.getProperty("java.io.tmpdir")).resolve("rustforgex");
        assertTrue(extrait.toAbsolutePath().startsWith(repli.toAbsolutePath()),
                "l'extraction aurait dû basculer sous " + repli + ", obtenu " + extrait);
        assertArrayEquals(binaire, Files.readAllBytes(extrait));

        Files.deleteIfExists(extrait);
    }

    @Test
    @DisplayName("T-122 : aucun emplacement inscriptible donne un échec propre, jamais une exception brute")
    void aucunEmplacementInscriptible(@TempDir Path racine) throws Exception {
        byte[] binaire = "contenu".getBytes(StandardCharsets.UTF_8);
        Path bloquee = racine.resolve("bloquee");
        Files.writeString(bloquee, "fichier");

        // En pointant le repli sur le même chemin bloqué, plus aucun emplacement
        // n'est utilisable : l'échec doit rester contrôlé.
        String ancien = System.getProperty("java.io.tmpdir");
        try {
            System.setProperty("java.io.tmpdir", bloquee.toString());
            EchecChargement echec = assertThrows(EchecChargement.class,
                    () -> new NativeLoader(sourceValide(binaire)).preparer(PLATEFORME, bloquee));
            assertEquals(CodeErreur.CHARGEMENT_ECHOUE, echec.code());
        } finally {
            System.setProperty("java.io.tmpdir", ancien);
        }
    }

    @Test
    @DisplayName("T-123 : deux versions du binaire coexistent sans conflit (R-301)")
    void deuxVersionsCoexistent(@TempDir Path racine) throws Exception {
        byte[] version1 = "binaire-version-1".getBytes(StandardCharsets.UTF_8);
        byte[] version2 = "binaire-version-2-plus-longue".getBytes(StandardCharsets.UTF_8);

        Path extrait1 = new NativeLoader(sourceValide(version1)).preparer(PLATEFORME, racine);
        Path extrait2 = new NativeLoader(sourceValide(version2)).preparer(PLATEFORME, racine);

        assertNotEquals(extrait1, extrait2, "chaque version doit avoir son propre chemin");
        assertTrue(Files.isRegularFile(extrait1), "la première version doit subsister");
        assertArrayEquals(version1, Files.readAllBytes(extrait1));
        assertArrayEquals(version2, Files.readAllBytes(extrait2));
    }

    @Test
    @DisplayName("Les plateformes cibles du build sont reconnues, les autres refusées")
    void detectionDesPlateformes() {
        assertEquals(Optional.of(new Plateforme("windows-x86_64", "rfx_native.dll")),
                NativeLoader.plateforme("Windows 11", "amd64"));
        assertEquals(Optional.of(new Plateforme("linux-x86_64", "librfx_native.so")),
                NativeLoader.plateforme("Linux", "x86_64"));
        assertEquals(Optional.of(new Plateforme("linux-aarch64", "librfx_native.so")),
                NativeLoader.plateforme("Linux", "aarch64"));

        // Aucune plateforme non prévue par le build ne doit être devinée.
        assertEquals(Optional.empty(), NativeLoader.plateforme("Mac OS X", "aarch64"));
        assertEquals(Optional.empty(), NativeLoader.plateforme("Windows 10", "x86"));
        assertEquals(Optional.empty(), NativeLoader.plateforme("SunOS", "sparc"));
    }

    @Test
    @DisplayName("Le condensé est celui de SHA-256, en hexadécimal minuscule")
    void condenseConformeASha256() {
        // Vecteur de test public de SHA-256 pour la chaîne vide.
        assertEquals(
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                NativeLoader.condense(new byte[0]));
    }
}
