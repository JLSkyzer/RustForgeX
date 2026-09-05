package dev.rustforgex.launch;

import org.objectweb.asm.AnnotationVisitor;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Enumeration;
import java.util.HashSet;
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

    /**
     * Empreintes, en UTF-8, du code qui n'existe pas sur un serveur dédié.
     *
     * <p>Une classe de mod qui les mentionne référence du rendu ou se déclare cliente.
     * La désigner comme cible sur un serveur dédié suffit à faire échouer le démarrage :
     * elle entre alors dans la chaîne de transformation, où Mixin ne sait pas la
     * résoudre. Le nom du paquet ne suffit pas à les reconnaître — le cas qui a cassé un
     * serveur de production s'appelait {@code common.events.ClientEvents} — d'où ce
     * filtre sur ce que la classe <strong>référence</strong>, et non sur ce qu'elle
     * s'appelle.
     */
    private static final byte[][] CLIENT_MARKERS = {
        "net/minecraft/client/".getBytes(StandardCharsets.UTF_8),
        "com/mojang/blaze3d/".getBytes(StandardCharsets.UTF_8),
        "Lnet/minecraftforge/api/distmarker/OnlyIn;".getBytes(StandardCharsets.UTF_8),
    };

    /**
     * Empreinte d'une classe mixin.
     *
     * <p>Une classe portant {@code @Mixin} n'est pas du code qui s'exécute : c'est une
     * description que Mixin lit, puis dont il transplante les méthodes dans une classe
     * cible. Y injecter une sonde revient à modifier le patch avant qu'il soit appliqué,
     * et Mixin échoue alors à retrouver ses points d'injection — constaté en production
     * sur un {@code @ModifyVariable} de ValkyrienSkies.
     *
     * <p>Elles sont écartées des deux côtés, client comme serveur.
     */
    private static final String MIXIN_ANNOTATION = "Lorg/spongepowered/asm/mixin/Mixin;";

    private static final byte[] MIXIN_MARKER =
            MIXIN_ANNOTATION.getBytes(StandardCharsets.UTF_8);

    /** Classes écartées faute de pouvoir exister de ce côté, ou parce que mixin. */
    private static int skippedForSide;

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
     * @param gameDirectory répertoire de jeu, ou {@code null} s'il est inconnu
     * @param dedicatedServer {@code true} sur un serveur dédié, où le code client ne
     *     peut pas être chargé
     * @return les noms internes, par exemple {@code net/minecraft/world/entity/Mob}
     */
    public static Set<String> scan(Path gameDirectory, boolean dedicatedServer) {
        Set<String> targets = new LinkedHashSet<>(16_384);
        Set<String> mixinTargets = new HashSet<>(8_192);
        skippedForSide = 0;
        long startedAt = System.nanoTime();

        if (gameDirectory != null) {
            scanModsDirectory(gameDirectory.resolve("mods"), targets, dedicatedServer,
                    mixinTargets);
        }
        scanLegacyClassPath(targets);

        // En production, le JAR du jeu ne figure pas sur le classpath : FML le localise
        // par son propre mécanisme. Sans cette seconde source, l'énumération ne trouve
        // que les utilitaires de Forge, et rien de ce où passe le temps de tick.
        if (gameDirectory != null) {
            scanGameLibraries(gameDirectory.resolve("libraries").resolve("net")
                    .resolve("minecraft"), targets);
        }

        // Une classe qu'un mixin patche ne peut pas être sondée : nos instructions
        // s'intercalent avant que Mixin cherche ses points d'injection, et un injecteur
        // qui raisonne sur les variables locales ne s'y retrouve plus. Constaté en
        // production sur un @ModifyVariable de ValkyrienSkies, dont l'échec est fatal.
        int protectedFromMixins = 0;
        for (String mixinTarget : mixinTargets) {
            if (targets.remove(mixinTarget)) {
                protectedFromMixins++;
            }
        }

        long elapsedMs = (System.nanoTime() - startedAt) / 1_000_000L;
        LOGGER.info("Cibles d'instrumentation énumérées : {} classes en {} ms"
                + " ({} écartées, absentes de ce côté ; {} laissées à Mixin sur {} cibles"
                + " de mixins déclarées).",
                targets.size(), elapsedMs, skippedForSide, protectedFromMixins,
                mixinTargets.size());
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
                    .forEach(jar ->
                            collectFromArchive(jar, targets, true, false, false, null));
        } catch (IOException | RuntimeException e) {
            LOGGER.warn("Bibliothèques du jeu illisibles sous {} : les classes du jeu ne "
                    + "seront pas sondées.", root);
        }
    }

    /** Énumère toutes les classes des archives de {@code mods}. */
    private static void scanModsDirectory(
            Path modsDirectory, Set<String> targets, boolean dedicatedServer,
            Set<String> mixinTargets) {
        if (!Files.isDirectory(modsDirectory)) {
            // Environnement de développement : les mods viennent du classpath, pas d'un
            // dossier. Ce n'est pas une anomalie.
            return;
        }
        try (DirectoryStream<Path> jars = Files.newDirectoryStream(modsDirectory, "*.jar")) {
            for (Path jar : jars) {
                collectFromArchive(jar, targets, false, true, dedicatedServer, mixinTargets);
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
                collectFromArchive(path, targets, true, false, false, null);
            }
        }
    }

    /**
     * Ajoute les classes d'une archive.
     *
     * @param platformOnly {@code true} pour ne retenir que les paquets de plateforme
     * @param checkSide {@code true} pour inspecter le contenu des classes — classes
     *     mixin toujours, code client sur un serveur dédié
     * @param dedicatedServer {@code true} si le code client doit être écarté
     * @param mixinTargets ensemble à compléter des classes que les mixins patcheront,
     *     ou {@code null} quand l'archive n'est pas inspectée
     */
    private static void collectFromArchive(
            Path archive, Set<String> targets, boolean platformOnly, boolean checkSide,
            boolean dedicatedServer, Set<String> mixinTargets) {
        try (ZipFile zip = new ZipFile(archive.toFile())) {
            Enumeration<? extends ZipEntry> entries = zip.entries();
            while (entries.hasMoreElements()) {
                ZipEntry entry = entries.nextElement();
                String internal = internalName(entry.getName());
                if (internal == null || !accepts(internal, platformOnly)) {
                    continue;
                }
                if (targets.size() >= MAX_TARGETS) {
                    LOGGER.warn("Énumération interrompue à {} cibles : au-delà, le coût "
                            + "de l'instrumentation dépasserait ce qu'elle rapporte.",
                            MAX_TARGETS);
                    return;
                }
                if (checkSide && mustSkip(zip, entry, dedicatedServer, mixinTargets)) {
                    skippedForSide++;
                    continue;
                }
                targets.add(internal);
            }
        } catch (IOException | RuntimeException e) {
            LOGGER.warn("Archive {} illisible : ses classes ne seront pas sondées.", archive);
        }
    }

    /**
     * Indique si une classe référence du code qui n'existe pas sur un serveur dédié.
     *
     * <p>Recherche brute des empreintes dans les octets de la classe : le pool de
     * constantes porte tous les noms de types référencés, et une recherche d'octets y
     * est bien moins coûteuse qu'une analyse du format.
     *
     * <p>En cas de doute — entrée illisible — la classe est écartée. C'est le principe
     * UNKNOWN = CONSERVATIVE : une classe non sondée dégrade la mesure, une classe
     * sondée à tort empêche le serveur de démarrer.
     */
    private static boolean mustSkip(ZipFile zip, ZipEntry entry, boolean dedicatedServer,
            Set<String> mixinTargets) {
        try (InputStream stream = zip.getInputStream(entry)) {
            byte[] bytes = stream.readAllBytes();
            if (contains(bytes, MIXIN_MARKER)) {
                if (mixinTargets != null) {
                    collectMixinTargets(bytes, mixinTargets);
                }
                return true;
            }
            if (!dedicatedServer) {
                return false;
            }
            for (byte[] marker : CLIENT_MARKERS) {
                if (contains(bytes, marker)) {
                    return true;
                }
            }
            return false;
        } catch (IOException | RuntimeException e) {
            return true;
        }
    }

    /**
     * Relève les classes qu'un mixin patchera.
     *
     * <p>L'annotation {@code @Mixin} les désigne de deux façons : {@code value} porte
     * des littéraux de classe, {@code targets} des noms sous forme textuelle — la forme
     * employée quand la cible n'est pas visible à la compilation, ce qui est le cas
     * courant entre mods. Les deux sont relevées.
     *
     * <p>L'analyse saute le code, les tables de débogage et les cartes de pile : seul
     * l'en-tête d'annotations est lu. Une classe illisible ne fait rien échouer, mais
     * ses cibles ne seront pas protégées — c'est le seul point de ce filtre où l'inconnu
     * n'est pas traité de façon conservatrice, faute de savoir quoi écarter.
     *
     * <p>Visible dans le paquet pour T-135, qui vérifie les deux formes de désignation.
     */
    static void collectMixinTargets(byte[] bytes, Set<String> mixinTargets) {
        try {
            new ClassReader(bytes).accept(new ClassVisitor(Opcodes.ASM9) {
                @Override
                public AnnotationVisitor visitAnnotation(String descriptor, boolean visible) {
                    if (!MIXIN_ANNOTATION.equals(descriptor)) {
                        return null;
                    }
                    return new AnnotationVisitor(Opcodes.ASM9) {
                        @Override
                        public AnnotationVisitor visitArray(String arrayName) {
                            if (!"value".equals(arrayName) && !"targets".equals(arrayName)) {
                                return null;
                            }
                            return new AnnotationVisitor(Opcodes.ASM9) {
                                @Override
                                public void visit(String elementName, Object value) {
                                    if (value instanceof Type type) {
                                        mixinTargets.add(type.getInternalName());
                                    } else if (value instanceof String target) {
                                        mixinTargets.add(target.replace('.', '/'));
                                    }
                                }
                            };
                        }
                    };
                }
            }, ClassReader.SKIP_CODE | ClassReader.SKIP_DEBUG | ClassReader.SKIP_FRAMES);
        } catch (RuntimeException | LinkageError e) {
            LOGGER.debug("Annotation @Mixin illisible : ses cibles ne seront pas protégées.");
        }
    }

    /** Recherche d'une suite d'octets dans une autre. */
    private static boolean contains(byte[] haystack, byte[] needle) {
        if (needle.length == 0 || haystack.length < needle.length) {
            return false;
        }
        byte first = needle[0];
        int last = haystack.length - needle.length;
        for (int i = 0; i <= last; i++) {
            if (haystack[i] != first) {
                continue;
            }
            int j = 1;
            while (j < needle.length && haystack[i + j] == needle[j]) {
                j++;
            }
            if (j == needle.length) {
                return true;
            }
        }
        return false;
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
