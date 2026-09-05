package dev.rustforgex.launch;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.AnnotationVisitor;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;

import java.util.HashSet;
import java.util.Set;

/**
 * T-135 : lecture des cibles déclarées par une classe mixin.
 *
 * <p>Composant : C-04. Cahier des charges : PARTIE 5.4.
 *
 * <p>Ce filtre est ce qui empêche un serveur de production de refuser de démarrer :
 * sonder une classe qu'un mixin patche déplace les points d'injection que Mixin cherche
 * ensuite, et un injecteur qui raisonne sur les variables locales échoue — de façon
 * fatale. Les deux formes de désignation doivent donc être lues, pas seulement la plus
 * courante.
 */
class MixinTargetsTest {

    private static final String MIXIN_DESCRIPTOR = "Lorg/spongepowered/asm/mixin/Mixin;";

    @Test
    @DisplayName("T-135 : les littéraux de classe de @Mixin(value) sont relevés")
    void readsClassLiteralTargets() {
        Set<String> targets = new HashSet<>();
        TargetScanner.collectMixinTargets(
                mixinClass(new String[] {"net/example/Chunk", "net/example/Level"}, null),
                targets);

        assertEquals(Set.of("net/example/Chunk", "net/example/Level"), targets);
    }

    @Test
    @DisplayName("T-135 : les noms textuels de @Mixin(targets) sont relevés et normalisés")
    void readsStringTargets() {
        Set<String> targets = new HashSet<>();
        TargetScanner.collectMixinTargets(
                mixinClass(null, new String[] {"net.example.Hidden"}), targets);

        // La forme textuelle sert quand la cible n'est pas visible à la compilation —
        // le cas courant d'un mod qui patche un autre mod. Elle emploie des points ;
        // l'énumération, elle, travaille sur des noms internes.
        assertEquals(Set.of("net/example/Hidden"), targets);
    }

    @Test
    @DisplayName("T-135 : les deux formes coexistent dans une même annotation")
    void readsBothForms() {
        Set<String> targets = new HashSet<>();
        TargetScanner.collectMixinTargets(
                mixinClass(new String[] {"net/example/Chunk"},
                        new String[] {"net.example.Hidden"}),
                targets);

        assertEquals(Set.of("net/example/Chunk", "net/example/Hidden"), targets);
    }

    @Test
    @DisplayName("T-135 : une classe sans @Mixin ne déclare aucune cible")
    void ignoresPlainClass() {
        Set<String> targets = new HashSet<>();
        TargetScanner.collectMixinTargets(mixinClass(null, null), targets);

        assertTrue(targets.isEmpty(), "une classe ordinaire ne protège rien");
    }

    @Test
    @DisplayName("T-135 : des octets illisibles ne lèvent pas")
    void survivesGarbage() {
        Set<String> targets = new HashSet<>();
        // Une classe illisible ne doit pas interrompre l'énumération : un mod exotique
        // ne peut pas empêcher le jeu de démarrer.
        TargetScanner.collectMixinTargets(new byte[] {1, 2, 3, 4}, targets);

        assertTrue(targets.isEmpty(), "aucune cible ne peut être tirée d'octets illisibles");
    }

    /**
     * Construit une classe portant une annotation {@code @Mixin} synthétique.
     *
     * <p>L'annotation est écrite à la main plutôt que par la vraie classe de Mixin :
     * la dépendance n'existe pas ici, et le filtre ne travaille de toute façon que sur
     * le descripteur.
     *
     * @param classTargets cibles sous forme de littéraux de classe, ou {@code null}
     * @param nameTargets cibles sous forme textuelle, ou {@code null}
     * @return les octets de la classe
     */
    private static byte[] mixinClass(String[] classTargets, String[] nameTargets) {
        ClassWriter writer = new ClassWriter(0);
        writer.visit(Opcodes.V17, Opcodes.ACC_PUBLIC, "net/example/mixin/SomeMixin", null,
                "java/lang/Object", null);

        if (classTargets != null || nameTargets != null) {
            AnnotationVisitor annotation = writer.visitAnnotation(MIXIN_DESCRIPTOR, false);
            if (classTargets != null) {
                AnnotationVisitor array = annotation.visitArray("value");
                for (String target : classTargets) {
                    array.visit(null, Type.getObjectType(target));
                }
                array.visitEnd();
            }
            if (nameTargets != null) {
                AnnotationVisitor array = annotation.visitArray("targets");
                for (String target : nameTargets) {
                    array.visit(null, target);
                }
                array.visitEnd();
            }
            annotation.visitEnd();
        }

        writer.visitEnd();
        return writer.toByteArray();
    }
}
