package dev.rustforgex.instrument;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.rustforgex.launch.ProbeIdSource;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

/**
 * Tests de l'échantillonnage de pile (C-05, PARTIE 5.5).
 *
 * <p>Ce qui se teste ici est la partie qui décide : la correspondance entre une trame et
 * une sonde, et le choix de la trame à créditer. La boucle de prélèvement, elle, ne se
 * teste pas utilement — elle dort et appelle {@code getStackTrace}.
 */
class StackSamplerTest {

    private static StackTraceElement frame(String className, String methodName) {
        return new StackTraceElement(className, methodName, "Source.java", 1);
    }

    @Test
    @DisplayName("Une trame connue désigne sa sonde")
    void aKnownFrameResolvesToItsProbe() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/Mob", "tick", 7);

        assertEquals(7, index.probeFor(frame("net.example.Mob", "tick")));
        assertEquals(StackFrameIndex.NO_PROBE, index.probeFor(frame("net.example.Mob", "move")));
        assertEquals(StackFrameIndex.NO_PROBE, index.probeFor(frame("net.example.Other", "tick")));
    }

    /**
     * Une pile ne porte pas le descripteur : deux surcharges y sont indiscernables.
     * Créditer l'une des deux au hasard fausserait durablement le classement des unités
     * de travail, alors que ne rien créditer ne perd qu'un échantillon.
     */
    @Test
    @DisplayName("Deux surcharges rendent la trame inattribuable, jamais devinée")
    void twoOverloadsMakeTheFrameUnattributable() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/Mob", "hurt", 7);
        index.declare("net/example/Mob", "hurt", 9);

        assertEquals(StackFrameIndex.AMBIGUOUS, index.probeFor(frame("net.example.Mob", "hurt")));
        assertEquals(1, index.ambiguousFrames());
    }

    @Test
    @DisplayName("Déclarer deux fois la même sonde ne crée pas d'ambiguïté")
    void declaringTheSameProbeTwiceIsNotAnAmbiguity() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/Mob", "tick", 7);
        index.declare("net/example/Mob", "tick", 7);

        assertEquals(7, index.probeFor(frame("net.example.Mob", "tick")));
        assertEquals(0, index.ambiguousFrames());
    }

    @Test
    @DisplayName("Un identifiant refusé n'entre pas dans l'index")
    void arefusedProbeIsNotIndexed() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/Mob", "tick", ProbeIdSource.NO_PROBE);

        assertEquals(0, index.size());
    }

    @Test
    @DisplayName("La trame créditée est la plus haute qui soit connue")
    void theTopmostKnownFrameIsCredited() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/Deep", "work", 3);
        index.declare("net/example/Caller", "call", 5);

        StackTraceElement[] stack = {
            frame("java.lang.Thread", "getStackTrace"),
            frame("net.example.Deep", "work"),
            frame("net.example.Caller", "call"),
        };

        assertEquals(3, StackSampler.topmostKnownProbe(stack, index),
                "le temps propre revient à ce qui s'exécutait, pas à son appelant");
    }

    @Test
    @DisplayName("Une trame ambiguë n'est pas franchie pour créditer son appelant")
    void anAmbiguousFrameIsNotSteppedOver() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/Deep", "hurt", 3);
        index.declare("net/example/Deep", "hurt", 4);
        index.declare("net/example/Caller", "call", 5);

        StackTraceElement[] stack = {
            frame("net.example.Deep", "hurt"),
            frame("net.example.Caller", "call"),
        };

        assertEquals(StackFrameIndex.AMBIGUOUS, StackSampler.topmostKnownProbe(stack, index),
                "la méthode qui s'exécutait est bien celle-là : créditer l'appelant "
                        + "serait faux");
    }

    @Test
    @DisplayName("Une pile sans aucune trame connue ne crédite rien")
    void anUnknownStackCreditsNothing() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/Mob", "tick", 7);

        StackTraceElement[] stack = {
            frame("java.util.HashMap", "get"),
            frame("net.example.Unknown", "run"),
        };

        assertEquals(StackFrameIndex.NO_PROBE, StackSampler.topmostKnownProbe(stack, index));
    }

    /**
     * Une pile de serveur moddé dépasse couramment la centaine de trames. Au-delà de la
     * profondeur retenue, on ne regarde plus ce qui s'exécute mais ce qui a appelé, et
     * la sonde d'entrée le dit déjà, mieux.
     */
    @Test
    @DisplayName("La profondeur examinée est bornée")
    void theExaminedDepthIsBounded() {
        StackFrameIndex index = new StackFrameIndex();
        index.declare("net/example/TooDeep", "work", 3);

        StackTraceElement[] stack = new StackTraceElement[StackSampler.MAX_DEPTH + 10];
        for (int i = 0; i < stack.length; i++) {
            stack[i] = frame("net.example.Unknown", "run");
        }
        stack[StackSampler.MAX_DEPTH] = frame("net.example.TooDeep", "work");

        assertEquals(StackFrameIndex.NO_PROBE, StackSampler.topmostKnownProbe(stack, index),
                "une trame au-delà de la profondeur retenue n'est pas examinée");
    }

    @Test
    @DisplayName("Un échantillonneur non démarré ne prélève rien")
    void anUnstartedSamplerTakesNothing() {
        StackSampler sampler =
                new StackSampler(new StackFrameIndex(), Thread.currentThread());

        assertTrue(!sampler.running());
        assertEquals(0, sampler.samplesTaken());
        assertEquals(0, sampler.samplesQueued());
    }

    /**
     * Le prélèvement doit cesser quand toutes les sondes sont éteintes : c'est ce qui
     * arrive pendant une pause de mesure de la PARTIE 12.4. Prélever alors attribuerait
     * au jeu un coût qui est le nôtre, et fausserait la ligne de base.
     */
    @Test
    @DisplayName("PARTIE 12.4 : aucune pile n'est prélevée quand tout est éteint")
    void noStackIsTakenWhileEveryProbeIsOff() {
        RfxProbes.uninstall();
        StackSampler sampler =
                new StackSampler(new StackFrameIndex(), Thread.currentThread());

        sampler.sampleOnce();

        assertEquals(0, sampler.samplesTaken(),
                "table vide : le profiler est à l'arrêt ou en pause de mesure");
    }
}
