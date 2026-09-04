package dev.rustforgex.bootstrap;

import dev.rustforgex.diag.ErrorCode;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import java.util.List;
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
 * traduit toujours par une {@link LoadFailure} porteuse d'un code de l'annexe A.2,
 * que C-02 traduit en {@code DEGRADED} ou {@code DISABLED}.
 */
public final class NativeLoader {

    /** Répertoire de ressources contenant les binaires natifs, dans le JAR. */
    public static final String RESOURCES_ROOT = "/natives";

    /** Extension du fichier portant le condensé attendu. */
    public static final String DIGEST_EXTENSION = ".sha256";

    /**
     * Plateforme cible : répertoire de ressources et nom de bibliothèque.
     *
     * @param directory nom du répertoire, par exemple {@code windows-x86_64}
     * @param library nom du fichier, par exemple {@code rfx_native.dll}
     */
    public record Platform(String directory, String library) {

        /** @return le chemin de ressource de la bibliothèque dans le JAR. */
        public String resourcePath() {
            return RESOURCES_ROOT + "/" + directory + "/" + library;
        }

        /** @return le chemin de ressource du condensé attendu. */
        public String digestPath() {
            return resourcePath() + DIGEST_EXTENSION;
        }
    }

    /** Échec d'extraction ou de chargement, porteur d'un code de l'annexe A.2. */
    public static final class LoadFailure extends Exception {

        private static final long serialVersionUID = 1L;

        private final transient ErrorCode code;

        /**
         * @param code code normatif de l'annexe A.2
         * @param message description de l'échec, destinée aux journaux
         */
        public LoadFailure(ErrorCode code, String message) {
            super(code.id() + " : " + message);
            this.code = code;
        }

        /**
         * @param code code normatif de l'annexe A.2
         * @param message description de l'échec
         * @param cause exception d'origine
         */
        public LoadFailure(ErrorCode code, String message, Throwable cause) {
            super(code.id() + " : " + message, cause);
            this.code = code;
        }

        /** @return le code normatif associé à cet échec. */
        public ErrorCode code() {
            return code;
        }
    }

    /** Source des ressources embarquées, injectable pour les tests. */
    @FunctionalInterface
    public interface ResourceSource {

        /**
         * Ouvre une ressource.
         *
         * @param path chemin absolu de ressource, commençant par {@code /}
         * @return le flux, ou {@code null} si la ressource n'existe pas
         */
        InputStream open(String path);
    }

    private final ResourceSource source;

    /** Construit un loader lisant les ressources embarquées dans le JAR du mod. */
    public NativeLoader() {
        this(path -> NativeLoader.class.getResourceAsStream(path));
    }

    /**
     * Construit un loader lisant une source de ressources donnée.
     *
     * @param source source des ressources
     */
    public NativeLoader(ResourceSource source) {
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
    public static Optional<Platform> hostPlatform() {
        return platform(System.getProperty("os.name", ""), System.getProperty("os.arch", ""));
    }

    /**
     * Détermine la plateforme correspondant à un couple système/architecture.
     *
     * @param osName valeur de {@code os.name}
     * @param osArch valeur de {@code os.arch}
     * @return la plateforme correspondante, ou un résultat vide
     */
    public static Optional<Platform> platform(String osName, String osArch) {
        String os = osName.toLowerCase(Locale.ROOT);
        String arch = osArch.toLowerCase(Locale.ROOT);
        boolean x64 = arch.equals("amd64") || arch.equals("x86_64");
        boolean arm64 = arch.equals("aarch64") || arch.equals("arm64");

        if (os.contains("win") && x64) {
            return Optional.of(new Platform("windows-x86_64", "rfx_native.dll"));
        }
        if (os.contains("linux") && x64) {
            return Optional.of(new Platform("linux-x86_64", "librfx_native.so"));
        }
        if (os.contains("linux") && arm64) {
            return Optional.of(new Platform("linux-aarch64", "librfx_native.so"));
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
     * @param platform plateforme dont extraire le binaire
     * @param root racine de travail, typiquement {@code <gameDir>/rustforgex}
     * @return le chemin du binaire vérifié, prêt à être chargé
     * @throws LoadFailure si le binaire est absent, altéré, ou si aucun emplacement
     *     inscriptible n'a pu être trouvé
     */
    public Path prepare(Platform platform, Path root) throws LoadFailure {
        String expected = readExpectedDigest(platform);
        byte[] binary = readBinary(platform);

        // R-300 : la vérification précède toute écriture et tout chargement.
        String actual = digest(binary);
        if (!actual.equalsIgnoreCase(expected)) {
            throw new LoadFailure(
                    ErrorCode.INVALID_NATIVE_DIGEST,
                    "le binaire embarqué pour " + platform.directory()
                            + " ne correspond pas à son empreinte (attendu " + expected
                            + ", obtenu " + actual + ")");
        }

        LoadFailure firstFailure = null;
        for (Path base : candidateLocations(root)) {
            try {
                return store(base, platform, binary, expected);
            } catch (IOException e) {
                if (firstFailure == null) {
                    firstFailure = new LoadFailure(
                            ErrorCode.LOAD_FAILED, "extraction impossible sous " + base, e);
                }
            }
        }
        throw firstFailure != null
                ? firstFailure
                : new LoadFailure(ErrorCode.LOAD_FAILED, "aucun emplacement d'extraction");
    }

    /**
     * Charge une bibliothèque déjà extraite et vérifiée.
     *
     * @param library chemin renvoyé par {@link #prepare(Platform, Path)}
     * @throws LoadFailure si le système refuse le chargement
     */
    public static void load(Path library) throws LoadFailure {
        try {
            System.load(library.toAbsolutePath().toString());
        } catch (UnsatisfiedLinkError | SecurityException e) {
            // FM-05 : le diagnostic doit permettre de distinguer une libc trop
            // ancienne, un montage noexec et un refus de sécurité.
            throw new LoadFailure(
                    ErrorCode.LOAD_FAILED,
                    "System.load a refusé " + library + " (" + e.getMessage() + ")", e);
        }
    }

    /**
     * Emplacements d'extraction essayés dans l'ordre.
     *
     * <p>R-302 : si la racine de jeu est en lecture seule ou montée {@code noexec},
     * le répertoire temporaire de la JVM est essayé avant d'échouer proprement.
     */
    private static List<Path> candidateLocations(Path root) {
        Path fallback = Path.of(System.getProperty("java.io.tmpdir", ".")).resolve("rustforgex");
        if (root.toAbsolutePath().normalize().equals(fallback.toAbsolutePath().normalize())) {
            return List.of(root);
        }
        return List.of(root, fallback);
    }

    /** Dépose le binaire sous {@code base}, en réutilisant une extraction valide. */
    private Path store(Path base, Platform platform, byte[] binary, String expectedDigest)
            throws IOException, LoadFailure {

        // R-301 : le chemin est versionné par le condensé, jamais par la version du mod.
        Path folder = base.resolve("native").resolve(expectedDigest);
        Path target = folder.resolve(platform.library());

        if (Files.isRegularFile(target)
                && expectedDigest.equalsIgnoreCase(digest(Files.readAllBytes(target)))) {
            return target;
        }

        Files.createDirectories(folder);
        Path temporary = Files.createTempFile(folder, platform.library(), ".partial");
        try {
            Files.write(temporary, binary);
            // Vérification du contenu réellement écrit sur le disque : une écriture
            // partielle ou corrompue ne doit jamais être chargée (R-300).
            String written = digest(Files.readAllBytes(temporary));
            if (!expectedDigest.equalsIgnoreCase(written)) {
                throw new LoadFailure(
                        ErrorCode.INVALID_NATIVE_DIGEST,
                        "le binaire écrit sous " + folder + " ne correspond pas à son empreinte");
            }
            moveInto(temporary, target);
            return target;
        } finally {
            Files.deleteIfExists(temporary);
        }
    }

    /** Renomme atomiquement quand le système le permet, sinon remplace. */
    private static void moveInto(Path source, Path target) throws IOException {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException e) {
            Files.move(source, target, StandardCopyOption.REPLACE_EXISTING);
        } catch (FileAlreadyExistsException e) {
            // Une autre instance a extrait le même contenu entre-temps : le chemin
            // étant versionné par le condensé, le fichier en place est identique.
            Files.deleteIfExists(source);
        }
    }

    /** Lit le condensé attendu depuis les ressources embarquées. */
    private String readExpectedDigest(Platform platform) throws LoadFailure {
        try (InputStream stream = source.open(platform.digestPath())) {
            if (stream == null) {
                throw new LoadFailure(
                        ErrorCode.NATIVE_MISSING,
                        "empreinte absente pour " + platform.directory()
                                + " (" + platform.digestPath() + ")");
            }
            String content = new String(stream.readAllBytes(), StandardCharsets.UTF_8).trim();
            if (!isValidDigest(content)) {
                throw new LoadFailure(
                        ErrorCode.INVALID_NATIVE_DIGEST,
                        "empreinte illisible pour " + platform.directory() + " : « " + content + " »");
            }
            return content;
        } catch (IOException e) {
            throw new LoadFailure(ErrorCode.NATIVE_MISSING, "lecture de l'empreinte impossible", e);
        }
    }

    /** Lit le binaire natif depuis les ressources embarquées. */
    private byte[] readBinary(Platform platform) throws LoadFailure {
        try (InputStream stream = source.open(platform.resourcePath())) {
            if (stream == null) {
                throw new LoadFailure(
                        ErrorCode.NATIVE_MISSING,
                        "aucun binaire natif embarqué pour " + platform.directory());
            }
            return stream.readAllBytes();
        } catch (IOException e) {
            throw new LoadFailure(
                    ErrorCode.NATIVE_MISSING, "lecture du binaire natif impossible", e);
        }
    }

    /** @return {@code true} si la chaîne est un condensé SHA-256 hexadécimal. */
    private static boolean isValidDigest(String value) {
        if (value.length() != 64) {
            return false;
        }
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            boolean hex = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
            if (!hex) {
                return false;
            }
        }
        return true;
    }

    /** Calcule le condensé SHA-256 d'un contenu, en hexadécimal minuscule. */
    static String digest(byte[] content) {
        try {
            MessageDigest sha256 = MessageDigest.getInstance("SHA-256");
            return HexFormat.of().formatHex(sha256.digest(content));
        } catch (NoSuchAlgorithmException e) {
            // SHA-256 est exigé de toute implémentation Java : ce cas ne peut pas
            // survenir sur une JVM conforme.
            throw new IllegalStateException("SHA-256 indisponible sur cette JVM", e);
        }
    }
}
