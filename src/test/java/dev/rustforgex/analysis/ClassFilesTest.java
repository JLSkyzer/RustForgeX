package dev.rustforgex.analysis;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.Opcodes;

import java.io.IOException;
import java.io.InputStream;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Lecture des fichiers {@code .class} et des transformations déclarées (C-07, ADR-051). */
class ClassFilesTest {

    private static byte[] jdkClass(String internalName) throws IOException {
        try (InputStream in = Object.class.getResourceAsStream("/" + internalName + ".class")) {
            assertNotNull(in, internalName);
            return in.readAllBytes();
        }
    }

    @Test
    @DisplayName("L'en-tête donne le nom, la super-classe et les drapeaux")
    void theHeaderGivesNameSuperAndFlags() {
        int access = Opcodes.ACC_PUBLIC | Opcodes.ACC_FINAL | Opcodes.ACC_SUPER;
        ClassHeader header = ClassHeader.parse(TestClasses.plain("a/b/Leaf", "a/b/Base", access));
        assertEquals(new ClassHeader("a/b/Leaf", "a/b/Base", access), header);
    }

    @Test
    @DisplayName("Les classes du JDK se lisent, java/lang/Object n'a pas de super-classe")
    void jdkClassesAreReadable() throws IOException {
        ClassHeader string = ClassHeader.parse(jdkClass("java/lang/String"));
        assertEquals("java/lang/String", string.name());
        assertEquals("java/lang/Object", string.superName());
        assertTrue((string.access() & Opcodes.ACC_FINAL) != 0);
        ClassHeader object = ClassHeader.parse(jdkClass("java/lang/Object"));
        assertEquals("", object.superName());
        ClassHeader list = ClassHeader.parse(jdkClass("java/util/List"));
        assertTrue((list.access() & Opcodes.ACC_INTERFACE) != 0);
    }

    @Test
    @DisplayName("Un contenu qui n'est pas une classe, ou tronqué, est illisible sans exception")
    void garbageAndTruncatedFilesAreUnreadable() {
        byte[] bytes = TestClasses.plain("a/B", "java/lang/Object", Opcodes.ACC_PUBLIC);
        assertNull(ClassHeader.parse(new byte[0]));
        assertNull(ClassHeader.parse("pas une classe".getBytes()));
        for (int length : new int[] {9, 10, 40, bytes.length / 2}) {
            assertNull(ClassHeader.parse(Arrays.copyOf(bytes, length)), "tronqué à " + length);
        }
        byte[] badTag = bytes.clone();
        badTag[10] = 2;
        assertNull(ClassHeader.parse(badTag), "étiquette inconnue");
    }

    @Test
    @DisplayName("Les cibles de @Mixin sont relevées, par classe et par nom")
    void mixinTargetsAreCollected() {
        Set<String> targets = new HashSet<>();
        byte[] mixin = TestClasses.mixin("m/EntityMixin", List.of("a/Entity", "a/Mob"),
                List.of("a.Hidden$Inner", "a/Slashed"));
        assertEquals(MixinTargets.Outcome.MIXIN, MixinTargets.collect(mixin, targets));
        assertEquals(Set.of("a/Entity", "a/Mob", "a/Hidden$Inner", "a/Slashed"), targets,
                "l'annotation imbriquée ailleurs n'est pas relevée");
    }

    @Test
    @DisplayName("Citer le descripteur de @Mixin n'est pas porter l'annotation")
    void mentioningTheDescriptorIsNotAMixin() {
        Set<String> targets = new HashSet<>();
        assertEquals(MixinTargets.Outcome.NOT_MIXIN,
                MixinTargets.collect(TestClasses.mentionsMixin("m/Plugin"), targets));
        assertEquals(MixinTargets.Outcome.NOT_MIXIN, MixinTargets.collect(
                TestClasses.plain("a/B", "java/lang/Object", Opcodes.ACC_PUBLIC), targets));
        assertTrue(targets.isEmpty());
    }

    @Test
    @DisplayName("Une classe mixin tronquée est illisible : ses cibles sont inconnues")
    void aTruncatedMixinIsUnreadable() {
        byte[] mixin = TestClasses.mixin("m/M", List.of("a/Entity"), List.of());
        Set<String> targets = new HashSet<>();
        assertEquals(MixinTargets.Outcome.UNREADABLE,
                MixinTargets.collect(Arrays.copyOf(mixin, mixin.length - 12), targets));
        assertEquals(MixinTargets.Outcome.UNREADABLE,
                MixinTargets.collect(Arrays.copyOf(mixin, mixin.length / 2), targets),
                "réservoir illisible, descripteur présent");
    }

    @Test
    @DisplayName("Access transformers : la classe de chaque ligne, commentaires ignorés")
    void accessTransformerLinesNameTheirClass() {
        Set<String> classes = new HashSet<>();
        TransformedClasses.accessTransformer("""
                # Forge
                public net.minecraft.world.entity.Entity m_20078_()V # commentaire
                public-f net.minecraft.world.level.Level
                protected net.minecraft.client.Outer$Inner f_1234_

                   private-f   a.b.Spaced   *()
                seulement
                """, classes);
        assertEquals(Set.of("net/minecraft/world/entity/Entity", "net/minecraft/world/level/Level",
                "net/minecraft/client/Outer$Inner", "a/b/Spaced"), classes);
    }

    @Test
    @DisplayName("Coremods : les scripts déclarés, puis tout littéral en forme de classe")
    void coreModLiteralsAreOverApproximated() {
        assertEquals(List.of("coremods/field_to_method.js", "/other.js"),
                TransformedClasses.coreModScripts("""
                        {"field_to_method": "coremods/field_to_method.js",
                         "other": "/other.js", "note": "texte"}
                        """));
        Set<String> classes = new HashSet<>();
        TransformedClasses.coreModScript("""
                var ASMAPI = Java.type('net.minecraftforge.coremod.api.ASMAPI');
                function initializeCoreMod() {
                    return { 'biome': { 'target': { 'type': 'CLASS',
                        'name': "net.minecraft.world.level.biome.Biome" } },
                        'inner': { 'names': ['a/b/Outer$In'] } };
                }
                """, classes);
        assertEquals(Set.of("net/minecraftforge/coremod/api/ASMAPI",
                "net/minecraft/world/level/biome/Biome", "a/b/Outer$In"), classes,
                "les littéraux sans point ni barre ne sont pas des classes");
    }
}
