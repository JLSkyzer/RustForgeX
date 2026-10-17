package dev.rustforgex.analysis;

import org.objectweb.asm.AnnotationVisitor;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.FieldVisitor;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;

import java.util.List;

/** Classes fabriquées pour les tests, par ASM : de vrais fichiers {@code .class}. */
final class TestClasses {

    private TestClasses() {
    }

    /**
     * Classe ordinaire, avec un champ constant, une méthode et des constantes longues et
     * doubles dans son réservoir (deux emplacements chacune).
     */
    static byte[] plain(String name, String superName, int access) {
        ClassWriter writer = new ClassWriter(ClassWriter.COMPUTE_MAXS);
        writer.visit(Opcodes.V17, access, name, null, superName, new String[] {"java/io/Serializable"});
        FieldVisitor field = writer.visitField(Opcodes.ACC_STATIC | Opcodes.ACC_FINAL, "LIMIT",
                "J", null, 1_234_567_890_123L);
        field.visitEnd();
        MethodVisitor method = writer.visitMethod(Opcodes.ACC_PUBLIC | Opcodes.ACC_STATIC,
                "value", "()D", null, null);
        method.visitCode();
        method.visitLdcInsn(2.5);
        method.visitInsn(Opcodes.DRETURN);
        method.visitMaxs(0, 0);
        method.visitEnd();
        writer.visitEnd();
        return writer.toByteArray();
    }

    /**
     * Classe mixin : {@code @Mixin(value = {classes}, targets = {names}, priority = 900)},
     * avec une annotation imbriquée ailleurs, qui ne doit pas être relevée.
     */
    static byte[] mixin(String name, List<String> classes, List<String> names) {
        ClassWriter writer = new ClassWriter(ClassWriter.COMPUTE_MAXS);
        writer.visit(Opcodes.V17, Opcodes.ACC_PUBLIC | Opcodes.ACC_ABSTRACT, name, null,
                "java/lang/Object", null);
        AnnotationVisitor other = writer.visitAnnotation("Lexample/Marker;", false);
        AnnotationVisitor nested = other.visitAnnotation("inner", MixinTargets.MIXIN);
        // `visitEnd` sur le tableau : ASM n'écrit sa taille qu'à ce moment, et l'oublier
        // produit une classe malformée.
        AnnotationVisitor nestedTargets = nested.visitArray("targets");
        nestedTargets.visit(null, "not.a.Target");
        nestedTargets.visitEnd();
        nested.visitEnd();
        other.visitEnd();
        AnnotationVisitor mixin = writer.visitAnnotation(MixinTargets.MIXIN, false);
        AnnotationVisitor value = mixin.visitArray("value");
        for (String target : classes) {
            value.visit(null, Type.getObjectType(target));
        }
        value.visitEnd();
        AnnotationVisitor targets = mixin.visitArray("targets");
        for (String target : names) {
            targets.visit(null, target);
        }
        targets.visitEnd();
        mixin.visit("priority", 900);
        mixin.visitEnum("kind", "Lexample/Kind;", "FAST");
        mixin.visitEnd();
        MethodVisitor method = writer.visitMethod(Opcodes.ACC_PRIVATE, "inject", "()V", null,
                null);
        method.visitCode();
        method.visitInsn(Opcodes.RETURN);
        method.visitMaxs(0, 0);
        method.visitEnd();
        writer.visitEnd();
        return writer.toByteArray();
    }

    /** Classe qui cite le descripteur de {@code @Mixin} sans porter l'annotation. */
    static byte[] mentionsMixin(String name) {
        ClassWriter writer = new ClassWriter(ClassWriter.COMPUTE_MAXS);
        writer.visit(Opcodes.V17, Opcodes.ACC_PUBLIC, name, null, "java/lang/Object", null);
        MethodVisitor method = writer.visitMethod(Opcodes.ACC_PUBLIC | Opcodes.ACC_STATIC,
                "descriptor", "()Ljava/lang/String;", null, null);
        method.visitCode();
        method.visitLdcInsn(MixinTargets.MIXIN);
        method.visitInsn(Opcodes.ARETURN);
        method.visitMaxs(0, 0);
        method.visitEnd();
        writer.visitEnd();
        return writer.toByteArray();
    }
}
