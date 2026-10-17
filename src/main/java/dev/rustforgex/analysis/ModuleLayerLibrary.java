package dev.rustforgex.analysis;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.lang.module.ModuleReader;
import java.lang.module.ModuleReference;
import java.lang.module.ResolvedModule;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileSystemNotFoundException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.ProviderNotFoundException;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.Deque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.IdentityHashMap;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.stream.Stream;

/**
 * Les classes d'une couche de modules et de toutes ses ancêtres.
 *
 * <p>Composant : C-07 (ADR-051). Maturité : {@code STABLE}.
 *
 * <p>Avec Forge, la couche du jeu contient Minecraft et les mods ; ses ancêtres, les
 * bibliothèques, les services de lancement et le JDK. Les parcourir toutes donne la
 * hiérarchie complète de ce que le jeu peut charger, sans rien charger : les archives
 * sont lues par leur {@link ModuleReader}, comme le ferait le chargeur.
 *
 * <p>Un paquet appartient au premier module qui le déclare, couche du jeu d'abord :
 * une classe homonyme plus loin serait masquée, et elle est sautée.
 */
public final class ModuleLayerLibrary implements ClassLibrary {

    private static final String CLASS_SUFFIX = ".class";
    private static final String ACCESS_TRANSFORMER = "META-INF/accesstransformer.cfg";
    private static final String CORE_MODS = "META-INF/coremods.json";

    private final List<ModuleReference> modules;
    private final ModuleReader[] readers;
    private final Map<String, Integer> packageOwners = new HashMap<>();

    private ModuleLayerLibrary(List<ModuleReference> modules) {
        this.modules = List.copyOf(modules);
        this.readers = new ModuleReader[modules.size()];
        for (int index = 0; index < modules.size(); index++) {
            for (String pkg : modules.get(index).descriptor().packages()) {
                packageOwners.putIfAbsent(pkg, index);
            }
        }
    }

    /**
     * @param layer couche de départ, en jeu celle du mod
     * @return la bibliothèque de cette couche et de ses ancêtres
     */
    public static ModuleLayerLibrary of(ModuleLayer layer) {
        List<ModuleReference> modules = new ArrayList<>();
        Set<String> names = new HashSet<>();
        Set<ModuleLayer> seen = Collections.newSetFromMap(new IdentityHashMap<>());
        Deque<ModuleLayer> pending = new ArrayDeque<>(List.of(layer));
        while (!pending.isEmpty()) {
            ModuleLayer current = pending.poll();
            if (!seen.add(current)) {
                continue;
            }
            current.configuration().modules().stream()
                    .sorted(Comparator.comparing(ResolvedModule::name))
                    .filter(module -> names.add(module.name()))
                    .forEach(module -> modules.add(module.reference()));
            pending.addAll(current.parents());
        }
        return new ModuleLayerLibrary(modules);
    }

    /** @return le nombre de modules parcourus */
    public int moduleCount() {
        return modules.size();
    }

    @Override
    public boolean forEachClass(ClassSink sink) throws IOException {
        for (int index = 0; index < modules.size(); index++) {
            ModuleReader reader = reader(index);
            try (Stream<String> entries = entries(index, reader)) {
                Iterator<String> names = entries.iterator();
                while (names.hasNext()) {
                    String entry = names.next();
                    if (!ownsClass(index, entry)) {
                        continue;
                    }
                    Optional<InputStream> stream = reader.open(entry);
                    if (stream.isEmpty()) {
                        continue;
                    }
                    byte[] bytes;
                    try (InputStream in = stream.get()) {
                        bytes = in.readAllBytes();
                    }
                    if (!sink.accept(bytes)) {
                        return false;
                    }
                }
            } catch (UncheckedIOException e) {
                throw e.getCause();
            }
        }
        return true;
    }

    /**
     * Entrées d'un module.
     *
     * <p>Le lecteur des modules de Forge (securejarhandler 2.1.10,
     * {@code JarModuleFinder.JarModuleReader}) renvoie {@code null} pour {@code list()},
     * contre le contrat de {@link ModuleReader}. Son emplacement est alors la racine du
     * système de fichiers du module, que l'on parcourt.
     *
     * @throws IOException si le module ne peut être énuméré par aucune des deux voies
     */
    private Stream<String> entries(int index, ModuleReader reader) throws IOException {
        Stream<String> listed = reader.list();
        if (listed != null) {
            return listed;
        }
        ModuleReference module = modules.get(index);
        URI location = module.location().orElseThrow(() -> new IOException(
                "module " + module.descriptor().name() + " sans emplacement ni liste"));
        Path root;
        try {
            root = Path.of(location);
        } catch (FileSystemNotFoundException | IllegalArgumentException
                | ProviderNotFoundException e) {
            throw new IOException("module " + module.descriptor().name()
                    + " illisible à " + location, e);
        }
        return Files.walk(root)
                .filter(Files::isRegularFile)
                .map(path -> root.relativize(path).toString().replace('\\', '/'))
                .map(name -> name.startsWith("/") ? name.substring(1) : name);
    }

    /** {@code true} si l'entrée est une classe ordinaire que ce module fournit. */
    private boolean ownsClass(int index, String entry) {
        if (!entry.endsWith(CLASS_SUFFIX) || entry.startsWith("META-INF/")
                || entry.endsWith("module-info.class") || entry.endsWith("package-info.class")) {
            return false;
        }
        int slash = entry.lastIndexOf('/');
        if (slash <= 0) {
            return false;
        }
        Integer owner = packageOwners.get(entry.substring(0, slash).replace('/', '.'));
        return owner != null && owner == index;
    }

    @Override
    public byte[] read(String internalName) {
        int slash = internalName.lastIndexOf('/');
        if (slash <= 0) {
            return null;
        }
        Integer owner = packageOwners.get(internalName.substring(0, slash).replace('/', '.'));
        if (owner == null) {
            return null;
        }
        try {
            Optional<InputStream> stream = reader(owner).open(internalName + CLASS_SUFFIX);
            if (stream.isEmpty()) {
                return null;
            }
            try (InputStream in = stream.get()) {
                return in.readAllBytes();
            }
        } catch (IOException | RuntimeException e) {
            return null;
        }
    }

    @Override
    public Set<String> transformedClasses() throws IOException {
        Set<String> classes = new HashSet<>();
        for (int index = 0; index < modules.size(); index++) {
            String accessTransformer = text(index, ACCESS_TRANSFORMER);
            if (accessTransformer != null) {
                TransformedClasses.accessTransformer(accessTransformer, classes);
            }
            String coreMods = text(index, CORE_MODS);
            if (coreMods == null) {
                continue;
            }
            for (String path : TransformedClasses.coreModScripts(coreMods)) {
                String script = text(index, path.startsWith("/") ? path.substring(1) : path);
                if (script == null) {
                    // Un coremod déclaré mais introuvable : ses cibles sont inconnues.
                    throw new IOException("coremod introuvable : " + path);
                }
                TransformedClasses.coreModScript(script, classes);
            }
        }
        return classes;
    }

    /** Contenu texte d'une ressource du module, ou {@code null} si elle n'existe pas. */
    private String text(int index, String resource) throws IOException {
        Optional<InputStream> stream = reader(index).open(resource);
        if (stream.isEmpty()) {
            return null;
        }
        try (InputStream in = stream.get()) {
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
    }

    private ModuleReader reader(int index) throws IOException {
        if (readers[index] == null) {
            readers[index] = modules.get(index).open();
        }
        return readers[index];
    }

    @Override
    public void close() {
        for (int index = 0; index < readers.length; index++) {
            if (readers[index] != null) {
                try {
                    readers[index].close();
                } catch (IOException e) {
                    // Rien à faire : l'archive était ouverte en lecture seule.
                }
                readers[index] = null;
            }
        }
    }
}
