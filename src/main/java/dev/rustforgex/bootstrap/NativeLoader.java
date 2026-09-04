package dev.rustforgex.bootstrap;

import dev.rustforgex.diag.CodeErreur;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import java.util.Locale;
import java.util.Optional;

/**
 * C-03 : extraction et chargement de la bibliothèque native.
 *
 * <p>Cahier des charges : PARTIE 5.3. Exigences : R-300 (vérification SHA-256
 * obligatoire avant chargement), R-301 (chemin d'extraction versionné par hash),
 * R-302 (repli si le système de fichiers refuse l'écriture ou l'exécution).
 * Tests : T-120 à T-123. Maturité : {@code STABLE}.
 *
 * <p>Algorithme :
 *
 * <pre>{@code
 * resource = "/natives/<os>-<arch>/<libname>"
 * sha256   = lire "/natives/<os>-<arch>/<libname>.sha256"
 * target   = <racine>/native/<sha256>/<libname>
 * si target existe et sha256(target) == attendu -> charger
 * sinon extraire dans un fichier temporaire puis renommer atomiquement -> charger
 * }</pre>
 *
 * <p>Aucune méthode de cette classe ne lève d'exception non contrôlée : un échec se
 * traduit toujours par une {@link EchecChargement} porteuse d'un code de l'annexe A.2,
 * que C-02 traduit en {@code DEGRADED} ou {@code DISABLED}.
 */
public final class NativeLoader {

    /** Répertoire de ressources contenant les binaires natifs, dans le JAR. */
    public static final String RACINE_RESSOURCES = "/natives";

    /** Extension du fichier portant le condensé attendu. */
    public static final String EXTENSION_EMPREINTE = ".sha256";

    /**
     * Plateforme cible : répertoire de ressources et nom de bibliothèque.
     *
     * @param repertoire nom du répertoire, par exemple {@code windows-x86_64}
     * @param bibliotheque nom du fichier, par exemple {@code rfx_native.dll}
     */
    public record Plateforme(String repertoire, String bibliotheque) {

        /** @return le chemin de ressource de la bibliothèque dans le JAR. */
        public String cheminRessource() {
            return RACINE_RESSOURCES + "/" + repertoire + "/" + bibliotheque;
        }

        /** @return le chemin de ressource du condensé attendu. */
        public String cheminEmpreinte() {
            return cheminRessource() + EXTENSION_EMPREINTE;
        }
    }

    /** Échec d'extraction ou de chargement, porteur d'un code de l'annexe A.2. */
    public static final class EchecChargement extends Exception {

        private static final long serialVersionUID = 1L;

        private final transient CodeErreur code;

        /**
         * @param code code normatif de l'annexe A.2
         * @param message description de l'échec, destinée aux journaux
         */
        public EchecChargement(CodeErreur code, String message) {
            super(code.identifiant() + " : " + message);
            this.code = code;
        }

        /**
         * @param code code normatif de l'annexe A.2
         * @param message description de l'échec
         * @param cause exception d'origine
         */
        public EchecChargement(CodeErreur code, String message, Throwable cause) {
            super(code.identifiant() + " : " + message, cause);
            this.code = code;
        }

        /** @return le code normatif associé à cet échec. */
        public CodeErreur code() {
            return code;
        }
    }

    /** Source des ressources embarquées, injectable pour les tests. */
    @FunctionalInterface
    public interface SourceRessources {

        /**
         * Ouvre une ressource.
         *
         * @param chemin chemin absolu de ressource, commençant par {@code /}
         * @return le flux, ou {@code null} si la ressource n'existe pas
         */
        InputStream ouvrir(String chemin);
    }

    private final SourceRessources source;

    /** Construit un loader lisant les ressources embarquées dans le JAR du mod. */
    public NativeLoader() {
        this(chemin -> NativeLoader.class.getResourceAsStream(chemin));
    }

    /**
     * Construit un loader lisant une source de ressources donnée.
     *
     * @param source source des ressources
     */
    public NativeLoader(SourceRessources source) {
        this.source = source;
    }

    /**
     * Détermine la plateforme de la machine hôte.
     *
     * <p>Les triplets reconnus sont ceux que le build sait produire (PARTIE 23.1).
     * Une plateforme inconnue renvoie un résultat vide : le runtime passera en
     * {@code DEGRADED} avec un message explicite (PARTIE 23.5), sans jamais tenter de
     * charger un binaire d'une autre architecture.
     *
     * @return la plateforme hôte, ou un résultat vide si elle n'est pas supportée
     */
    public static Optional<Plateforme> plateformeHote() {
        return plateforme(System.getProperty("os.name", ""), System.getProperty("os.arch", ""));
    }

    /**
     * Détermine la plateforme correspondant à un couple système/architecture.
     *
     * @param osName valeur de {@code os.name}
     * @param osArch valeur de {@code os.arch}
     * @return la plateforme correspondante, ou un résultat vide
     */
    public static Optional<Plateforme> plateforme(String osName, String osArch) {
        String os = osName.toLowerCase(Locale.ROOT);
        String arch = osArch.toLowerCase(Locale.ROOT);
        boolean x64 = arch.equals("amd64") || arch.equals("x86_64");
        boolean arm64 = arch.equals("aarch64") || arch.equals("arm64");

        if (os.contains("win") && x64) {
            return Optional.of(new Plateforme("windows-x86_64", "rfx_native.dll"));
        }
        if (os.contains("linux") && x64) {
            return Optional.of(new Plateforme("linux-x86_64", "librfx_native.so"));
        }
        if (os.contains("linux") && arm64) {
            return Optional.of(new Plateforme("linux-aarch64", "librfx_native.so"));
        }
        return Optional.empty();
    }

    /**
     * Extrait la bibliothèque et vérifie son intégrité, sans la charger.
     *
     * <p>L'extraction est idempotente : un binaire déjà présent et dont le condensé
     * correspond est réutilisé tel quel. Le chemin est versionné par le condensé
     * (R-301), ce qui permet à plusieurs versions de coexister sans conflit.
     *
     * @param plateforme plateforme dont extraire le binaire
     * @param racine racine de travail, typiquement {@code <gameDir>/rustforgex}
     * @return le chemin du binaire vérifié, prêt à être chargé
     * @throws EchecChargement si le binaire est absent, altéré, ou si aucun
     *     emplacement inscriptible n'a pu être trouvé
     */
    public Path preparer(Plateforme plateforme, Path racine) throws EchecChargement {
        String attendu = lireEmpreinteAttendue(plateforme);
        byte[] binaire = lireBinaire(plateforme);

        // R-300 : la vérification précède toute écriture et tout chargement.
        String reel = condense(binaire);
        if (!reel.equalsIgnoreCase(attendu)) {
            throw new EchecChargement(
                    CodeErreur.HASH_NATIF_INVALIDE,
                    "le binaire embarqué pour " + plateforme.repertoire()
                            + " ne correspond pas à son empreinte (attendu " + attendu
                            + ", obtenu " + reel + ")");
        }

        EchecChargement premierEchec = null;
        for (Path base : emplacementsCandidats(racine)) {
            try {
                return deposer(base, plateforme, binaire, attendu);
            } catch (IOException e) {
                if (premierEchec == null) {
                    premierEchec = new EchecChargement(
                            CodeErreur.CHARGEMENT_ECHOUE,
                            "extraction impossible sous " + base, e);
                }
            }
        }
        throw premierEchec != null
                ? premierEchec
                : new EchecChargement(CodeErreur.CHARGEMENT_ECHOUE, "aucun emplacement d'extraction");
    }

    /**
     * Charge une bibliothèque déjà extraite et vérifiée.
     *
     * @param bibliotheque chemin renvoyé par {@link #preparer(Plateforme, Path)}
     * @throws EchecChargement si le système refuse le chargement
     */
    public static void charger(Path bibliotheque) throws EchecChargement {
        try {
            System.load(bibliotheque.toAbsolutePath().toString());
        } catch (UnsatisfiedLinkError | SecurityException e) {
            // FM-05 : le diagnostic doit permettre de distinguer une libc trop
            // ancienne, un montage noexec et un refus de sécurité.
            throw new EchecChargement(
                    CodeErreur.CHARGEMENT_ECHOUE,
                    "System.load a refusé " + bibliotheque + " (" + e.getMessage() + ")", e);
        }
    }

    /**
     * Emplacements d'extraction essayés dans l'ordre.
     *
     * <p>R-302 : si la racine de jeu est en lecture seule ou montée {@code noexec},
     * le répertoire temporaire de la JVM est essayé avant d'échouer proprement.
     */
    private static Iterable<Path> emplacementsCandidats(Path racine) {
        Path repli = Path.of(System.getProperty("java.io.tmpdir", ".")).resolve("rustforgex");
        if (racine.toAbsolutePath().normalize().equals(repli.toAbsolutePath().normalize())) {
            return java.util.List.of(racine);
        }
        return java.util.List.of(racine, repli);
    }

    /** Dépose le binaire sous {@code base}, en réutilisant une extraction valide. */
    private Path deposer(Path base, Plateforme plateforme, byte[] binaire, String empreinte)
            throws IOException, EchecChargement {

        // R-301 : le chemin est versionné par le condensé, jamais par la version du mod.
        Path dossier = base.resolve("native").resolve(empreinte);
        Path cible = dossier.resolve(plateforme.bibliotheque());

        if (Files.isRegularFile(cible) && empreinte.equalsIgnoreCase(condense(Files.readAllBytes(cible)))) {
            return cible;
        }

        Files.createDirectories(dossier);
        Path temporaire = Files.createTempFile(dossier, plateforme.bibliotheque(), ".partiel");
        try {
            Files.write(temporaire, binaire);
            // Vérification du contenu réellement écrit sur le disque : une écriture
            // partielle ou corrompue ne doit jamais être chargée (R-300).
            String ecrit = condense(Files.readAllBytes(temporaire));
            if (!empreinte.equalsIgnoreCase(ecrit)) {
                throw new EchecChargement(
                        CodeErreur.HASH_NATIF_INVALIDE,
                        "le binaire écrit sous " + dossier + " ne correspond pas à son empreinte");
            }
            deplacer(temporaire, cible);
            return cible;
        } finally {
            Files.deleteIfExists(temporaire);
        }
    }

    /** Renomme atomiquement quand le système le permet, sinon remplace. */
    private static void deplacer(Path source, Path cible) throws IOException {
        try {
            Files.move(source, cible, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException e) {
            Files.move(source, cible, StandardCopyOption.REPLACE_EXISTING);
        } catch (java.nio.file.FileAlreadyExistsException e) {
            // Une autre instance a extrait le même contenu entre-temps : le chemin
            // étant versionné par le condensé, le fichier en place est identique.
            Files.deleteIfExists(source);
        }
    }

    /** Lit le condensé attendu depuis les ressources embarquées. */
    private String lireEmpreinteAttendue(Plateforme plateforme) throws EchecChargement {
        try (InputStream flux = source.ouvrir(plateforme.cheminEmpreinte())) {
            if (flux == null) {
                throw new EchecChargement(
                        CodeErreur.NATIF_ABSENT,
                        "empreinte absente pour " + plateforme.repertoire()
                                + " (" + plateforme.cheminEmpreinte() + ")");
            }
            String contenu = new String(flux.readAllBytes(), StandardCharsets.UTF_8).trim();
            if (!estCondenseValide(contenu)) {
                throw new EchecChargement(
                        CodeErreur.HASH_NATIF_INVALIDE,
                        "empreinte illisible pour " + plateforme.repertoire() + " : « " + contenu + " »");
            }
            return contenu;
        } catch (IOException e) {
            throw new EchecChargement(
                    CodeErreur.NATIF_ABSENT, "lecture de l'empreinte impossible", e);
        }
    }

    /** Lit le binaire natif depuis les ressources embarquées. */
    private byte[] lireBinaire(Plateforme plateforme) throws EchecChargement {
        try (InputStream flux = source.ouvrir(plateforme.cheminRessource())) {
            if (flux == null) {
                throw new EchecChargement(
                        CodeErreur.NATIF_ABSENT,
                        "aucun binaire natif embarqué pour " + plateforme.repertoire());
            }
            return flux.readAllBytes();
        } catch (IOException e) {
            throw new EchecChargement(
                    CodeErreur.NATIF_ABSENT, "lecture du binaire natif impossible", e);
        }
    }

    /** @return {@code true} si la chaîne est un condensé SHA-256 hexadécimal. */
    private static boolean estCondenseValide(String valeur) {
        if (valeur.length() != 64) {
            return false;
        }
        for (int i = 0; i < valeur.length(); i++) {
            char c = valeur.charAt(i);
            boolean hex = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
            if (!hex) {
                return false;
            }
        }
        return true;
    }

    /** Calcule le condensé SHA-256 d'un contenu, en hexadécimal minuscule. */
    static String condense(byte[] contenu) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            return HexFormat.of().formatHex(digest.digest(contenu));
        } catch (NoSuchAlgorithmException e) {
            // SHA-256 est exigé de toute implémentation Java : ce cas ne peut pas
            // survenir sur une JVM conforme.
            throw new IllegalStateException("SHA-256 indisponible sur cette JVM", e);
        }
    }
}
