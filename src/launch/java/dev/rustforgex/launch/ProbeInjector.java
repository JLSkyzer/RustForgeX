package dev.rustforgex.launch;

import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.InsnList;
import org.objectweb.asm.tree.InsnNode;
import org.objectweb.asm.tree.LabelNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;
import org.objectweb.asm.tree.TryCatchBlockNode;
import org.objectweb.asm.tree.VarInsnNode;

/**
 * C-04 : injection des sondes dans le bytecode.
 *
 * <p>Cahier des charges : PARTIE 5.4. Exigences : R-310 (la sémantique observable ne
 * change pas), R-311 et R-312 (méthodes exclues). Tests : T-130 à T-134.
 * Maturité : {@code STABLE}.
 *
 * <p>La transformation enveloppe le corps de la méthode :
 *
 * <pre>{@code
 * long t0 = RfxProbes.enter(probeId);
 * try {
 *     ... corps d'origine, inchangé ...
 * } finally {
 *     RfxProbes.exit(probeId, t0);
 * }
 * }</pre>
 *
 * <p>Le {@code finally} n'est pas un confort : sans lui, une méthode qui lève ne
 * signalerait jamais sa sortie et fausserait durablement les compteurs. Il est obtenu
 * par un gestionnaire d'exception universel qui rappelle {@code exit} puis relance
 * l'exception d'origine — jamais par un {@code catch} qui l'avalerait.
 *
 * <p>Ce qui n'est <strong>pas</strong> touché : la signature, l'ordre des
 * instructions d'origine, les exceptions déclarées, les blocs de rattrapage existants.
 * Le corps est déplacé sans être réécrit. Une seule variable locale est ajoutée, au-delà
 * de celles que la méthode utilise.
 */
public final class ProbeInjector {

    /** Nom interne de la classe cible des appels injectés. */
    public static final String PROBES_CLASS = "dev/rustforgex/instrument/RfxProbes";

    private static final String ENTER_NAME = "enter";
    private static final String ENTER_DESCRIPTOR = "(I)J";
    private static final String EXIT_NAME = "exit";
    private static final String EXIT_DESCRIPTOR = "(IJ)V";

    private ProbeInjector() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Injecte une sonde dans une méthode.
     *
     * <p>N'a aucun effet si la méthode n'est pas éligible : c'est
     * {@link ProbeEligibility} qui tranche, et son refus fait foi.
     *
     * @param owner classe propriétaire
     * @param method méthode à instrumenter
     * @param probeId identifiant de la sonde
     * @return {@code true} si la méthode a été instrumentée
     */
    public static boolean inject(ClassNode owner, MethodNode method, int probeId) {
        return inject(owner, method, probeId, ProbeEligibility.SPEC_MIN_INSTRUCTIONS);
    }

    /**
     * Injecte une sonde dans une méthode, sous un seuil de taille donné.
     *
     * @param owner classe propriétaire
     * @param method méthode à instrumenter
     * @param probeId identifiant de la sonde
     * @param minInstructions nombre minimal d'instructions réelles exigé (ADR-021)
     * @return {@code true} si la méthode a été instrumentée
     */
    public static boolean inject(
            ClassNode owner, MethodNode method, int probeId, int minInstructions) {
        if (ProbeEligibility.evaluate(owner, method, minInstructions)
                != ProbeEligibility.Refusal.NONE) {
            return false;
        }
        if (method.instructions == null || method.instructions.size() == 0) {
            return false;
        }

        // La variable qui portera l'horodatage d'entrée, au-delà de celles utilisées
        // par la méthode. `maxLocals` est ajusté en conséquence.
        int timestampSlot = method.maxLocals;
        method.maxLocals += Type.LONG_TYPE.getSize();

        LabelNode bodyStart = new LabelNode();
        LabelNode bodyEnd = new LabelNode();
        LabelNode handler = new LabelNode();

        // --- Préambule : t0 = RfxProbes.enter(probeId) ------------------------
        InsnList preamble = new InsnList();
        preamble.add(intConstant(probeId));
        preamble.add(new MethodInsnNode(
                Opcodes.INVOKESTATIC, PROBES_CLASS, ENTER_NAME, ENTER_DESCRIPTOR, false));
        preamble.add(new VarInsnNode(Opcodes.LSTORE, timestampSlot));
        preamble.add(bodyStart);

        // --- Sortie normale : exit avant chaque retour ------------------------
        // Parcours sur une copie du tableau : on insère pendant l'itération.
        for (AbstractInsnNode instruction : method.instructions.toArray()) {
            if (isReturn(instruction.getOpcode())) {
                method.instructions.insertBefore(instruction, exitCall(probeId, timestampSlot));
            }
        }

        // --- Sortie par exception : exit puis relance --------------------------
        InsnList epilogue = new InsnList();
        epilogue.add(bodyEnd);
        epilogue.add(handler);
        // La pile ne porte que l'exception rattrapée ; on la garde pour la relancer.
        epilogue.add(exitCall(probeId, timestampSlot));
        epilogue.add(new InsnNode(Opcodes.ATHROW));

        method.instructions.insert(preamble);
        method.instructions.add(epilogue);

        // Le gestionnaire couvre tout le corps d'origine. Il est ajouté en dernier :
        // les blocs de rattrapage de la méthode gardent la priorité, donc son
        // comportement observable ne change pas (R-310).
        if (method.tryCatchBlocks == null) {
            method.tryCatchBlocks = new java.util.ArrayList<>();
        }
        method.tryCatchBlocks.add(new TryCatchBlockNode(bodyStart, bodyEnd, handler, null));

        // Le préambule empile un entier puis un long ; l'épilogue une exception, un
        // entier et un long. Trois emplacements suffisent au-delà du besoin d'origine.
        method.maxStack = Math.max(method.maxStack, 3);
        return true;
    }

    /** Construit l'appel {@code RfxProbes.exit(probeId, t0)}. */
    private static InsnList exitCall(int probeId, int timestampSlot) {
        InsnList call = new InsnList();
        call.add(intConstant(probeId));
        call.add(new VarInsnNode(Opcodes.LLOAD, timestampSlot));
        call.add(new MethodInsnNode(
                Opcodes.INVOKESTATIC, PROBES_CLASS, EXIT_NAME, EXIT_DESCRIPTOR, false));
        return call;
    }

    /** Charge un entier constant sous la forme la plus compacte disponible. */
    private static AbstractInsnNode intConstant(int value) {
        if (value >= -1 && value <= 5) {
            return new InsnNode(Opcodes.ICONST_0 + value);
        }
        if (value >= Byte.MIN_VALUE && value <= Byte.MAX_VALUE) {
            return new org.objectweb.asm.tree.IntInsnNode(Opcodes.BIPUSH, value);
        }
        if (value >= Short.MIN_VALUE && value <= Short.MAX_VALUE) {
            return new org.objectweb.asm.tree.IntInsnNode(Opcodes.SIPUSH, value);
        }
        return new org.objectweb.asm.tree.LdcInsnNode(value);
    }

    /** @return {@code true} si l'opcode termine la méthode par un retour */
    private static boolean isReturn(int opcode) {
        return opcode >= Opcodes.IRETURN && opcode <= Opcodes.RETURN;
    }
}
