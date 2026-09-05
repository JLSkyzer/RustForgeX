package dev.rustforgex.launch;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.File;
import java.io.IOException;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Enumeration;
import java.util.LinkedHashSet;
import java.util.Locale;
import java.util.Set;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

/**
 * Énumération des classes que le transformateur doit cibler (ADR-019).
 *
 * <p>Composant : C-04. Cahier des charges : PARTIE 5.4. Invariant : INV-12.
 * Maturité : {@code STABLE}.
 *
 * <p>{@code ITransformer} ne connaît pas le caractère générique : {@code TransformStore}
 * filtre sur un ensemble de noms, et {@code TargetType} ne propose que {@code CLASS},
 * {@code PRE_CLASS}, {@code METHOD} et {@code FIELD}. Les cibles doivent donc être
 * nommées une à une, ce qui oblige à les découvrir avant tout chargement.
 *
 * <p>Deux sources, disponibles au moment où ModLauncher demande nos transformateurs :
 * le dossier {@code mods} du jeu, et les archives du {@code legacyClassPath}.
 *
 * <h2>Ce qui n'est pas fait ici</h2>
 *
 * <p>Aucun nom de mod n'intervient (INV-12) : on énumère le contenu de fichiers, on ne
 * reconnaît personne. Le filtre porte sur des préfixes de paquets, et il sert à écarter
 * les bibliothèques tierces qui ne relèvent ni du jeu ni d'un mod — pas à traiter un mod
 * différemment d'un autre.
 *
 * <p>Le tri fin — méthode trop courte, constructeur, classe du chargeur de démarrage —
 * reste à {@link ProbeEligibility}, qui travaille sur le bytecode réel. Ici on ne fait
 * que réduire l'ensemble à ce qui a une chance d'être sondé.
 */
public final class TargetScanner {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-launch");

    /**
     * Préfixes retenus dans les archives du {@code legacyClassPath}.
     *
     * <p>Le classpath d'un serveur moddé porte des centaines de bibliothèques — bases de
     * données, sérialiseurs, journalisation — dont aucune n'est du code de jeu. Les
     * cibler coûterait de la mémoire et du temps de décodage sans jamais rien apprendre
     * sur le tick.
     *
     * <p>{@code net/minecraft} et {@code net/minecraftforge} ne sont pas des mods : ce
     * sont la plateforme. Les nommer ici ne contrevient pas à INV-12.
     */
    private static final String[] PLATFORM_PACKAGES = {"net/minecraft/", "net/minecraftforge/"};

    /**
     * Paquets jamais ciblés, quelle qu'en soit la provenance.
     *
     * <p>Le code de rendu n'existe pas sur un serveur dédié. Le désigner comme cible
     * suffit à le faire passer dans la chaîne de transformation, où Mixin échoue à en
     * résoudre les métadonnées — et le serveur ne démarre pas.
     */
    private static final String[] EXCLUDED_PACKAGES = {
        "net/minecraft/client/", "com/mojang/blaze3d/"
    };

    /** Suffixe des entrées de classe dans une archive. */
    private static final String CLASS_SUFFIX = ".class";

    /** Au-delà, l'énumération est jugée aberrante et abandonnée. */
    private static final int MAX_TARGETS = 400_000;

    private TargetScanner() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Énumère les noms internes des classes à cibler.
     *
     * <p>Ne lève jamais : une archive illisible est comptée et ignorée. Une énumération
     * partielle produit une instrumentation partielle, ce qui reste préférable à un
     * démarrage interrompu.
     *
     * @param gameDirectory répertoire de jeu, ou {@code null} s'il est inconnu ;
     *     conservé pour l'extension aux classes de mods, qui n'est pas active
     * @return les noms internes, par exemple {@code net/minecraft/world/entity/Mob}
     */
    public static Set<String> scan(Path gameDirectory) {
        Set<String> targets = new LinkedHashSet<>(16_384);

        // Les classes des mods ne sont pas ciblées à ce jour : voir ADR-019, complément
        // du 2026-09-05. Désigner une cible n'est pas neutre — ModLauncher fait alors
        // passer la classe par toute la chaîne de transformation, y compris Mixin, là où
        // elle prenait le chemin rapide. Sur les classes de mods, cela suffit à faire
        // échouer le démarrage d'un serveur.
        scanLegacyClassPath(targets);

        // En production, le JAR du jeu ne figure pas sur le classpath : FML le localise
        // par son propre mécanisme. Sans cette seconde source, l'énumération ne trouve
        // que les utilitaires de Forge, et rien de ce où passe le temps de tick.
        if (gameDirectory != null) {
            scanGameLibraries(gameDirectory.resolve("libraries").resolve("net")
                    .resolve("minecraft"), targets);
        }

        LOGGER.info("Cibles d'instrumentation énumérées : {} classes.", targets.size());
        return targets;
    }

    /**
     * Énumère les classes de plateforme des archives du jeu.
     *
     * <p>Parcourt récursivement le répertoire, sans présumer du nom de l'archive : la
     * disposition diffère entre un client et un serveur, et entre deux versions de
     * l'installeur. Le filtre de paquets fait le tri.
     */
    private static void scanGameLibraries(Path root, Set<String> targets) {
        if (!Files.isDirectory(root)) {
            return;
        }
        try (var paths = Files.walk(root)) {
            paths.filter(Files::isRegularFile)
                    .filter(p -> p.toString().toLowerCase(Locale.ROOT).endsWith(".jar"))
                    .forEach(jar -> collectFromArchive(jar, targets, true));
        } catch (IOException | RuntimeException e) {
            LOGGER.warn("Bibliothèques du jeu illisibles sous {} : les classes du jeu ne "
                    + "seront pas sondées.", root);
        }
    }

    /** Énumère toutes les classes des archives de {@code mods}. */
    private static void scanModsDirectory(Path modsDirectory, Set<String> targets) {
        if (!Files.isDirectory(modsDirectory)) {
            // Environnement de développement : les mods viennent du classpath, pas d'un
            // dossier. Ce n'est pas une anomalie.
            return;
        }
        try (DirectoryStream<Path> jars = Files.newDirectoryStream(modsDirectory, "*.jar")) {
            for (Path jar : jars) {
                collectFromArchive(jar, targets, false);
            }
        } catch (IOException e) {
            LOGGER.warn("Dossier {} illisible : les mods ne seront pas sondés.",
                    modsDirectory, e);
        }
    }

    /** Énumère les classes de plateforme trouvées sur le classpath d'amorçage. */
    private static void scanLegacyClassPath(Set<String> targets) {
        String classPath = System.getProperty("legacyClassPath", "");
        if (classPath.isBlank()) {
            return;
        }
        for (String entry : classPath.split(File.pathSeparator)) {
            if (entry.isBlank()) {
                continue;
            }
            Path path = Path.of(entry);
            if (Files.isDirectory(path)) {
                collectFromDirectory(path, targets);
            } else if (entry.toLowerCase(Locale.ROOT).endsWith(".jar")) {
                collectFromArchive(path, targets, true);
            }
        }
    }

    /**
     * Ajoute les classes d'une archive.
     *
     * @param platformOnly {@code true} pour ne retenir que les paquets de plateforme
     */
    private static void collectFromArchive(Path archive, Set<String> targets, boolean platformOnly) {
        try (ZipFile zip = new ZipFile(archive.toFile())) {
            Enumeration<? extends ZipEntry> entries = zip.entries();
            while (entries.hasMoreElements()) {
                String name = entries.nextElement().getName();
                String internal = internalName(name);
                if (internal != null && accepts(internal, platformOnly)) {
                    if (targets.size() >= MAX_TARGETS) {
                        LOGGER.warn("Énumération interrompue à {} cibles : au-delà, le coût "
                                + "de l'instrumentation dépasserait ce qu'elle rapporte.",
                                MAX_TARGETS);
                        return;
                    }
                    targets.add(internal);
                }
            }
        } catch (IOException | RuntimeException e) {
            LOGGER.warn("Archive {} illisible : ses classes ne seront pas sondées.", archive);
        }
    }

    /** Ajoute les classes d'un répertoire de sortie, forme du développement. */
    private static void collectFromDirectory(Path root, Set<String> targets) {
        try (var paths = Files.walk(root)) {
            paths.filter(Files::isRegularFile).forEach(file -> {
                String relative = root.relativize(file).toString().replace(File.separatorChar, '/');
                String internal = internalName(relative);
                if (internal != null && accepts(internal, false) && targets.size() < MAX_TARGETS) {
                    targets.add(internal);
                }
            });
        } catch (IOException | RuntimeException e) {
            LOGGER.warn("Répertoire {} illisible : ses classes ne seront pas sondées.", root);
        }
    }

    /** Nom interne d'une entrée d'archive, ou {@code null} si ce n'est pas une classe. */
    private static String internalName(String entry) {
        if (!entry.endsWith(CLASS_SUFFIX) || entry.startsWith("META-INF/")) {
            return null;
        }
        String internal = entry.substring(0, entry.length() - CLASS_SUFFIX.length());
        // `module-info` et `package-info` n'ont pas de corps exécutable.
        if (internal.endsWith("module-info") || internal.endsWith("package-info")) {
            return null;
        }
        return internal;
    }

    /** Décide si une classe mérite d'être ciblée. */
    private static boolean accepts(String internalName, boolean platformOnly) {
        if (ProbeEligibility.isBootstrapClass(internalName)) {
            return false;
        }
        for (String excluded : EXCLUDED_PACKAGES) {
            if (internalName.startsWith(excluded)) {
                return false;
            }
        }
        if (internalName.startsWith("dev/rustforgex/")) {
            // Se sonder soi-même fausserait la mesure (ProbeEligibility.OWN_CLASS).
            return false;
        }
        if (!platformOnly) {
            return true;
        }
        for (String prefix : PLATFORM_PACKAGES) {
            if (internalName.startsWith(prefix)) {
                return true;
            }
        }
        return false;
    }
}
