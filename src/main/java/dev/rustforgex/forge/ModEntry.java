package dev.rustforgex.forge;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import java.util.Set;

/**
 * Un mod inventorié, et son empreinte à la demande (C-41).
 *
 * <p>Cahier des charges : PARTIE 5.39. Maturité : {@code STABLE}.
 *
 * <h2>Pourquoi l'empreinte est paresseuse</h2>
 *
 * <p>La PARTIE 5.39 demande {@code owner_mod_hash = sha256} tronqué du JAR, et R-621
 * exige que la découverte tienne sous 500 ms pour 250 mods. Mesuré sur le serveur de
 * banc — 273 JAR, 1 214 Mio — un sha256 intégral prend <strong>1 248 ms</strong> avec
 * le cache de fichiers déjà chaud. Les deux exigences ne tiennent pas ensemble.
 *
 * <p>L'empreinte est donc calculée <strong>au premier usage</strong>, jamais pendant la
 * découverte, et retenue ensuite. Rien de ce qui tourne pendant une partie n'en a
 * besoin : elle sert à identifier une version de mod dans un rapport ou une
 * télémétrie, hors du chemin de jeu. Voir ADR-023.
 */
public final class ModEntry {

    /** Empreinte des mods dont le fichier est illisible ou absent. */
    public static final String NO_HASH = "sans-empreinte";

    /**
     * Caractères hexadécimaux retenus de l'empreinte.
     *
     * <p>Seize, soit soixante-quatre bits. De quoi distinguer deux versions d'un mod
     * sans traîner soixante-quatre caractères dans chaque ligne de rapport ; le hachage
     * ne sert ici qu'à l'identification, jamais à une garantie de sécurité.
     */
    private static final int HASH_CHARS = 16;

    private static final int READ_BUFFER = 1 << 16;

    private final String modId;
    private final String version;
    private final Path file;
    private final String moduleName;
    private final Set<String> packages;
    private final long sizeBytes;

    /**
     * Empreinte calculée, ou {@code null} tant que personne ne l'a demandée.
     *
     * <p>{@code volatile} : la demande peut venir du fil serveur comme d'un fil de
     * télémétrie. Deux calculs concurrents rendraient la même valeur, et un verrou
     * coûterait plus que cette redondance improbable.
     */
    private volatile String ownerModHash;

    ModEntry(ModSource.RawMod raw) {
        this.modId = raw.modId();
        this.version = raw.version();
        this.file = raw.file();
        this.moduleName = raw.moduleName();
        this.packages = raw.packages();
        this.sizeBytes = sizeOf(raw.file());
    }

    /** @return l'identifiant déclaré du mod */
    public String modId() {
        return modId;
    }

    /** @return la version déclarée, ou {@code "inconnue"} */
    public String version() {
        return version;
    }

    /** @return le chemin du JAR ou du répertoire, éventuellement {@code null} */
    public Path file() {
        return file;
    }

    /** @return le nom du module Java portant ce mod, éventuellement {@code null} */
    public String moduleName() {
        return moduleName;
    }

    /** @return les paquets déclarés par le fichier */
    public Set<String> packages() {
        return packages;
    }

    /** @return la taille du fichier en octets, ou {@code 0} s'il est illisible */
    public long sizeBytes() {
        return sizeBytes;
    }

    /**
     * Empreinte du fichier, calculée au premier appel puis retenue.
     *
     * <p>Cet appel lit le fichier entier : il n'a rien à faire dans un tick (INV-14).
     *
     * @return les seize premiers caractères du sha256, ou {@link #NO_HASH}
     */
    public String ownerModHash() {
        String known = ownerModHash;
        if (known != null) {
            return known;
        }
        String computed = digest(file);
        ownerModHash = computed;
        return computed;
    }

    /** @return {@code true} si l'empreinte a déjà été calculée */
    public boolean hashComputed() {
        return ownerModHash != null;
    }

    /** Taille du fichier, sans échouer sur un mod en développement sans JAR. */
    private static long sizeOf(Path file) {
        if (file == null) {
            return 0L;
        }
        try {
            return Files.isRegularFile(file) ? Files.size(file) : 0L;
        } catch (IOException e) {
            return 0L;
        }
    }

    /**
     * Calcule l'empreinte tronquée d'un fichier.
     *
     * <p>Un mod en développement est un répertoire, pas un JAR : il n'a pas d'empreinte
     * stable et n'en aura pas. Le dire vaut mieux que hacher un chemin, ce qui
     * produirait une empreinte qui ne dépend pas du contenu.
     */
    private static String digest(Path file) {
        if (file == null || !Files.isRegularFile(file)) {
            return NO_HASH;
        }
        try {
            MessageDigest sha256 = MessageDigest.getInstance("SHA-256");
            byte[] buffer = new byte[READ_BUFFER];
            try (InputStream stream = Files.newInputStream(file)) {
                for (int read = stream.read(buffer); read > 0; read = stream.read(buffer)) {
                    sha256.update(buffer, 0, read);
                }
            }
            return HexFormat.of().formatHex(sha256.digest()).substring(0, HASH_CHARS);
        } catch (IOException | NoSuchAlgorithmException e) {
            // Un mod dont on ne sait pas lire le fichier reste un mod : l'inventaire
            // doit le porter quand même, sans empreinte.
            return NO_HASH;
        }
    }
}
