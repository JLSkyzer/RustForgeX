package rfxbench.synth;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.HashMap;
import java.util.Map;

/**
 * Tests du mod synthétique de charge (PARTIE 22, R-870, R-871).
 *
 * <p>Ni Minecraft ni Forge ici : {@link SynthConfig} et {@link SynthWorkload} sont
 * délibérément indépendants des deux, et c'est ce qui les rend vérifiables.
 */
class SynthLoadTest {

    /** Propriétés simulées, sans toucher à celles du processus. */
    private static java.util.function.UnaryOperator<String> props(String... pairs) {
        Map<String, String> values = new HashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            values.put(pairs[i], pairs[i + 1]);
        }
        return values::get;
    }

    /**
     * Le mod reste dans {@code mods} entre deux campagnes. S'il coûtait quoi que ce soit
     * sans réglage, il fausserait toutes les mesures de configuration B.
     */
    @Test
    @DisplayName("R-871 : sans réglage, le mod est inerte")
    void withoutSettingsTheModIsIdle() {
        SynthConfig config = SynthConfig.from(props());

        assertTrue(config.idle());
        assertEquals(0, config.workloads());
        assertEquals(0, config.handlers());
        assertEquals(0, config.threads());
    }

    @Test
    @DisplayName("Une valeur illisible ou négative vaut zéro, jamais un comportement indéfini")
    void anUnreadableOrNegativeValueMeansZero() {
        SynthConfig config = SynthConfig.from(props(
                SynthConfig.WORKLOADS, "beaucoup",
                SynthConfig.ITERATIONS, "-50",
                SynthConfig.THREADS, ""));

        assertEquals(0, config.workloads());
        assertEquals(0, config.iterations());
        assertEquals(0, config.threads());
        assertTrue(config.idle());
    }

    @Test
    @DisplayName("Les réglages demandés sont lus tels quels")
    void requestedSettingsAreReadAsGiven() {
        SynthConfig config = SynthConfig.from(props(
                SynthConfig.WORKLOADS, "64",
                SynthConfig.ITERATIONS, "500",
                SynthConfig.ALLOCATION, "1024",
                SynthConfig.HANDLERS, "40",
                SynthConfig.THREADS, "4",
                SynthConfig.NONDETERMINISTIC, "true"));

        assertFalse(config.idle());
        assertEquals(64, config.workloads());
        assertEquals(500, config.iterations());
        assertEquals(1024, config.allocationBytes());
        assertEquals(40, config.handlers());
        assertEquals(4, config.threads());
        assertTrue(config.nondeterministic());
    }

    /**
     * Un profil qui ne demanderait que des gestionnaires ou que des fils reste une
     * charge : le mod ne doit pas se croire inerte pour autant.
     */
    @Test
    @DisplayName("Des gestionnaires seuls, ou des fils seuls, suffisent à armer le mod")
    void handlersAloneOrThreadsAloneArmTheMod() {
        assertFalse(SynthConfig.from(props(SynthConfig.HANDLERS, "10")).idle());
        assertFalse(SynthConfig.from(props(SynthConfig.THREADS, "2")).idle());
    }

    /**
     * Sans état accumulé, le compilateur supprimerait la boucle et la charge n'existerait
     * que dans le code source — un piège classique des micro-charges.
     */
    @Test
    @DisplayName("Le calcul modifie l'état, donc il ne peut pas être éliminé")
    void theComputationChangesStateSoItCannotBeEliminated() {
        SynthWorkload workload = new SynthWorkload(7);
        long before = workload.accumulator();

        workload.tick(100, 0, false);

        assertNotEquals(before, workload.accumulator());
        assertEquals(1, workload.ticks());
    }

    @Test
    @DisplayName("Deux unités identiques produisent la même suite")
    void twoIdenticalWorkloadsProduceTheSameSequence() {
        // Reproductible : sans cela, deux exécutions d'une campagne ne seraient pas
        // comparables, ce qui est exactement ce que le profil doit garantir.
        SynthWorkload first = new SynthWorkload(3);
        SynthWorkload second = new SynthWorkload(3);

        for (int i = 0; i < 20; i++) {
            first.tick(50, 64, false);
            second.tick(50, 64, false);
        }

        assertEquals(first.accumulator(), second.accumulator());
    }

    @Test
    @DisplayName("Deux unités d'identifiants différents divergent")
    void twoWorkloadsWithDifferentIdsDiverge() {
        SynthWorkload first = new SynthWorkload(1);
        SynthWorkload second = new SynthWorkload(2);

        first.tick(50, 0, false);
        second.tick(50, 0, false);

        assertNotEquals(first.accumulator(), second.accumulator(),
                "des unités indiscernables ne représenteraient qu'une seule unité de travail");
    }

    @Test
    @DisplayName("L'allocation demandée est effectivement faite")
    void therequestedAllocationActuallyHappens() {
        SynthWorkload workload = new SynthWorkload(1);

        workload.tick(10, 800, false);

        // Arrondi au multiple de huit octets : 800 / 8 = 100 cases de huit.
        assertEquals(800, workload.allocatedBytes());
    }

    @Test
    @DisplayName("Sans allocation demandée, rien n'est alloué")
    void withoutRequestedAllocationNothingIsAllocated() {
        SynthWorkload workload = new SynthWorkload(1);

        workload.tick(10, 0, false);

        assertEquals(0, workload.allocatedBytes());
    }

    /**
     * R-870 : un profil doit pouvoir être non déterministe, pour vérifier que
     * RUSTFORGE-X le refuse ou le sérialise plutôt que de le casser.
     */
    @Test
    @DisplayName("R-870 : le mode non déterministe produit des suites différentes")
    void theNondeterministicModeProducesDifferentSequences() {
        SynthWorkload first = new SynthWorkload(5);
        SynthWorkload second = new SynthWorkload(5);

        for (int i = 0; i < 200; i++) {
            first.tick(20, 0, true);
            second.tick(20, 0, true);
        }

        assertNotEquals(first.accumulator(), second.accumulator(),
                "sans divergence, le profil non déterministe ne testerait rien");
    }

    /**
     * ADR-021 a porté le seuil de sondage à soixante-quatre instructions. Une unité plus
     * courte ne serait jamais sondée, et le mod synthétique ne mesurerait rien du coût
     * de l'instrumentation — ce pour quoi il existe.
     */
    @Test
    @DisplayName("ADR-021 : l'unité de travail est assez longue pour être sondée")
    void theWorkloadIsLongEnoughToBeProbed() throws Exception {
        byte[] bytes;
        try (var stream = SynthWorkload.class.getResourceAsStream(
                "/rfxbench/synth/SynthWorkload.class")) {
            bytes = stream.readAllBytes();
        }

        // Le corps de `tick` dépasse largement le seuil : on le vérifie grossièrement
        // par la taille de la classe, faute d'ASM sur ce chemin de test.
        assertTrue(bytes.length > 800,
                "classe de " + bytes.length + " octets : trop courte pour être sondée");
    }
}
