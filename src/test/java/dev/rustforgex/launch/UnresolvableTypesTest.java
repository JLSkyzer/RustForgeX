package dev.rustforgex.launch;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;

import java.util.Set;

/**
 * T-136 : une classe référençant un type absent n'est pas désignée comme cible.
 *
 * <p>Composant : C-04. Cahier des charges : PARTIE 5.4. Décision : ADR-019.
 *
 * <p>Désigner une telle classe suffit à la faire réécrire par {@code ClassTransformer},
 * qui recalcule alors ses frames ; ASM demande le super-type commun de types fusionnés,
 * échoue à charger celui qui manque, et ModLauncher journalise en {@code FATAL}. Constaté
 * en production : 180 lignes provoquées par une seule classe d'intégration facultative.
 */
class UnresolvableTypesTest {

    /** L'univers d'une installation d'essai : deux classes, et rien d'autre. */
    private static final Set<String> UNIVERSE =
            Set.of("net/example/Present", "net/example/Uses");

    @Test
    @DisplayName("T-136 : une classe dont tous les types existent est retenue")
    void keepsAClassWhoseTypesAreAllPresent() {
        byte[] bytes = classReferencing("net/example/Present");

        assertFalse(TargetScanner.referencesMissingType(bytes, UNIVERSE));
    }

    @Test
    @DisplayName("T-136 : une classe référençant un type absent est écartée")
    void skipsAClassReferencingAnAbsentType() {
        byte[] bytes = classReferencing("xyz/jpenilla/squaremap/api/Registry");

        assertTrue(TargetScanner.referencesMissingType(bytes, UNIVERSE),
                "c'est exactement le cas qui a produit 180 lignes FATAL en production");
    }

    @Test
    @DisplayName("T-136 : les types de la plateforme Java n'ont pas à figurer dans l'univers")
    void platformTypesAreAlwaysLoadable() {
        // `java/lang/Object` et `java/util/List` ne sont dans aucune archive de
        // l'installation : sans la liste des modules du système, toute classe du
        // modpack serait écartée.
        byte[] bytes = classReferencing("java/util/List");

        assertFalse(TargetScanner.referencesMissingType(bytes, UNIVERSE));
    }

    @Test
    @DisplayName("T-136 : un tableau d'un type absent est reconnu comme absent")
    void arraysAreUnwrapped() {
        byte[] bytes = classReferencing("[Lnet/example/Absent;");

        assertTrue(TargetScanner.referencesMissingType(bytes, UNIVERSE));
    }

    @Test
    @DisplayName("T-136 : un tableau de primitifs ne désigne aucune classe")
    void primitiveArraysDesignateNoClass() {
        byte[] bytes = classReferencing("[[I");

        assertFalse(TargetScanner.referencesMissingType(bytes, UNIVERSE));
    }

    @Test
    @DisplayName("T-136 : des octets illisibles sont réputés référencer un type absent")
    void unreadableBytesAreTreatedAsUnresolvable() {
        // UNKNOWN = CONSERVATIVE : ne pas savoir ce qu'une classe référence interdit de
        // la désigner comme cible.
        assertTrue(TargetScanner.referencesMissingType(new byte[] {1, 2, 3, 4}, UNIVERSE));
    }

    /**
     * Construit une classe qui référence le type donné dans une entrée
     * {@code CONSTANT_Class}.
     *
     * <p>Un {@code checkcast} suffit : c'est l'une des instructions par lesquelles un
     * type entre sur la pile, donc l'un des cas où le calcul de frames peut avoir à le
     * fusionner avec un autre.
     *
     * @param referenced nom interne, ou descripteur de tableau
     * @return les octets de la classe
     */
    private static byte[] classReferencing(String referenced) {
        ClassWriter writer = new ClassWriter(0);
        writer.visit(Opcodes.V17, Opcodes.ACC_PUBLIC, "net/example/Uses", null,
                "java/lang/Object", null);

        MethodVisitor method = writer.visitMethod(
                Opcodes.ACC_PUBLIC | Opcodes.ACC_STATIC, "use",
                "(Ljava/lang/Object;)Ljava/lang/Object;", null, null);
        method.visitCode();
        method.visitVarInsn(Opcodes.ALOAD, 0);
        method.visitTypeInsn(Opcodes.CHECKCAST, referenced);
        method.visitInsn(Opcodes.ARETURN);
        method.visitMaxs(1, 1);
        method.visitEnd();

        writer.visitEnd();
        return writer.toByteArray();
    }
}
