package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.Bootstrap.Contexte;
import dev.rustforgex.bootstrap.Bootstrap.Etat;
import dev.rustforgex.bootstrap.Bootstrap.Rapport;
import dev.rustforgex.bootstrap.NativeLoader.Plateforme;
import dev.rustforgex.bridge.PontNatif;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.CodeErreur;
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

    private static final Plateforme PLATEFORME = new Plateforme("windows-x86_64", "rfx_native.dll");
    private static final byte[] BINAIRE = "binaire-natif-simule".getBytes(StandardCharsets.UTF_8);

    /** Pont simulé : consigne les appels et rend des codes contrôlés. */
    private static class PontSimule implements PontNatif {

        int abi = 1;
        long handleAttribue = 0x5246_5800_0000_0001L;
        int sondes;
        int appelsNoop;
        int jniCallNs = -1;
        int ffiBatchNsPerKb = -1;
        byte[] configRecue;

        @Override
        public int abiVersion() {
            return abi;
        }

        @Override
        public long init(byte[] configCbor) {
            configRecue = configCbor;
            return handleAttribue;
        }

        @Override
        public int shutdown(long handle) {
            return 0;
        }

        @Override
        public int noop(long handle) {
            appelsNoop++;
            return 0;
        }

        @Override
        public int hwProbe(long handle) {
            sondes++;
            return 0;
        }

        @Override
        public int hwSetFfiCosts(long handle, int appel, int lot) {
            jniCallNs = appel;
            ffiBatchNsPerKb = lot;
            return 0;
        }

        @Override
        public long transferProbe(long handle, ByteBuffer tampon, int longueur) {
            return longueur;
        }

        @Override
        public byte[] status(long handle) {
            return new byte[0];
        }

        @Override
        public int panicTest(long handle) {
            return -3001;
        }
    }

    /** Source de ressources simulée contenant un binaire et son empreinte. */
    private static NativeLoader.SourceRessources sourceValide() {
        Map<String, byte[]> contenus = new HashMap<>();
        contenus.put(PLATEFORME.cheminRessource(), BINAIRE);
        contenus.put(
                PLATEFORME.cheminEmpreinte(),
                NativeLoader.condense(BINAIRE).getBytes(StandardCharsets.UTF_8));
        return chemin -> {
            byte[] c = contenus.get(chemin);
            return c == null ? null : new ByteArrayInputStream(c);
        };
    }

    /** Source vide : le binaire natif est absent du JAR. */
    private static NativeLoader.SourceRessources sourceVide() {
        return chemin -> (InputStream) null;
    }

    private static Contexte contexte(
            Path racine,
            Configuration configuration,
            NativeLoader.SourceRessources source,
            PontNatif pont) {
        return new Contexte(
                racine,
                configuration,
                false,
                Optional.of(PLATEFORME),
                new NativeLoader(source),
                bibliotheque -> { /* chargement simulé : rien à faire */ },
                pont);
    }

    @BeforeEach
    void reinitialiser() {
        Bootstrap.reinitialiserPourTests();
    }

    @Test
    @DisplayName("T-110 : démarrage nominal jusqu'à READY")
    void demarrageNominal(@TempDir Path racine) {
        PontSimule pont = new PontSimule();

        Rapport r = Bootstrap.demarrer(
                contexte(racine, Configuration.parDefaut(), sourceValide(), pont));

        assertEquals(Etat.READY, r.etat(), r.message());
        assertTrue(r.pret());
        assertNull(r.code(), "un démarrage nominal ne porte aucun code d'erreur");
        assertEquals(pont.handleAttribue, r.handle());
        assertTrue(Files.isRegularFile(r.bibliotheque()), "le binaire doit avoir été extrait");

        // La séquence a bien sondé le matériel et publié des coûts mesurés (C-45).
        assertEquals(1, pont.sondes);
        assertTrue(pont.appelsNoop >= CalibrationFfi.APPELS_MESURES,
                "la calibration doit exécuter au moins " + CalibrationFfi.APPELS_MESURES + " appels");
        assertTrue(pont.jniCallNs >= 0, "le coût d'appel doit avoir été publié");
        assertTrue(pont.ffiBatchNsPerKb >= 0, "le coût de transfert doit avoir été publié");
        assertNotEquals(0, pont.configRecue.length, "la configuration doit avoir été transmise");
    }

    @Test
    @DisplayName("T-110 : le démarrage tient dans son budget de temps")
    void demarrageRapide(@TempDir Path racine) {
        Rapport r = Bootstrap.demarrer(
                contexte(racine, Configuration.parDefaut(), sourceValide(), new PontSimule()));

        assertEquals(Etat.READY, r.etat());
        // Critère d'acceptation de C-02 : moins de 300 ms hors extraction initiale.
        // L'extraction ayant lieu ici, la marge retenue reste large.
        assertTrue(r.dureeMs() < 3_000,
                "démarrage anormalement long : " + r.dureeMs() + " ms");
    }

    @Test
    @DisplayName("T-111 : binaire natif absent, le mod passe en DEGRADED (FM-04, E-1005)")
    void natifAbsentDegrade(@TempDir Path racine) {
        Rapport r = Bootstrap.demarrer(
                contexte(racine, Configuration.parDefaut(), sourceVide(), new PontSimule()));

        assertEquals(Etat.DEGRADED, r.etat());
        assertEquals(CodeErreur.NATIF_ABSENT, r.code());
        assertEquals(0, r.handle(), "aucun handle ne doit être publié");
    }

    @Test
    @DisplayName("T-111 : plateforme non supportée, DEGRADED avec un message explicite")
    void plateformeNonSupporteeDegrade(@TempDir Path racine) {
        Contexte c = new Contexte(
                racine,
                Configuration.parDefaut(),
                false,
                Optional.empty(),
                new NativeLoader(sourceValide()),
                bibliotheque -> { },
                new PontSimule());

        Rapport r = Bootstrap.demarrer(c);

        assertEquals(Etat.DEGRADED, r.etat());
        assertEquals(CodeErreur.NATIF_ABSENT, r.code());
        assertTrue(r.message().contains("Plateforme non supportée"), r.message());
    }

    @Test
    @DisplayName("T-112 : ABI incompatible, le mod passe en DISABLED sans aucun appel natif (FM-06, E-1002)")
    void abiIncompatibleDesactive(@TempDir Path racine) {
        PontSimule pont = new PontSimule();
        pont.abi = 2;

        Rapport r = Bootstrap.demarrer(
                contexte(racine, Configuration.parDefaut(), sourceValide(), pont));

        assertEquals(Etat.DISABLED, r.etat());
        assertEquals(CodeErreur.ABI_INCOMPATIBLE, r.code());
        assertNull(pont.configRecue, "R-702 : aucun appel après un handshake refusé");
        assertEquals(0, pont.sondes);
        assertEquals(0, r.handle());
    }

    @Test
    @DisplayName("T-113 : répertoire non inscriptible, le mod reste jouable")
    void repertoireNonInscriptibleDegrade(@TempDir Path racine) throws Exception {
        // Racine occupée par un fichier : ni la racine ni le repli ne sont utilisables.
        Path bloquee = racine.resolve("bloquee");
        Files.writeString(bloquee, "fichier");

        String ancienTmp = System.getProperty("java.io.tmpdir");
        try {
            System.setProperty("java.io.tmpdir", bloquee.toString());
            Rapport r = Bootstrap.demarrer(
                    contexte(bloquee, Configuration.parDefaut(), sourceValide(), new PontSimule()));

            assertEquals(Etat.DEGRADED, r.etat());
            assertEquals(CodeErreur.CHARGEMENT_ECHOUE, r.code());
        } finally {
            System.setProperty("java.io.tmpdir", ancienTmp);
        }
    }

    @Test
    @DisplayName("T-114 : une seconde initialisation est refusée (E-1004)")
    void doubleInitialisationRefusee(@TempDir Path racine) {
        Contexte c = contexte(racine, Configuration.parDefaut(), sourceValide(), new PontSimule());

        assertEquals(Etat.READY, Bootstrap.demarrer(c).etat());

        Rapport second = Bootstrap.demarrer(c);
        assertEquals(Etat.DISABLED, second.etat());
        assertEquals(CodeErreur.DOUBLE_INIT, second.code());
    }

    @Test
    @DisplayName("Un binaire altéré interdit l'activation (FM-08, E-1003)")
    void binaireAltereDesactive(@TempDir Path racine) {
        Map<String, byte[]> contenus = new HashMap<>();
        contenus.put(PLATEFORME.cheminRessource(), "binaire-substitue".getBytes(StandardCharsets.UTF_8));
        contenus.put(
                PLATEFORME.cheminEmpreinte(),
                NativeLoader.condense(BINAIRE).getBytes(StandardCharsets.UTF_8));
        NativeLoader.SourceRessources altere = chemin -> {
            byte[] c = contenus.get(chemin);
            return c == null ? null : new ByteArrayInputStream(c);
        };

        Rapport r = Bootstrap.demarrer(
                contexte(racine, Configuration.parDefaut(), altere, new PontSimule()));

        assertEquals(Etat.DISABLED, r.etat(), "un binaire altéré ne doit jamais être activé");
        assertEquals(CodeErreur.HASH_NATIF_INVALIDE, r.code());
    }

    @Test
    @DisplayName("enabled = false désactive tout sans toucher au natif")
    void desactivationParConfiguration(@TempDir Path racine) throws Exception {
        Path fichier = racine.resolve("rustforgex.toml");
        Files.writeString(fichier, """
                [general]
                enabled = false
                """);
        Configuration desactive = Configuration.charger(fichier, cle -> null);
        PontSimule pont = new PontSimule();

        Rapport r = Bootstrap.demarrer(contexte(racine, desactive, sourceValide(), pont));

        assertEquals(Etat.DISABLED, r.etat());
        assertNull(r.code(), "une désactivation volontaire n'est pas une erreur");
        assertNull(r.bibliotheque(), "aucun binaire ne doit avoir été extrait");
        assertNull(pont.configRecue, "aucun appel natif ne doit avoir eu lieu");
    }

    @Test
    @DisplayName("Une erreur inattendue dégrade au lieu de remonter jusqu'à Forge")
    void erreurInattendueConfinee(@TempDir Path racine) {
        PontNatif pontFautif = new PontSimule() {
            @Override
            public int abiVersion() {
                throw new IllegalStateException("panne simulée du pont");
            }
        };

        Rapport r = Bootstrap.demarrer(
                contexte(racine, Configuration.parDefaut(), sourceValide(), pontFautif));

        assertEquals(Etat.DEGRADED, r.etat(), "le jeu doit rester jouable");
        assertTrue(r.message().contains("panne simulée"), r.message());
    }

    @Test
    @DisplayName("Le rapport journalise chaque étape franchie")
    void journalDesEtapes(@TempDir Path racine) {
        Rapport r = Bootstrap.demarrer(
                contexte(racine, Configuration.parDefaut(), sourceValide(), new PontSimule()));

        assertFalse(r.journal().isEmpty());
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("INIT")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("PROBE")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("LOAD_NATIVE")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("HANDSHAKE")));
        assertTrue(r.journal().stream().anyMatch(l -> l.startsWith("CONFIGURE")));
        assertTrue(r.journal().get(r.journal().size() - 1).startsWith("READY"));
    }
}
