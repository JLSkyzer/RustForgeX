package dev.rustforgex.instrument;

import dev.rustforgex.launch.ProbeEligibility;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests du recensement des trames inconnues (C-05, ADR-027).
 *
 * <p>Ce que ce recensement doit produire est une <strong>liste de candidats au
 * sondage</strong>. Un classement qui déborderait de méthodes qu'on n'a pas le droit
 * d'instrumenter serait joli et inutilisable : c'est le premier point vérifié.
 */
class UnknownFrameIndexTest {

    private static StackTraceElement frame(String className, String methodName) {
        return new StackTraceElement(className, methodName, "Source.java", 1);
    }

    private static StackTraceElement nativeFrame(String className, String methodName) {
        // Un numéro de ligne à -2 est la convention de la JVM pour une méthode native.
        return new StackTraceElement(className, methodName, null, -2);
    }

    @Test
    @DisplayName("Une méthode vue plusieurs fois monte dans le classement")
    void aMethodSeenOftenRisesInTheRanking() {
        UnknownFrameIndex index = new UnknownFrameIndex();
        index.record(frame("net.example.Chunk", "tick"));
        index.record(frame("net.example.Chunk", "tick"));
        index.record(frame("net.example.Mob", "move"));

        List<UnknownFrameIndex.Sighting> top = index.top(10);

        assertEquals(2, top.size());
        assertEquals("net.example.Chunk#tick", top.get(0).label());
        assertEquals(2L, top.get(0).samples());
        assertEquals(1L, top.get(1).samples());
        assertEquals(3L, index.recorded());
        assertEquals(2, index.distinct());
    }

    /**
     * Un classement qui change d'ordre d'un appel à l'autre sans que rien n'ait bougé
     * fait douter de tout le reste. À égalité, l'ordre est celui des noms.
     */
    @Test
    @DisplayName("À égalité d'échantillons, l'ordre est stable")
    void tiesAreBrokenStably() {
        UnknownFrameIndex index = new UnknownFrameIndex();
        index.record(frame("net.example.Zeta", "tick"));
        index.record(frame("net.example.Alpha", "tick"));

        assertEquals("net.example.Alpha#tick", index.top(10).get(0).label());
        assertEquals("net.example.Alpha#tick", index.top(10).get(0).label());
    }

    @Test
    @DisplayName("La limite demandée est respectée")
    void theRequestedLimitIsHonoured() {
        UnknownFrameIndex index = new UnknownFrameIndex();
        for (int i = 0; i < 20; i++) {
            index.record(frame("net.example.C" + i, "tick"));
        }

        assertEquals(5, index.top(5).size());
        assertTrue(index.top(0).isEmpty());
    }

    /**
     * R-660 : une table pleine cesse d'apprendre, mais doit le dire. Un classement
     * tronqué qui s'annonce reste utilisable ; un classement tronqué qui se tait ment.
     */
    @Test
    @DisplayName("Une table pleine continue de compter ce qu'elle connaît, et le dit")
    void afullTableKeepsCountingWhatItKnowsAndSaysSo() {
        UnknownFrameIndex index = new UnknownFrameIndex();
        for (int i = 0; i < UnknownFrameIndex.CAPACITY; i++) {
            index.record(frame("net.example.C" + i, "tick"));
        }
        assertEquals(0L, index.distinctDropped());

        index.record(frame("net.example.TropTard", "tick"));
        index.record(frame("net.example.C0", "tick"));

        assertEquals(UnknownFrameIndex.CAPACITY, index.distinct());
        assertEquals(1L, index.distinctDropped());
        assertEquals(2L, index.top(1).get(0).samples(), "la méthode connue compte encore");
    }

    @Test
    @DisplayName("Le recensement se vide pour mesurer une situation, pas une moyenne")
    void thecensusClearsToMeasureASituationRatherThanAnAverage() {
        UnknownFrameIndex index = new UnknownFrameIndex();
        index.record(frame("net.example.Chunk", "tick"));

        index.reset();

        assertEquals(0, index.distinct());
        assertEquals(0L, index.recorded());
        assertTrue(index.top(10).isEmpty());
    }

    @Test
    @DisplayName("R-312 : ni constructeur, ni méthode native, ni classe de démarrage")
    void whatCannotBeProbedIsNotACandidate() {
        assertFalse(UnknownFrameIndex.isCandidate(frame("net.example.Mob", "<init>")));
        assertFalse(UnknownFrameIndex.isCandidate(frame("net.example.Mob", "<clinit>")));
        assertFalse(UnknownFrameIndex.isCandidate(nativeFrame("net.example.Mob", "tick")));
        assertFalse(UnknownFrameIndex.isCandidate(frame("java.util.HashMap", "get")));
        assertFalse(UnknownFrameIndex.isCandidate(
                frame("dev.rustforgex.instrument.RfxProbes", "enter")),
                "se recenser soi-même n'apprendrait rien");

        assertTrue(UnknownFrameIndex.isCandidate(frame("net.minecraft.world.Level", "tick")));
        assertTrue(UnknownFrameIndex.isCandidate(frame("net.example.Mob", "lambda$tick$0")),
                "une lambda est une vraie méthode, et se sonde");
    }

    /**
     * Le critère est écrit deux fois : en noms internes dans le plugin de lancement, en
     * noms pointés ici — {@code dev.rustforgex.instrument} ne doit pas dépendre d'un JAR
     * qui peut être absent (ADR-017). Deux écritures divergent, sauf si un test les tient
     * ensemble. C'est ce test.
     */
    @Test
    @DisplayName("Le critère de candidature dit la même chose que ProbeEligibility")
    void thecandidateRuleAgreesWithProbeEligibility() {
        String[] excluded = {
            "java/lang/String", "javax/naming/Context", "jdk/internal/Foo",
            "sun/misc/Unsafe", "com/sun/tools/Bar", "org/w3c/dom/Node",
            "org/xml/sax/Parser"
        };
        for (String internal : excluded) {
            assertTrue(ProbeEligibility.isBootstrapClass(internal),
                    internal + " devrait être refusée par ProbeEligibility");
            assertFalse(UnknownFrameIndex.isCandidateClass(internal.replace('/', '.')),
                    internal + " devrait être refusée par UnknownFrameIndex");
        }

        String[] allowed = {
            "net/minecraft/world/level/Level", "com/example/mod/Machine",
            "org/example/Thing", "javassist/Loader"
        };
        for (String internal : allowed) {
            assertFalse(ProbeEligibility.isBootstrapClass(internal),
                    internal + " devrait être acceptée par ProbeEligibility");
            assertTrue(UnknownFrameIndex.isCandidateClass(internal.replace('/', '.')),
                    internal + " devrait être acceptée par UnknownFrameIndex");
        }
    }
}
