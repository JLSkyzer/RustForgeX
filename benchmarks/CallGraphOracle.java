import java.io.IOException;
import java.io.InputStream;
import java.lang.invoke.MethodType;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.net.URI;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Deque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;

/**
 * Oracle de l'acceptance de C-07 : confronte chaque site déclaré résolu par l'analyse
 * native à la JVM elle-même (ADR-053).
 *
 * <p>Indépendant de l'analyse : les classes sont chargées par la JVM, sans être
 * initialisées, depuis les mêmes archives que le corpus, dans le même ordre. La
 * hiérarchie vient de {@code Class.getSuperclass} et {@code getInterfaces}, les
 * méthodes de la réflexion. La résolution (JVMS §5.4.3.3, §5.4.3.4), la sélection
 * (§5.4.6) et la redéfinition (§5.4.5, règles d'accès de paquet comprises) sont
 * réécrites ici, sans rien partager avec le code Rust.
 *
 * <p>Quand la réflexion d'une classe échoue — une signature cite un type optionnel
 * absent du classpath, que la JVM ne résoudrait qu'à l'appel —, ses méthodes déclarées
 * sont lues dans son bytecode par ASM, lui aussi étranger au code Rust. La hiérarchie,
 * elle, vient toujours de la JVM.
 *
 * <p>Verdict par site : une cible que la JVM sélectionnerait et que l'analyse n'a pas
 * est un <strong>faux résolu</strong> ; une cible de l'analyse que la JVM ne
 * sélectionnerait pas est une imprécision, prudente ; un site que l'oracle ne peut pas
 * trancher (classe non chargeable) est compté à part.
 *
 * <p>Usage : {@code java -Xmx4g --add-modules ALL-SYSTEM -cp asm.jar
 * CallGraphOracle.java <classpath.txt> <sites.tsv> <rapport-natif.json> <commit>
 * <arbre-modifié> <résultat.json>}
 */
public final class CallGraphOracle {

    /** Seuil du cahier des charges (PARTIE 5.7, acceptance) : taux de résolution. */
    private static final double MIN_RESOLUTION_PCT = 80.0;

    private final ClassLoader loader;
    private final Map<String, Class<?>> loaded = new HashMap<>();
    private final Map<String, String> unloadable = new HashMap<>();
    private final Map<Class<?>, List<Class<?>>> children = new HashMap<>();
    private final Map<Class<?>, Map<String, Integer>> declared = new HashMap<>();
    private final Map<Class<?>, Set<String>> constructors = new HashMap<>();
    private final Set<String> jdkNames = new HashSet<>();
    private long readByAsm;
    private final Map<Class<?>, List<Class<?>>> receivers = new HashMap<>();

    private CallGraphOracle(ClassLoader loader) {
        this.loader = loader;
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 6) {
            System.err.println("usage : CallGraphOracle <classpath.txt> <sites.tsv> "
                    + "<rapport-natif.json> <commit> <arbre-modifie> <resultat.json>");
            System.exit(2);
        }
        List<Path> archives = new ArrayList<>();
        for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            Path path = Path.of(line.trim());
            if (!archives.contains(path) && Files.exists(path)) {
                archives.add(path);
            }
        }
        URL[] urls = new URL[archives.size()];
        for (int i = 0; i < urls.length; i++) {
            urls[i] = archives.get(i).toUri().toURL();
        }
        try (URLClassLoader loader = new URLClassLoader(urls,
                ClassLoader.getPlatformClassLoader())) {
            CallGraphOracle oracle = new CallGraphOracle(loader);
            long started = System.nanoTime();
            oracle.loadUniverse(archives);
            long loadMs = (System.nanoTime() - started) / 1_000_000L;
            Verdict verdict = oracle.check(Path.of(args[1]));
            String nativeReport = Files.readString(Path.of(args[2]), StandardCharsets.UTF_8);
            long resolved = field(nativeReport, "reference_resolved");
            long sites = field(nativeReport, "reference_sites");
            double rate = sites == 0 ? 0.0 : resolved * 100.0 / sites;
            boolean accepted = verdict.falseResolved == 0 && verdict.unverifiable == 0
                    && rate > MIN_RESOLUTION_PCT;
            String json = verdict.toJson(nativeReport.strip(), args[3],
                    Boolean.parseBoolean(args[4]), oracle.loaded.size(),
                    oracle.unloadable.size(), loadMs, oracle.readByAsm, rate, accepted);
            Files.writeString(Path.of(args[5]), json, StandardCharsets.UTF_8);
            System.out.println(verdict.summary());
            System.out.printf(java.util.Locale.ROOT,
                    "taux de resolution des methodes de reference : %.1f %% (seuil %.0f %%)%n",
                    rate, MIN_RESOLUTION_PCT);
            System.out.println(accepted ? "ACCEPTE" : "REFUSE");
            System.exit(accepted ? 0 : 1);
        }
    }

    /** Valeur entière d'un champ du rapport natif (JSON plat écrit par rfx-bench). */
    private static long field(String json, String name) {
        java.util.regex.Matcher m = java.util.regex.Pattern
                .compile("\"" + name + "\"\\s*:\\s*(\\d+)").matcher(json);
        if (!m.find()) {
            throw new IllegalStateException("champ absent du rapport natif : " + name);
        }
        return Long.parseLong(m.group(1));
    }

    // --- Univers : toutes les classes des archives et du JDK, chargées sans init. ---

    private void loadUniverse(List<Path> archives) throws IOException {
        Set<String> names = new LinkedHashSet<>();
        for (Path archive : archives) {
            if (Files.isDirectory(archive)) {
                try (Stream<Path> files = Files.walk(archive)) {
                    files.map(p -> archive.relativize(p).toString().replace('\\', '/'))
                            .forEach(entry -> addName(names, entry));
                }
            } else {
                try (ZipFile zip = new ZipFile(archive.toFile())) {
                    for (ZipEntry entry : Collections.list(zip.entries())) {
                        addName(names, entry.getName());
                    }
                }
            }
        }
        FileSystem jrt = FileSystems.getFileSystem(URI.create("jrt:/"));
        try (Stream<Path> modules = Files.list(jrt.getPath("/modules"))) {
            for (Path module : (Iterable<Path>) modules::iterator) {
                try (Stream<Path> files = Files.walk(module)) {
                    files.map(p -> module.relativize(p).toString())
                            .forEach(entry -> {
                                addName(names, entry);
                                if (entry.endsWith(".class")) {
                                    jdkNames.add(entry.substring(0, entry.length() - 6));
                                }
                            });
                }
            }
        }
        for (String name : names) {
            classOf(name);
        }
        for (Class<?> type : loaded.values()) {
            Class<?> parent = type.getSuperclass();
            if (parent != null) {
                children.computeIfAbsent(parent, k -> new ArrayList<>()).add(type);
            }
            for (Class<?> iface : type.getInterfaces()) {
                children.computeIfAbsent(iface, k -> new ArrayList<>()).add(type);
            }
        }
    }

    private static void addName(Set<String> names, String entry) {
        if (entry.endsWith(".class") && !entry.startsWith("META-INF/")
                && !entry.endsWith("module-info.class")
                && !entry.endsWith("package-info.class")) {
            names.add(entry.substring(0, entry.length() - 6));
        }
    }

    /** Classe de ce nom interne, ou {@code null} si la JVM ne peut pas la charger. */
    private Class<?> classOf(String internalName) {
        Class<?> known = loaded.get(internalName);
        if (known != null || unloadable.containsKey(internalName)) {
            return known;
        }
        String binary = internalName.replace('/', '.');
        try {
            Class<?> type = Class.forName(binary, false, loader);
            loaded.put(internalName, type);
            return type;
        } catch (Throwable e) {
            // Modules du JDK hors du chargeur de plateforme (outils, incubateurs) :
            // le chargeur systeme les voit avec --add-modules ALL-SYSTEM.
            if (jdkNames.contains(internalName)) {
                try {
                    Class<?> type = Class.forName(binary, false,
                            ClassLoader.getSystemClassLoader());
                    loaded.put(internalName, type);
                    return type;
                } catch (Throwable ignored) {
                    // Inchargeable aussi par ce chemin.
                }
            }
            unloadable.put(internalName, e.getClass().getSimpleName());
            return null;
        }
    }

    // --- Membres, par la réflexion. ---

    private static String internal(Class<?> type) {
        return type.getName().replace('.', '/');
    }

    /** Méthodes déclarées, nom et descripteur vers drapeaux (ceux du format de classe). */
    private Map<String, Integer> methods(Class<?> type) {
        return declared.computeIfAbsent(type, t -> {
            Map<String, Integer> found = new HashMap<>();
            try {
                for (Method method : t.getDeclaredMethods()) {
                    String descriptor = MethodType.methodType(method.getReturnType(),
                            method.getParameterTypes()).toMethodDescriptorString();
                    found.put(method.getName() + descriptor, method.getModifiers());
                }
            } catch (LinkageError e) {
                found.clear();
                readMembers(t, found, null);
            }
            return found;
        });
    }

    private Set<String> constructorsOf(Class<?> type) {
        return constructors.computeIfAbsent(type, t -> {
            Set<String> found = new HashSet<>();
            try {
                for (Constructor<?> constructor : t.getDeclaredConstructors()) {
                    found.add(MethodType.methodType(void.class,
                            constructor.getParameterTypes()).toMethodDescriptorString());
                }
            } catch (LinkageError e) {
                found.clear();
                readMembers(t, null, found);
            }
            return found;
        });
    }

    /** Repli : les membres déclarés, lus dans le bytecode par ASM. */
    private void readMembers(Class<?> type, Map<String, Integer> methods,
            Set<String> constructorDescriptors) {
        String resource = internal(type) + ".class";
        ClassLoader source = type.getClassLoader() != null ? type.getClassLoader() : loader;
        try (InputStream in = source.getResourceAsStream(resource)) {
            if (in == null) {
                throw new IllegalStateException("bytecode introuvable : " + resource);
            }
            new ClassReader(in).accept(new ClassVisitor(Opcodes.ASM9) {
                @Override
                public MethodVisitor visitMethod(int access, String name, String descriptor,
                        String signature, String[] exceptions) {
                    if ("<init>".equals(name)) {
                        if (constructorDescriptors != null) {
                            constructorDescriptors.add(descriptor);
                        }
                    } else if (!"<clinit>".equals(name) && methods != null) {
                        methods.put(name + descriptor, access & 0xFFFF);
                    }
                    return null;
                }
            }, ClassReader.SKIP_CODE | ClassReader.SKIP_DEBUG | ClassReader.SKIP_FRAMES);
            readByAsm++;
        } catch (IOException e) {
            throw new IllegalStateException("bytecode illisible : " + resource, e);
        }
    }

    /** Une méthode déclarée : la classe qui la déclare et ses drapeaux. */
    private record Found(Class<?> owner, String name, String descriptor, int modifiers) {
        String key() {
            return internal(owner) + "." + name + descriptor;
        }
    }

    private Found declaredIn(Class<?> type, String name, String descriptor) {
        Integer modifiers = methods(type).get(name + descriptor);
        return modifiers == null ? null : new Found(type, name, descriptor, modifiers);
    }

    // --- JVMS §5.4.3.3 et §5.4.3.4 : résolution. ---

    private Found resolve(Class<?> owner, String name, String descriptor) {
        if ("<init>".equals(name)) {
            return constructorsOf(owner).contains(descriptor)
                    ? new Found(owner, name, descriptor, 0) : null;
        }
        if (!owner.isInterface()) {
            for (Class<?> c = owner; c != null; c = c.getSuperclass()) {
                Found found = declaredIn(c, name, descriptor);
                if (found != null) {
                    return found;
                }
            }
        } else {
            Found found = declaredIn(owner, name, descriptor);
            if (found != null) {
                return found;
            }
            Found inObject = declaredIn(Object.class, name, descriptor);
            if (inObject != null && Modifier.isPublic(inObject.modifiers())) {
                return inObject;
            }
        }
        List<Found> maximal = maximallySpecific(owner, name, descriptor);
        List<Found> concrete = maximal.stream()
                .filter(f -> !Modifier.isAbstract(f.modifiers())).toList();
        if (concrete.size() == 1) {
            return concrete.get(0);
        }
        return maximal.isEmpty() ? null : maximal.get(0);
    }

    /** Méthodes maximalement spécifiques des super-interfaces (JVMS §5.4.3.3). */
    private List<Found> maximallySpecific(Class<?> type, String name, String descriptor) {
        List<Found> candidates = new ArrayList<>();
        for (Class<?> iface : superinterfaces(type)) {
            Found found = declaredIn(iface, name, descriptor);
            if (found != null && !Modifier.isPrivate(found.modifiers())
                    && !Modifier.isStatic(found.modifiers())) {
                candidates.add(found);
            }
        }
        List<Found> maximal = new ArrayList<>();
        for (Found candidate : candidates) {
            boolean dominated = candidates.stream().anyMatch(other ->
                    other.owner() != candidate.owner()
                            && candidate.owner().isAssignableFrom(other.owner()));
            if (!dominated) {
                maximal.add(candidate);
            }
        }
        return maximal;
    }

    private static Set<Class<?>> superinterfaces(Class<?> type) {
        Set<Class<?>> found = new LinkedHashSet<>();
        Deque<Class<?>> pending = new ArrayDeque<>();
        for (Class<?> c = type; c != null; c = c.getSuperclass()) {
            pending.addAll(List.of(c.getInterfaces()));
        }
        while (!pending.isEmpty()) {
            Class<?> iface = pending.poll();
            if (found.add(iface)) {
                pending.addAll(List.of(iface.getInterfaces()));
            }
        }
        return found;
    }

    // --- JVMS §5.4.5 : redéfinition ; §5.4.6 : sélection. ---

    private static boolean samePackage(Class<?> a, Class<?> b) {
        return a.getClassLoader() == b.getClassLoader()
                && a.getPackageName().equals(b.getPackageName());
    }

    /** {@code mC} peut-elle redéfinir {@code mA} (JVMS §5.4.5) ? */
    private boolean canOverride(Found mC, Found mA) {
        if (mC.owner() == mA.owner()) {
            return true;
        }
        if (Modifier.isPrivate(mC.modifiers()) || Modifier.isStatic(mC.modifiers())
                || !mA.owner().isAssignableFrom(mC.owner())) {
            return false;
        }
        int access = mA.modifiers();
        if (Modifier.isPublic(access) || Modifier.isProtected(access)) {
            return true;
        }
        if (Modifier.isPrivate(access)) {
            return false;
        }
        if (samePackage(mC.owner(), mA.owner())) {
            return true;
        }
        // Transitivement, par une méthode intermédiaire distincte des deux.
        for (Class<?> between = mC.owner().getSuperclass();
                between != null && between != mA.owner(); between = between.getSuperclass()) {
            Found middle = declaredIn(between, mA.name(), mA.descriptor());
            if (middle != null && canOverride(mC, middle) && canOverride(middle, mA)) {
                return true;
            }
        }
        return false;
    }

    /** Méthode sélectionnée pour un receveur, ou {@code null} (erreur à l'appel). */
    private Found select(Class<?> receiver, Found resolved) {
        if (Modifier.isPrivate(resolved.modifiers())) {
            return resolved;
        }
        for (Class<?> c = receiver; c != null; c = c.getSuperclass()) {
            Found candidate = declaredIn(c, resolved.name(), resolved.descriptor());
            if (candidate != null && !Modifier.isStatic(candidate.modifiers())
                    && canOverride(candidate, resolved)) {
                return candidate;
            }
        }
        List<Found> concrete = maximallySpecific(receiver, resolved.name(),
                resolved.descriptor()).stream()
                .filter(f -> !Modifier.isAbstract(f.modifiers())).toList();
        return concrete.size() == 1 ? concrete.get(0) : null;
    }

    /** Classes concrètes chargées qui sont {@code owner} ou en descendent. */
    private List<Class<?>> concreteReceivers(Class<?> owner) {
        return receivers.computeIfAbsent(owner, o -> {
            List<Class<?>> found = new ArrayList<>();
            Set<Class<?>> seen = new HashSet<>();
            Deque<Class<?>> pending = new ArrayDeque<>(List.of(o));
            while (!pending.isEmpty()) {
                Class<?> type = pending.poll();
                if (!seen.add(type)) {
                    continue;
                }
                if (!type.isInterface() && !Modifier.isAbstract(type.getModifiers())) {
                    found.add(type);
                }
                pending.addAll(children.getOrDefault(type, List.of()));
            }
            return found;
        });
    }

    // --- Confrontation. ---

    /** Cibles que la JVM peut atteindre depuis ce site, ou une raison de ne pas savoir. */
    private record Expected(Set<String> targets, String unknown) {
    }

    private Expected expected(String kind, String member) {
        int paren = member.indexOf('(');
        int dot = member.lastIndexOf('.', paren);
        String ownerName = member.substring(0, dot);
        String name = member.substring(dot + 1, paren);
        String descriptor = member.substring(paren);
        Class<?> owner = classOf(ownerName);
        if (owner == null) {
            return new Expected(Set.of(), "classe " + ownerName + " non chargeable ("
                    + unloadable.get(ownerName) + ")");
        }
        try {
            Found resolved = resolve(owner, name, descriptor);
            if (resolved == null) {
                return new Expected(Set.of(), "methode " + member + " non resolue par la JVM");
            }
            switch (kind) {
                case "static", "special", "handle6", "handle7", "handle8":
                    return new Expected(Set.of(resolved.key()), null);
                case "virtual", "interface", "handle5", "handle9": {
                    // Privee : selectionnee telle quelle (JVMS §5.4.6). Finale : aucune
                    // classe qui la redefinirait ne passe la verification (§4.10).
                    if (Modifier.isPrivate(resolved.modifiers())
                            || Modifier.isFinal(resolved.modifiers())) {
                        return new Expected(Set.of(resolved.key()), null);
                    }
                    Set<String> targets = new TreeSet<>();
                    for (Class<?> receiver : concreteReceivers(owner)) {
                        Found selected = select(receiver, resolved);
                        if (selected != null) {
                            targets.add(selected.key());
                        }
                    }
                    return new Expected(targets, null);
                }
                default:
                    return new Expected(Set.of(), "nature inconnue " + kind);
            }
        } catch (Throwable e) {
            return new Expected(Set.of(), "reflexion impossible : " + e);
        }
    }

    private Verdict check(Path sites) throws IOException {
        Verdict verdict = new Verdict();
        for (String line : Files.readAllLines(sites, StandardCharsets.UTF_8)) {
            if (line.isBlank()) {
                continue;
            }
            String[] f = line.split("\t", -1);
            String site = f[0] + " @" + f[1] + " " + f[2] + " " + f[3] + " [" + f[4] + "]";
            // `|` : un descripteur contient des `;`.
            Set<String> ours = new TreeSet<>(List.of(f[5].split("\\|")));
            verdict.sites++;
            verdict.byResolution.merge(f[4], 1L, Long::sum);
            Expected expected = expected(f[2], f[3]);
            if (expected.unknown() != null) {
                verdict.unverifiable++;
                verdict.example(verdict.unverifiableExamples, site + " : " + expected.unknown());
                continue;
            }
            Set<String> missing = new TreeSet<>(expected.targets());
            missing.removeAll(ours);
            Set<String> extra = new TreeSet<>(ours);
            extra.removeAll(expected.targets());
            if (!missing.isEmpty()) {
                verdict.falseResolved++;
                verdict.falseByResolution.merge(f[4], 1L, Long::sum);
                verdict.example(verdict.falseExamples,
                        site + " manque " + missing + ", analyse " + ours);
            } else if (!extra.isEmpty()) {
                verdict.imprecise++;
                verdict.example(verdict.impreciseExamples, site + " en trop " + extra);
            } else {
                verdict.exact++;
            }
        }
        return verdict;
    }

    /** Bilan de la confrontation. */
    private static final class Verdict {
        long sites;
        long exact;
        long falseResolved;
        long imprecise;
        long unverifiable;
        final Map<String, Long> byResolution = new TreeMap<>();
        final Map<String, Long> falseByResolution = new TreeMap<>();
        final List<String> falseExamples = new ArrayList<>();
        final List<String> impreciseExamples = new ArrayList<>();
        final List<String> unverifiableExamples = new ArrayList<>();

        void example(List<String> list, String text) {
            if (list.size() < 25) {
                list.add(text);
            }
        }

        String summary() {
            return sites + " sites resolus confrontes a la JVM : " + exact + " exacts, "
                    + falseResolved + " faux resolus, " + imprecise + " avec cibles en trop, "
                    + unverifiable + " non verifiables"
                    + (falseResolved == 0 ? "" : System.lineSeparator() + "  premier : "
                            + falseExamples.get(0));
        }

        String toJson(String nativeReport, String commit, boolean dirty, int loadedClasses,
                int unloadableClasses, long loadMs, long readByAsm, double rate,
                boolean accepted) {
            StringBuilder json = new StringBuilder("{\n");
            json.append("  \"schema\": 1,\n  \"benchmark\": \"callgraph-acceptance\",\n");
            json.append("  \"commit\": ").append(quote(commit)).append(",\n");
            json.append("  \"dirty\": ").append(dirty).append(",\n");
            json.append("  \"native\": ").append(nativeReport.replace("\n", "\n  "))
                    .append(",\n");
            json.append("  \"oracle\": {\n");
            json.append("    \"loaded_classes\": ").append(loadedClasses).append(",\n");
            json.append("    \"unloadable_classes\": ").append(unloadableClasses).append(",\n");
            json.append("    \"load_ms\": ").append(loadMs).append(",\n");
            json.append("    \"members_read_by_asm\": ").append(readByAsm).append(",\n");
            json.append("    \"sites\": ").append(sites).append(",\n");
            json.append("    \"exact\": ").append(exact).append(",\n");
            json.append("    \"false_resolved\": ").append(falseResolved).append(",\n");
            json.append("    \"imprecise\": ").append(imprecise).append(",\n");
            json.append("    \"unverifiable\": ").append(unverifiable).append(",\n");
            json.append("    \"by_resolution\": ").append(map(byResolution)).append(",\n");
            json.append("    \"false_by_resolution\": ").append(map(falseByResolution))
                    .append(",\n");
            json.append("    \"false_examples\": ").append(list(falseExamples)).append(",\n");
            json.append("    \"imprecise_examples\": ").append(list(impreciseExamples))
                    .append(",\n");
            json.append("    \"unverifiable_examples\": ").append(list(unverifiableExamples))
                    .append("\n  },\n");
            json.append("  \"resolution_rate_pct\": ")
                    .append(String.format(java.util.Locale.ROOT, "%.2f", rate)).append(",\n");
            json.append("  \"min_resolution_pct\": ").append(MIN_RESOLUTION_PCT).append(",\n");
            json.append("  \"accepted\": ").append(accepted).append("\n}\n");
            return json.toString();
        }

        private static String map(Map<String, Long> values) {
            StringBuilder out = new StringBuilder("{");
            values.forEach((k, v) -> out.append(out.length() > 1 ? ", " : "")
                    .append(quote(k)).append(": ").append(v));
            return out.append('}').toString();
        }

        private static String list(List<String> values) {
            StringBuilder out = new StringBuilder("[");
            for (String value : values) {
                out.append(out.length() > 1 ? ",\n      " : "\n      ").append(quote(value));
            }
            return out.append(values.isEmpty() ? "]" : "\n    ]").toString();
        }

        private static String quote(String text) {
            StringBuilder out = new StringBuilder("\"");
            for (char c : text.toCharArray()) {
                switch (c) {
                    case '"' -> out.append("\\\"");
                    case '\\' -> out.append("\\\\");
                    case '\n' -> out.append("\\n");
                    case '\t' -> out.append("\\t");
                    default -> {
                        if (c < 0x20) {
                            out.append(String.format("\\u%04x", (int) c));
                        } else {
                            out.append(c);
                        }
                    }
                }
            }
            return out.append('"').toString();
        }
    }
}
