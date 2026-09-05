package dev.rustforgex.launch;

import dev.rustforgex.bridge.FakeNativeBridge;
import dev.rustforgex.instrument.ProbeSink;
import dev.rustforgex.instrument.RfxProbes;
import rfxtest.workload.SampleWorkload;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodNode;

import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests T-130 à T-134 de C-04 (JVM Instrumentation).
 *
 * <p>Ces tests transforment réellement {@link SampleWorkload}, chargent le résultat
 * dans un chargeur de classes isolé, et l'exécutent. Vérifier le bytecode produit sans
 * jamais l'exécuter ne prouverait rien : c'est la JVM qui juge de sa validité, et
 * c'est le comportement observable qu'il faut préserver (R-310).
 */
class ProbeInjectorTest {

    private static final String TARGET = "rfxtest.workload.SampleWorkload";

    /** Chargeur qui définit lui-même la classe transformée, sans déléguer. */
    private static final class TransformingLoader extends ClassLoader {

        private final Map<String, byte[]> classes;

        TransformingLoader(Map<String, byte[]> classes) {
            super(ProbeInjectorTest.class.getClassLoader());
            this.classes = classes;
        }

        @Override
        protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
            byte[] bytes = classes.get(name);
            if (bytes == null) {
                return super.loadClass(name, resolve);
            }
            Class<?> already = findLoadedClass(name);
            if (already != null) {
                return already;
            }
            Class<?> defined = defineClass(name, bytes, 0, bytes.length);
            if (resolve) {
                resolveClass(defined);
            }
            return defined;
        }
    }

    /** Pont simulé fournissant un vrai tampon direct, pour relire les sondes. */
    private static final class BufferBridge extends FakeNativeBridge {

        final ByteBuffer buffer =
                ByteBuffer.allocateDirect(64 * 1024).order(ByteOrder.LITTLE_ENDIAN);

        @Override
        public ByteBuffer probeBufferAcquire(long handle, int threadId) {
            return buffer;
        }
    }

    private BufferBridge bridge;
    private ProbeSink sink;

    @BeforeEach
    void installProbes() {
        bridge = new BufferBridge();
        sink = new ProbeSink(bridge, FakeNativeBridge.HANDLE);
        // Deux sondes déclarées : la 0 en TIMED, la 1 en COUNTER.
        RfxProbes.install(sink, new byte[] {
                ProbeLevel.TIMED.code(), ProbeLevel.COUNTER.code(),
        });
    }

    @AfterEach
    void removeProbes() {
        RfxProbes.uninstall();
    }

    /** Lit le bytecode d'origine de la classe cible. */
    private static ClassNode readTarget() throws IOException {
        String resource = "/" + TARGET.replace('.', '/') + ".class";
        try (InputStream stream = ProbeInjectorTest.class.getResourceAsStream(resource)) {
            assertTrue(stream != null, "bytecode introuvable : " + resource);
            ClassNode node = new ClassNode();
            new ClassReader(stream.readAllBytes()).accept(node, 0);
            return node;
        }
    }

    /**
     * Transforme la classe cible en sondant les méthodes nommées, puis la charge.
     *
     * @param probedMethods méthodes à sonder, associées à leur identifiant de sonde
     * @return la classe transformée, chargée dans un chargeur isolé
     */
    private static Class<?> transformAndLoad(Map<String, Integer> probedMethods) throws Exception {
        ClassNode node = readTarget();
        for (MethodNode method : node.methods) {
            Integer probeId = probedMethods.get(method.name);
            if (probeId != null) {
                assertTrue(ProbeInjector.inject(node, method, probeId),
                        "la méthode " + method.name + " aurait dû être sondée");
            }
        }
        // COMPUTE_FRAMES : les cadres de vérification sont recalculés, c'est la JVM
        // qui validera le résultat au chargement.
        ClassWriter writer = new ClassWriter(ClassWriter.COMPUTE_FRAMES | ClassWriter.COMPUTE_MAXS);
        node.accept(writer);

        Map<String, byte[]> classes = new HashMap<>();
        classes.put(TARGET, writer.toByteArray());
        return new TransformingLoader(classes).loadClass(TARGET);
    }

    /** Instancie la classe transformée et invoque une méthode. */
    private static Object invoke(Class<?> type, Object instance, String name, Class<?>[] signature,
            Object... args) throws Exception {
        Method method = type.getMethod(name, signature);
        try {
            return method.invoke(instance, args);
        } catch (InvocationTargetException e) {
            // On propage la cause réelle : c'est elle que le test veut observer.
            throw (Exception) e.getCause();
        }
    }

    /** Relit les enregistrements écrits dans le tampon. */
    private List<int[]> recordedProbes() {
        List<int[]> records = new ArrayList<>();
        ByteBuffer buffer = bridge.buffer;
        for (int offset = 0; offset + ProbeSink.RECORD_SIZE <= buffer.capacity();
                offset += ProbeSink.RECORD_SIZE) {
            int probeId = buffer.getInt(offset);
            byte kind = buffer.get(offset + 4);
            long timestamp = buffer.getLong(offset + 8);
            if (timestamp == 0) {
                break;
            }
            records.add(new int[] {probeId, kind});
        }
        return records;
    }

    @Test
    @DisplayName("T-130 : une méthode instrumentée rend exactement le même résultat")
    void semanticsArePreservedForAValueReturningMethod() throws Exception {
        SampleWorkload reference = new SampleWorkload();
        Class<?> transformed = transformAndLoad(Map.of("sum", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        for (int n : new int[] {0, 1, 10, 100}) {
            Object actual = invoke(transformed, instance, "sum", new Class<?>[] {int.class}, n);
            assertEquals(reference.sum(n), actual, "sum(" + n + ")");
        }
    }

    @Test
    @DisplayName("T-130 : les retours multiples restent corrects")
    void multipleReturnsArePreserved() throws Exception {
        SampleWorkload reference = new SampleWorkload();
        Class<?> transformed = transformAndLoad(Map.of("classify", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        for (int value : new int[] {-5, 0, 7}) {
            Object actual =
                    invoke(transformed, instance, "classify", new Class<?>[] {int.class}, value);
            assertEquals(reference.classify(value), actual, "classify(" + value + ")");
        }
    }

    @Test
    @DisplayName("T-130 : une exception traverse la sonde sans être modifiée")
    void anExceptionPassesThroughUnchanged() throws Exception {
        Class<?> transformed = transformAndLoad(Map.of("alwaysThrows", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        IllegalStateException thrown = assertThrows(IllegalStateException.class,
                () -> invoke(transformed, instance, "alwaysThrows", new Class<?>[] {int.class}, 1));

        // Le type, le message et la pile d'origine doivent être intacts : la sonde
        // relance l'exception, elle ne la remplace pas.
        assertEquals("échec attendu : 3", thrown.getMessage());
        assertTrue(thrown.getStackTrace().length > 0);
    }

    @Test
    @DisplayName("T-130 : un bloc de rattrapage existant garde la priorité")
    void anExistingCatchBlockStillWins() throws Exception {
        SampleWorkload reference = new SampleWorkload();
        Class<?> transformed = transformAndLoad(Map.of("catchesItsOwn", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        for (int n : new int[] {-1, 0, 3}) {
            Object actual =
                    invoke(transformed, instance, "catchesItsOwn", new Class<?>[] {int.class}, n);
            assertEquals(reference.catchesItsOwn(n), actual, "catchesItsOwn(" + n + ")");
        }
    }

    @Test
    @DisplayName("T-130 : un finally existant s'exécute toujours, dans le même ordre")
    void anExistingFinallyStillRuns() throws Exception {
        Class<?> transformed = transformAndLoad(Map.of("withFinally", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        List<String> trace = new ArrayList<>();
        Object result = invoke(transformed, instance, "withFinally",
                new Class<?>[] {int.class, List.class}, 3, trace);
        assertEquals("ok:1", result);
        assertEquals(List.of("corps", "finally"), trace);

        List<String> failing = new ArrayList<>();
        assertThrows(IllegalStateException.class, () -> invoke(transformed, instance, "withFinally",
                new Class<?>[] {int.class, List.class}, -1, failing));
        assertEquals(List.of("corps", "finally"), failing,
                "le finally d'origine s'exécute aussi sur le chemin d'exception");
    }

    @Test
    @DisplayName("T-130 : une méthode sans valeur de retour reste correcte")
    void aVoidMethodIsPreserved() throws Exception {
        SampleWorkload reference = new SampleWorkload();
        Class<?> transformed = transformAndLoad(Map.of("appendAll", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        List<String> expected = new ArrayList<>();
        reference.appendAll(expected, 4);

        List<String> actual = new ArrayList<>();
        invoke(transformed, instance, "appendAll", new Class<?>[] {List.class, int.class}, actual, 4);

        assertEquals(expected, actual);
    }

    @Test
    @DisplayName("T-130 : une méthode statique est instrumentée sans instance")
    void aStaticMethodIsPreserved() throws Exception {
        Class<?> transformed = transformAndLoad(Map.of("staticWork", 0));
        Object actual = invoke(transformed, null, "staticWork", new Class<?>[] {long.class}, 7L);
        assertEquals(SampleWorkload.staticWork(7L), actual);
    }

    @Test
    @DisplayName("T-131 : la sonde enregistre bien l'entrée et la sortie")
    void theProbeRecordsEntryAndExit() throws Exception {
        Class<?> transformed = transformAndLoad(Map.of("sum", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        invoke(transformed, instance, "sum", new Class<?>[] {int.class}, 10);

        List<int[]> records = recordedProbes();
        assertEquals(1, records.size(), "une sonde TIMED n'écrit qu'à la sortie");
        assertEquals(0, records.get(0)[0], "identifiant de sonde");
        assertEquals(ProbeSink.KIND_EXIT, records.get(0)[1], "nature EXIT");
    }

    @Test
    @DisplayName("T-131 : la sortie est enregistrée même quand la méthode lève")
    void theExitIsRecordedEvenOnException() throws Exception {
        Class<?> transformed = transformAndLoad(Map.of("alwaysThrows", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        assertThrows(IllegalStateException.class,
                () -> invoke(transformed, instance, "alwaysThrows", new Class<?>[] {int.class}, 1));

        List<int[]> records = recordedProbes();
        assertEquals(1, records.size(),
                "sans le finally, une méthode qui lève ne signalerait jamais sa sortie");
        assertEquals(ProbeSink.KIND_EXIT, records.get(0)[1]);
    }

    @Test
    @DisplayName("Une sonde éteinte n'écrit rien, alors que le bytecode est en place")
    void aDisabledProbeWritesNothing() throws Exception {
        RfxProbes.setLevels(new byte[] {ProbeLevel.OFF.code()});
        Class<?> transformed = transformAndLoad(Map.of("sum", 0));
        Object instance = transformed.getDeclaredConstructor().newInstance();

        Object result = invoke(transformed, instance, "sum", new Class<?>[] {int.class}, 10);

        assertEquals(new SampleWorkload().sum(10), result, "le résultat reste juste");
        assertTrue(recordedProbes().isEmpty(), "aucun enregistrement pour une sonde éteinte");
    }

    @Test
    @DisplayName("T-132 : une méthode non éligible est laissée intacte")
    void anIneligibleMethodIsLeftAlone() throws Exception {
        ClassNode node = readTarget();
        MethodNode tooShort = node.methods.stream()
                .filter(m -> "tooShort".equals(m.name))
                .findFirst()
                .orElseThrow();

        int before = tooShort.instructions.size();
        assertFalse(ProbeInjector.inject(node, tooShort, 0), "R-311 : méthode trop courte");
        assertEquals(before, tooShort.instructions.size(), "le bytecode ne doit pas bouger");
    }

    @Test
    @DisplayName("T-134 : le constructeur de classe n'est jamais sondé")
    void theClassInitializerIsNeverProbed() throws Exception {
        ClassNode node = readTarget();
        MethodNode fake = new MethodNode(org.objectweb.asm.Opcodes.ACC_STATIC, "<clinit>", "()V",
                null, null);
        assertEquals(ProbeEligibility.Refusal.CLASS_INITIALIZER,
                ProbeEligibility.evaluate(node, fake));
        assertFalse(ProbeInjector.inject(node, fake, 0));
    }

    @Test
    @DisplayName("R-312 : constructeurs, méthodes natives et classes du socle sont refusés")
    void structurallyIneligibleMethodsAreRefused() throws Exception {
        ClassNode node = readTarget();

        MethodNode constructor = new MethodNode(0, "<init>", "()V", null, null);
        assertEquals(ProbeEligibility.Refusal.CONSTRUCTOR,
                ProbeEligibility.evaluate(node, constructor));

        MethodNode nativeMethod = new MethodNode(
                org.objectweb.asm.Opcodes.ACC_NATIVE, "nativeCall", "()V", null, null);
        assertEquals(ProbeEligibility.Refusal.NO_BODY,
                ProbeEligibility.evaluate(node, nativeMethod));

        MethodNode abstractMethod = new MethodNode(
                org.objectweb.asm.Opcodes.ACC_ABSTRACT, "abstractCall", "()V", null, null);
        assertEquals(ProbeEligibility.Refusal.NO_BODY,
                ProbeEligibility.evaluate(node, abstractMethod));

        ClassNode bootstrap = new ClassNode();
        bootstrap.name = "java/util/ArrayList";
        MethodNode any = node.methods.stream()
                .filter(m -> "sum".equals(m.name))
                .findFirst()
                .orElseThrow();
        assertEquals(ProbeEligibility.Refusal.BOOTSTRAP_CLASS,
                ProbeEligibility.evaluate(bootstrap, any));

        ClassNode own = new ClassNode();
        own.name = "dev/rustforgex/instrument/Something";
        assertEquals(ProbeEligibility.Refusal.OWN_CLASS, ProbeEligibility.evaluate(own, any));
    }

    @Test
    @DisplayName("Les instructions comptées excluent étiquettes, lignes et cadres")
    void onlyRealInstructionsAreCounted() throws Exception {
        ClassNode node = readTarget();
        MethodNode sum = node.methods.stream()
                .filter(m -> "sum".equals(m.name))
                .findFirst()
                .orElseThrow();

        int real = ProbeEligibility.countRealInstructions(sum);
        assertTrue(real >= ProbeEligibility.MIN_INSTRUCTIONS,
                "sum doit être éligible, obtenu " + real + " instructions");
        assertNotEquals(sum.instructions.size(), real,
                "le corps contient des pseudo-instructions qui ne doivent pas compter");
    }
}
