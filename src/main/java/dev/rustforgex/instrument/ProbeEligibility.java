package dev.rustforgex.instrument;

import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodNode;

import java.util.List;

/**
 * C-04 : ce qui peut être sondé, et ce qui ne doit pas l'être.
 *
 * <p>Cahier des charges : PARTIE 5.4. Exigences : R-311 (méthodes trop courtes),
 * R-312 (constructeurs de classe, méthodes natives, méthodes du chargeur de
 * démarrage). Tests : T-132, T-134. Maturité : {@code STABLE}.
 *
 * <p>Le refus est la position par défaut. Une méthode n'est sondée que si l'on peut
 * établir que l'instrumenter est à la fois possible et utile : sonder une méthode dont
 * le corps est plus court que la sonde elle-même coûte plus qu'il ne rapporte, et
 * fausse la mesure qu'on prétend faire.
 */
public final class ProbeEligibility {

    /**
     * Nombre minimal d'instructions réelles pour qu'un sondage vaille la peine (R-311).
     *
     * <p>En deçà, le coût de la sonde domine celui de la méthode : la mesure décrirait
     * surtout l'instrument.
     */
    public static final int MIN_INSTRUCTIONS = 12;

    /**
     * Taille en deçà de laquelle une méthode synchronisée n'est pas sondée (R-312).
     */
    public static final int MIN_SYNCHRONIZED_INSTRUCTIONS = 20;

    /** Préfixes de paquets appartenant au chargeur de démarrage (R-312). */
    private static final List<String> BOOTSTRAP_PACKAGES =
            List.of("java/", "javax/", "jdk/", "sun/", "com/sun/", "org/w3c/", "org/xml/");

    /** Motif d'un refus, pour les diagnostics. */
    public enum Refusal {
        /** La méthode peut être sondée. */
        NONE,
        /** Méthode abstraite ou native : aucun corps à instrumenter. */
        NO_BODY,
        /** Constructeur de classe : exécuté une fois, sous verrou d'initialisation. */
        CLASS_INITIALIZER,
        /** Constructeur d'instance : l'objet n'est pas encore formé. */
        CONSTRUCTOR,
        /** Classe du chargeur de démarrage. */
        BOOTSTRAP_CLASS,
        /** Classe de RUSTFORGE-X : se sonder soi-même fausserait la mesure. */
        OWN_CLASS,
        /** Corps trop court pour que la mesure ait un sens. */
        TOO_SHORT,
        /** Méthode synchronisée et courte. */
        SHORT_SYNCHRONIZED
    }

    private ProbeEligibility() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Détermine si une méthode peut être sondée.
     *
     * @param owner classe propriétaire
     * @param method méthode candidate
     * @return {@link Refusal#NONE} si la méthode peut être sondée, sinon le motif
     */
    public static Refusal evaluate(ClassNode owner, MethodNode method) {
        if ((method.access & (Opcodes.ACC_ABSTRACT | Opcodes.ACC_NATIVE)) != 0) {
            return Refusal.NO_BODY;
        }
        if ("<clinit>".equals(method.name)) {
            return Refusal.CLASS_INITIALIZER;
        }
        if ("<init>".equals(method.name)) {
            // R-312 : envelopper un constructeur dans un try/finally demanderait de
            // traiter l'objet partiellement construit du bloc de rattrapage. Le jeu
            // n'en vaut pas la chandelle en V1.0.
            return Refusal.CONSTRUCTOR;
        }
        if (isBootstrapClass(owner.name)) {
            return Refusal.BOOTSTRAP_CLASS;
        }
        if (owner.name.startsWith("dev/rustforgex/")) {
            return Refusal.OWN_CLASS;
        }

        int instructions = countRealInstructions(method);
        if ((method.access & Opcodes.ACC_SYNCHRONIZED) != 0
                && instructions < MIN_SYNCHRONIZED_INSTRUCTIONS) {
            return Refusal.SHORT_SYNCHRONIZED;
        }
        if (instructions < MIN_INSTRUCTIONS) {
            return Refusal.TOO_SHORT;
        }
        return Refusal.NONE;
    }

    /**
     * Compte les instructions réelles d'une méthode.
     *
     * <p>Les étiquettes, numéros de ligne et cadres ne sont pas des instructions : les
     * compter gonflerait artificiellement des méthodes triviales et ferait sonder
     * exactement ce que R-311 veut écarter.
     *
     * @param method méthode à mesurer
     * @return le nombre d'instructions exécutables
     */
    public static int countRealInstructions(MethodNode method) {
        if (method.instructions == null) {
            return 0;
        }
        int count = 0;
        for (AbstractInsnNode instruction : method.instructions) {
            switch (instruction.getType()) {
                case AbstractInsnNode.LABEL, AbstractInsnNode.LINE, AbstractInsnNode.FRAME -> {
                    // Pseudo-instructions : aucune existence à l'exécution.
                }
                default -> count++;
            }
        }
        return count;
    }

    /**
     * @param internalName nom interne d'une classe
     * @return {@code true} si la classe appartient au chargeur de démarrage
     */
    public static boolean isBootstrapClass(String internalName) {
        return BOOTSTRAP_PACKAGES.stream().anyMatch(internalName::startsWith);
    }
}
