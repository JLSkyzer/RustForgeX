package dev.rustforgex.analysis;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.ModuleVisitor;
import org.objectweb.asm.Opcodes;

import java.io.IOException;
import java.io.InputStream;
import java.lang.module.Configuration;
import java.lang.module.ModuleFinder;
import java.lang.module.ModuleReader;
import java.lang.module.ModuleReference;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Lecture des classes par couches de modules, comme en jeu (C-07, ADR-051). */
class ModuleLayerLibraryTest {

    private static final int PUBLIC = Opcodes.ACC_PUBLIC | Opcodes.ACC_SUPER;

    /** Module éclaté : {@code module-info.class}, ses classes et ses ressources. */
    private static Path module(Path root, String name, String pkg, Map<String, byte[]> files)
            throws IOException {
        return module(root, name, pkg, files, Map.of());
    }

    /**
     * Module automatique : une archive sans {@code module-info}, nommée d'après son
     * fichier ; renvoie le répertoire qui la contient.
     */
    private static Path automatic(Path dir, String jar, Map<String, byte[]> files)
            throws IOException {
        Files.createDirectories(dir);
        try (java.util.zip.ZipOutputStream out = new java.util.zip.ZipOutputStream(
                Files.newOutputStream(dir.resolve(jar)))) {
            for (Map.Entry<String, byte[]> file : files.entrySet()) {
                out.putNextEntry(new java.util.zip.ZipEntry(file.getKey()));
                out.write(file.getValue());
                out.closeEntry();
            }
        }
        return dir;
    }

    /** Module qui fournit des services : nom du service, puis ses fournisseurs. */
    private static Path module(Path root, String name, String pkg, Map<String, byte[]> files,
            Map<String, List<String>> provides) throws IOException {
        Path dir = root.resolve(name);
        ClassWriter writer = new ClassWriter(0);
        writer.visit(Opcodes.V17, Opcodes.ACC_MODULE, "module-info", null, null, null);
        ModuleVisitor module = writer.visitModule(name, 0, null);
        module.visitRequire("java.base", Opcodes.ACC_MANDATED, null);
        module.visitPackage(pkg);
        for (Map.Entry<String, List<String>> service : provides.entrySet()) {
            module.visitProvide(service.getKey().replace('.', '/'), service.getValue().stream()
                    .map(provider -> provider.replace('.', '/')).toArray(String[]::new));
        }
        module.visitEnd();
        writer.visitEnd();
        Files.createDirectories(dir);
        Files.write(dir.resolve("module-info.class"), writer.toByteArray());
        for (Map.Entry<String, byte[]> file : files.entrySet()) {
            Path path = dir.resolve(file.getKey());
            Files.createDirectories(path.getParent());
            Files.write(path, file.getValue());
        }
        return dir;
    }

    private static ModuleLayer layer(ModuleLayer parent, Path dir, String name) {
        Configuration configuration = parent.configuration()
                .resolve(ModuleFinder.of(dir), ModuleFinder.of(), Set.of(name));
        return parent.defineModulesWithOneLoader(configuration,
                ClassLoader.getSystemClassLoader());
    }

    private static byte[] utf8(String text) {
        return text.getBytes(StandardCharsets.UTF_8);
    }

    @Test
    @DisplayName("Les couches du jeu passent avant leurs ancêtres ; une classe masquée est sautée")
    void theGameLayerShadowsItsAncestors(@TempDir Path root) throws IOException {
        byte[] lower = TestClasses.plain("p/Dup", "java/lang/Object", PUBLIC);
        byte[] upper = TestClasses.plain("p/Dup", "p/Base", PUBLIC);
        ModuleLayer libraries = layer(ModuleLayer.boot(),
                module(root.resolve("a"), "test.lower", "p",
                        Map.of("p/Dup.class", lower, "p/OnlyLower.class",
                                TestClasses.plain("p/OnlyLower", "java/lang/Object", PUBLIC))),
                "test.lower");
        ModuleLayer game = layer(libraries,
                module(root.resolve("b"), "test.upper", "p", Map.of(
                        "p/Dup.class", upper,
                        "p/Base.class", TestClasses.plain("p/Base", "java/lang/Object", PUBLIC),
                        "META-INF/accesstransformer.cfg",
                        utf8("public-f p.Base # finale retirée\n"),
                        "META-INF/coremods.json", utf8("{\"m\": \"scripts/m.js\"}"),
                        "scripts/m.js", utf8("var t = 'p.Patched';"))),
                "test.upper");

        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(game)) {
            assertTrue(library.moduleCount() > 2, "le JDK est parcouru aussi");
            List<String> ours = new ArrayList<>();
            boolean complete = library.forEachClass((bytes, gameLayer) -> {
                ClassHeader header = ClassHeader.parse(bytes);
                if (!header.name().startsWith("p/")) {
                    return false;
                }
                ours.add(header.name() + "<" + header.superName());
                return true;
            });
            assertFalse(complete, "arrêté à la première classe du JDK");
            assertEquals(List.of("p/Base<java/lang/Object", "p/Dup<p/Base"),
                    ours.stream().sorted().toList(),
                    "p appartient au module du jeu : p/OnlyLower et l'autre p/Dup sont masquées");

            assertArrayEquals(upper, library.read("p/Dup"));
            assertNull(library.read("p/OnlyLower"), "paquet masqué, la JVM ne la verrait pas");
            assertNull(library.read("p/Missing"));
            assertNull(library.read("NoPackage"));
            assertEquals("java/lang/String",
                    ClassHeader.parse(library.read("java/lang/String")).name());
            assertEquals(Set.of("p/Base", "p/Patched"), library.transformedClasses());
        }
    }

    /**
     * Référence dont le lecteur renvoie {@code null} pour {@code list()}, comme celui de
     * securejarhandler 2.1.10 en jeu ; son emplacement est la racine du module.
     */
    private static ModuleFinder unlistable(Path dir, String name) {
        ModuleReference real = ModuleFinder.of(dir).find(name).orElseThrow();
        ModuleReference reference = new ModuleReference(real.descriptor(), dir.toUri()) {
            @Override
            public ModuleReader open() throws IOException {
                ModuleReader delegate = real.open();
                return new ModuleReader() {
                    @Override
                    public Optional<URI> find(String resource) throws IOException {
                        return delegate.find(resource);
                    }

                    @Override
                    public Optional<InputStream> open(String resource) throws IOException {
                        return delegate.open(resource);
                    }

                    @Override
                    public Stream<String> list() {
                        return null;
                    }

                    @Override
                    public void close() throws IOException {
                        delegate.close();
                    }
                };
            }
        };
        return new ModuleFinder() {
            @Override
            public Optional<ModuleReference> find(String moduleName) {
                return name.equals(moduleName) ? Optional.of(reference) : Optional.empty();
            }

            @Override
            public Set<ModuleReference> findAll() {
                return Set.of(reference);
            }
        };
    }

    @Test
    @DisplayName("Un lecteur sans liste (Forge) : les entrées viennent de l'emplacement")
    void aReaderWithoutListIsWalkedFromItsLocation(@TempDir Path root) throws IOException {
        Path dir = module(root, "test.forge", "f", Map.of(
                "f/A.class", TestClasses.plain("f/A", "java/lang/Object", PUBLIC),
                "f/B.class", TestClasses.plain("f/B", "f/A", PUBLIC)));
        ModuleLayer boot = ModuleLayer.boot();
        Configuration configuration = boot.configuration()
                .resolve(unlistable(dir, "test.forge"), ModuleFinder.of(), Set.of("test.forge"));
        ModuleLayer game = boot.defineModulesWithOneLoader(configuration,
                ClassLoader.getSystemClassLoader());
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(game)) {
            List<String> ours = new ArrayList<>();
            library.forEachClass((bytes, gameLayer) -> {
                String name = ClassHeader.parse(bytes).name();
                if (!name.startsWith("f/")) {
                    return false;
                }
                ours.add(name);
                return true;
            });
            assertEquals(List.of("f/A", "f/B"), ours.stream().sorted().toList());
        }
    }

    @Test
    @DisplayName("Seules les classes de la couche de départ sont dites du jeu (ADR-059)")
    void onlyTheStartingLayerIsTheGameLayer(@TempDir Path root) throws IOException {
        ModuleLayer libraries = layer(ModuleLayer.boot(),
                module(root.resolve("a"), "test.lib", "l",
                        Map.of("l/L.class", TestClasses.plain("l/L", "java/lang/Object", PUBLIC))),
                "test.lib");
        ModuleLayer game = layer(libraries,
                module(root.resolve("b"), "test.game", "g",
                        Map.of("g/G.class", TestClasses.plain("g/G", "java/lang/Object", PUBLIC))),
                "test.game");
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(game)) {
            List<String> seen = new ArrayList<>();
            library.forEachClass((bytes, gameLayer) -> {
                String name = ClassHeader.parse(bytes).name();
                if (!name.startsWith("l/") && !name.startsWith("g/")) {
                    return false;
                }
                seen.add(name + (gameLayer ? " jeu" : " parente"));
                return true;
            });
            assertEquals(List.of("g/G jeu", "l/L parente"), seen);
        }
    }

    @Test
    @DisplayName("Un service de transformation inconnu arrête l'analyse ; ceux de la plateforme non")
    void anUnknownTransformationServiceIsAnError(@TempDir Path root) throws IOException {
        // Un service tiers est un JAR sans module-info : un module automatique, dont les
        // fournisseurs viennent de META-INF/services.
        ModuleLayer rogue = layer(ModuleLayer.boot(),
                automatic(root.resolve("a"), "test-rogue.jar", Map.of(
                        "x/Rogue.class", TestClasses.plain("x/Rogue", "java/lang/Object", PUBLIC),
                        "META-INF/services/" + TransformationServices.TRANSFORMATION_SERVICE,
                        utf8("x.Rogue"))),
                "test.rogue");
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(rogue)) {
            IOException error = assertThrows(IOException.class, library::transformedClasses);
            assertTrue(error.getMessage().contains("x.Rogue"), error.getMessage());
        }

        String known = "net.minecraftforge.fml.loading.RuntimeDistCleaner";
        assertTrue(TransformationServices.KNOWN.contains(known));
        ModuleLayer platform = layer(ModuleLayer.boot(),
                automatic(root.resolve("b"), "test-platform.jar", Map.of(
                        "net/minecraftforge/fml/loading/RuntimeDistCleaner.class",
                        TestClasses.plain("net/minecraftforge/fml/loading/RuntimeDistCleaner",
                                "java/lang/Object", PUBLIC),
                        "META-INF/services/" + TransformationServices.LAUNCH_PLUGIN,
                        utf8(known))),
                "test.platform");
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(platform)) {
            assertEquals(Set.of(), library.transformedClasses());
        }
    }

    @Test
    @DisplayName("Les cibles d'un coremod lues dans ses fichiers de données comptent (ADR-059)")
    void coreModDataFilesNameTargets(@TempDir Path root) throws IOException {
        ModuleLayer game = layer(ModuleLayer.boot(),
                module(root, "test.data", "q", Map.of(
                        "q/A.class", TestClasses.plain("q/A", "java/lang/Object", PUBLIC),
                        "META-INF/coremods.json", utf8("{\"m\": \"scripts/m.js\"}"),
                        "scripts/m.js", utf8("var t = 'data/targets.json';"
                                + " var n = ASMAPI.loadData(t);"),
                        "data/targets.json", utf8("[\"q/Zombie\", \"q.Raid\"]"))),
                "test.data");
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(game)) {
            // Le nom du fichier lui-même a la forme d'un nom de classe : une cible de plus,
            // inexistante, sans effet.
            Set<String> classes = library.transformedClasses();
            assertTrue(classes.containsAll(Set.of("q/Zombie", "q/Raid")), classes.toString());
        }
    }

    @Test
    @DisplayName("Des données de coremod introuvables ou non nommées : cibles inconnues")
    void missingOrUnnamedCoreModDataIsAnError(@TempDir Path root) throws IOException {
        ModuleLayer missing = layer(ModuleLayer.boot(),
                module(root.resolve("a"), "test.missing", "q", Map.of(
                        "q/A.class", TestClasses.plain("q/A", "java/lang/Object", PUBLIC),
                        "META-INF/coremods.json", utf8("{\"m\": \"m.js\"}"),
                        "m.js", utf8("ASMAPI.loadData('absent.json');"))),
                "test.missing");
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(missing)) {
            assertThrows(IOException.class, library::transformedClasses);
        }
        ModuleLayer unnamed = layer(ModuleLayer.boot(),
                module(root.resolve("b"), "test.unnamed", "r", Map.of(
                        "r/A.class", TestClasses.plain("r/A", "java/lang/Object", PUBLIC),
                        "META-INF/coremods.json", utf8("{\"m\": \"m.js\"}"),
                        "m.js", utf8("var d = ASMAPI.loadData(prefix + suffix);"))),
                "test.unnamed");
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(unnamed)) {
            assertThrows(IOException.class, library::transformedClasses);
        }
    }

    @Test
    @DisplayName("Un coremod déclaré mais introuvable rend les transformations inconnues")
    void aMissingCoreModScriptIsAnError(@TempDir Path root) throws IOException {
        ModuleLayer game = layer(ModuleLayer.boot(),
                module(root, "test.broken", "q", Map.of(
                        "q/A.class", TestClasses.plain("q/A", "java/lang/Object", PUBLIC),
                        "META-INF/coremods.json", utf8("{\"m\": \"absent.js\"}"))),
                "test.broken");
        try (ModuleLayerLibrary library = ModuleLayerLibrary.of(game)) {
            assertThrows(IOException.class, library::transformedClasses);
        }
    }
}
