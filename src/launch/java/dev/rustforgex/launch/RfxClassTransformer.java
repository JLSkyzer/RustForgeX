package dev.rustforgex.launch;

import cpw.mods.modlauncher.api.ITransformer;
import cpw.mods.modlauncher.api.ITransformerVotingContext;
import cpw.mods.modlauncher.api.TransformerVoteResult;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodNode;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.Collections;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.atomic.AtomicLong;

/**
 * C-04 : transformateur de bytecode (ADR-019).
 *
 * <p>Cahier des charges : PARTIE 5.4. Exigences : R-310 (la sémantique observable ne
 * change pas), R-311 et R-312 (méthodes écartées), FM-09 (un échec de transformation
 * n'interrompt jamais le chargement). Maturité : {@code STABLE}.
 *
 * <p>ModLauncher demande ses transformateurs à notre {@code ITransformationService}, y
 * compris quand celui-ci a été découvert dans {@code mods}. C'est la seule voie
 * supportée depuis un JAR déposé par un joueur — voir ADR-019, qui remplace sur ce
 * point le plugin de lancement d'ADR-017.
 *
 * <h2>Inerte par construction</h2>
 *
 * <p>Le transformateur est enregistré très tôt, avant que le mod, sa configuration ou le
 * runtime natif n'existent. Il ne transforme donc <strong>rien</strong> tant que
 * {@link #arm(ProbeIdSource)} n'a pas été appelée depuis la couche du jeu. Les classes
 * chargées avant l'armement ne sont pas sondées : c'est le choix conservateur, et il
 * coûte peu, l'essentiel des classes de jeu se chargeant paresseusement.
 *
 * <h2>Ce que ce code ne fait jamais</h2>
 *
 * <p>Il ne lève pas. Une exception qui s'échapperait d'ici remonterait dans le chargeur
 * de classes et empêcherait le jeu de démarrer — pour un défaut de profilage. Toute
 * défaillance est comptée, la classe est rendue telle quelle, et le jeu continue.
 */
public final class RfxClassTransformer implements ITransformer<ClassNode> {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-launch");

    /**
     * Fournisseur d'identifiants, ou {@code null} tant que le mod ne l'a pas remis.
     *
     * <p>{@code volatile} : l'armement vient du fil de chargement du mod, les lectures
     * de n'importe quel fil chargeant une classe.
     */
    private static volatile ProbeIdSource probeIds;

    /**
     * {@code true} dès que ModLauncher a réclamé nos cibles.
     *
     * <p>Distinguer « la classe est chargeable » de « le transformateur est enregistré »
     * n'est pas un détail : c'est en confondant les deux qu'ADR-017 a paru fonctionner
     * pendant une journée alors qu'il ne sondait rien en production. Armer un
     * transformateur que ModLauncher n'a jamais enregistré réussirait sans rien
     * produire.
     */
    private static volatile boolean registered;

    private static final AtomicLong CLASSES_SEEN = new AtomicLong();

    /**
     * Classes passées avant l'armement, donc perdues pour toujours.
     *
     * <p>Une classe ne se charge qu'une fois. Celles qui traversent le transformateur
     * avant qu'il ait des identifiants de sonde à distribuer ressortent inchangées et ne
     * repasseront jamais. Ce compteur dit combien de couverture est hors d'atteinte, et
     * c'est la seule façon de savoir ce que « {@code N} méthodes sondées » vaut
     * réellement.
     */
    private static final AtomicLong CLASSES_MISSED = new AtomicLong();

    /** Méthodes écartées par le seuil retenu, alors que R-311 les autoriserait. */
    private static final AtomicLong REFUSED_BY_THRESHOLD = new AtomicLong();

    /** Méthodes que le plancher normatif de R-311 refuse de toute façon. */
    private static final AtomicLong REFUSED_UNDER_SPEC = new AtomicLong();

    /** Méthodes écartées pour une autre raison : sans corps, constructeur, etc. */
    private static final AtomicLong REFUSED_OTHER = new AtomicLong();

    /**
     * Seuil de sondage demandé, en instructions bytecode.
     *
     * <p>Lu une seule fois, au chargement de cette classe : il gouverne chaque
     * transformation et ne peut pas changer en cours de partie sans rendre le parc de
     * sondes incohérent.
     */
    private static final int MIN_INSTRUCTIONS = readMinInstructions();

    /** Lit le seuil demandé, ou rend celui d'ADR-021 si la propriété est absente. */
    private static int readMinInstructions() {
        try {
            String value = System.getProperty("rustforgex.instrumentation.min_instructions");
            return value == null || value.isBlank()
                    ? ProbeEligibility.DEFAULT_MIN_INSTRUCTIONS
                    : Integer.parseInt(value.trim());
        } catch (NumberFormatException | SecurityException e) {
            // Un réglage illisible ne doit pas changer le comportement en silence : on
            // retombe sur le seuil décidé, qui est celui que les campagnes ont mesuré.
            return ProbeEligibility.DEFAULT_MIN_INSTRUCTIONS;
        }
    }
    private static final AtomicLong CLASSES_TRANSFORMED = new AtomicLong();
    private static final AtomicLong METHODS_PROBED = new AtomicLong();
    /** Échecs de transformation, méthodes et classes confondues. */
    private static final AtomicLong TRANSFORM_FAILURES = new AtomicLong();
    private static final AtomicLong UNPROBEABLE_METHODS = new AtomicLong();

    private final Set<Target> targets;

    /**
     * @param internalNames noms internes des classes à cibler
     */
    RfxClassTransformer(Set<String> internalNames) {
        Set<Target> built = new HashSet<>(Math.max(16, internalNames.size() * 2));
        for (String internalName : internalNames) {
            // ModLauncher désigne ses cibles en notation pointée.
            built.add(Target.targetClass(internalName.replace('/', '.')));
        }
        this.targets = Collections.unmodifiableSet(built);
    }

    /**
     * Arme le transformateur.
     *
     * <p>Appelée par le mod une fois le runtime natif prêt. Avant cet appel, chaque
     * classe est rendue intacte.
     *
     * @param source fournisseur d'identifiants de sonde, jamais {@code null}
     */
    public static void arm(ProbeIdSource source) {
        probeIds = source;
        LOGGER.info("Transformateur armé.");
    }

    /** Désarme : les classes chargées ensuite ne sont plus transformées. */
    public static void disarm() {
        probeIds = null;
    }

    /** @return {@code true} si le mod a remis un fournisseur d'identifiants */
    public static boolean armed() {
        return probeIds != null;
    }

    /**
     * Indique si ModLauncher a réellement enregistré ce transformateur.
     *
     * @return {@code true} si {@link #targets()} a été appelée par ModLauncher
     */
    public static boolean installed() {
        return registered;
    }

    /** @return le nombre de classes soumises au transformateur */
    public static long classesMissed() {
        return CLASSES_MISSED.get();
    }

    /** @return le nombre de classes vues passer depuis l'armement */
    public static long classesSeen() {
        return CLASSES_SEEN.get();
    }

    /** @return le nombre de classes effectivement transformées */
    public static long classesTransformed() {
        return CLASSES_TRANSFORMED.get();
    }

    /** @return le nombre de méthodes sondées (`rfx.instr.methods_probed`) */
    public static long methodsProbed() {
        return METHODS_PROBED.get();
    }

    /** @return le nombre d'échecs de transformation (`rfx.instr.transform_failures`) */
    public static long transformFailures() {
        return TRANSFORM_FAILURES.get();
    }

    @Override
    public Set<Target> targets() {
        // ModLauncher n'appelle cette méthode qu'en enregistrant le transformateur :
        // c'est la preuve que nous serons consultés, et la seule qui vaille.
        registered = true;
        return targets;
    }

    @Override
    public TransformerVoteResult castVote(ITransformerVotingContext context) {
        return TransformerVoteResult.YES;
    }

    @Override
    public ClassNode transform(ClassNode classNode, ITransformerVotingContext context) {
        ProbeIdSource source = probeIds;
        if (source == null) {
            CLASSES_MISSED.incrementAndGet();
            return classNode;
        }
        CLASSES_SEEN.incrementAndGet();
        try {
            instrument(classNode, source);
        } catch (RuntimeException | LinkageError e) {
            // FM-09 : la classe est rendue telle quelle. Un défaut de RUSTFORGE-X ne
            // doit jamais empêcher une classe de se charger.
            TRANSFORM_FAILURES.incrementAndGet();
        }
        return classNode;
    }

    /** Injecteur d'une sonde dans une méthode : {@link ProbeInjector}, ou un essai. */
    @FunctionalInterface
    interface MethodInjector {

        /** Voir {@link ProbeInjector#inject(ClassNode, MethodNode, int, int)}. */
        boolean inject(ClassNode owner, MethodNode method, int probeId, int minInstructions);
    }

    /** Instrumente les méthodes éligibles de la classe. */
    private static void instrument(ClassNode classNode, ProbeIdSource source) {
        instrument(classNode, source, ProbeInjector::inject, minInstructions());
    }

    /**
     * Instrumente les méthodes éligibles de la classe, chacune de façon atomique.
     *
     * <p>FM-09 : une méthode dont l'injection lève est rendue <strong>intacte</strong> et
     * comptée non sondable ; les autres méthodes de la classe restent sondées. L'injection
     * modifie la méthode en place : sans copie préalable, un échec au milieu laissait une
     * méthode à moitié injectée dans une classe qui serait chargée quand même — et que la
     * JVM pouvait refuser. La copie ne coûte que pour les méthodes effectivement sondées.
     *
     * @param threshold seuil d'instructions — {@link #minInstructions()} en production
     * @return le nombre de méthodes sondées
     */
    static int instrument(ClassNode classNode, ProbeIdSource source, MethodInjector injector,
            int threshold) {
        int probed = 0;
        for (int index = 0; index < classNode.methods.size(); index++) {
            MethodNode method = classNode.methods.get(index);
            // Le seuil appliqué ici est celui d'ADR-021, bien au-dessus du plancher
            // normatif : une sonde posée sur une méthode courte coûte plus qu'elle
            // n'apprend, et c'est leur NOMBRE qui fait le surcoût mesuré.
            ProbeEligibility.Refusal refusal =
                    ProbeEligibility.evaluate(classNode, method, threshold);
            if (refusal != ProbeEligibility.Refusal.NONE) {
                census(refusal, method);
                continue;
            }
            int probeId = source.probeIdFor(classNode.name, method.name, method.desc);
            if (probeId == ProbeIdSource.NO_PROBE) {
                continue;
            }
            MethodNode intact = copyOf(method);
            try {
                if (injector.inject(classNode, method, probeId, threshold)) {
                    probed++;
                }
            } catch (RuntimeException | LinkageError e) {
                classNode.methods.set(index, intact);
                UNPROBEABLE_METHODS.incrementAndGet();
                TRANSFORM_FAILURES.incrementAndGet();
            }
        }
        if (probed > 0) {
            CLASSES_TRANSFORMED.incrementAndGet();
            METHODS_PROBED.addAndGet(probed);
        }
        return probed;
    }

    /** Copie complète d'une méthode : instructions, blocs d'exception, variables, cadres. */
    static MethodNode copyOf(MethodNode method) {
        MethodNode copy = new MethodNode(method.access, method.name, method.desc,
                method.signature,
                method.exceptions == null ? null : method.exceptions.toArray(new String[0]));
        method.accept(copy);
        return copy;
    }

    /**
     * @return les méthodes dont l'injection a échoué, rendues intactes et non sondées
     *     (FM-09)
     */
    public static long unprobeableMethods() {
        return UNPROBEABLE_METHODS.get();
    }

    /**
     * Compte les méthodes écartées, et par quelle bande de taille.
     *
     * <p>Le seuil d'ADR-021 écarte des méthodes, et jusqu'ici <strong>personne ne
     * savait lesquelles ni combien</strong>. Décider d'un seuil sans connaître la
     * population qu'il coupe, c'est le choisir par raisonnement — exactement ce que la
     * todo reproche à ADR-021 depuis son écriture.
     *
     * <p>Les bandes séparent ce que le plancher normatif refuse de toute façon (moins
     * de douze instructions, R-311) de ce que le seuil retenu écarte en plus.
     */
    private static void census(ProbeEligibility.Refusal refusal, MethodNode method) {
        if (refusal != ProbeEligibility.Refusal.TOO_SHORT) {
            REFUSED_OTHER.incrementAndGet();
            return;
        }
        int size = ProbeEligibility.countRealInstructions(method);
        if (size < ProbeEligibility.SPEC_MIN_INSTRUCTIONS) {
            REFUSED_UNDER_SPEC.incrementAndGet();
        } else {
            REFUSED_BY_THRESHOLD.incrementAndGet();
        }
    }

    /**
     * Seuil de sondage effectif, en instructions bytecode.
     *
     * <p>Lu une fois dans une propriété système, et non dans le fichier de
     * configuration : ce transformateur est enregistré par ModLauncher bien avant que
     * le répertoire de jeu soit connu, et il transforme des classes avant que le mod
     * n'existe. Une campagne le règle donc par
     * {@code -Drustforgex.instrumentation.min_instructions=N}.
     *
     * <p>Ramené au plancher normatif de R-311 s'il est plus bas : le cahier des charges
     * interdit de sonder sous douze instructions, et un réglage ne prime pas sur une
     * exigence.
     */
    public static int minInstructions() {
        int configured = MIN_INSTRUCTIONS;
        return Math.max(configured, ProbeEligibility.SPEC_MIN_INSTRUCTIONS);
    }

    /** @return les méthodes écartées par le seuil retenu, au-dessus du plancher R-311 */
    public static long refusedByThreshold() {
        return REFUSED_BY_THRESHOLD.get();
    }

    /** @return les méthodes que R-311 refuse de toute façon */
    public static long refusedUnderSpec() {
        return REFUSED_UNDER_SPEC.get();
    }

    /** @return les méthodes écartées pour toute autre raison */
    public static long refusedOther() {
        return REFUSED_OTHER.get();
    }
}
