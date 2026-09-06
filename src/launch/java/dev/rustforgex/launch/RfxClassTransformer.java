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
    private static final AtomicLong CLASSES_TRANSFORMED = new AtomicLong();
    private static final AtomicLong METHODS_PROBED = new AtomicLong();
    private static final AtomicLong TRANSFORM_FAILURES = new AtomicLong();

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

    /** Instrumente les méthodes éligibles de la classe. */
    private static void instrument(ClassNode classNode, ProbeIdSource source) {
        int probed = 0;
        for (MethodNode method : classNode.methods) {
            // Le seuil appliqué ici est celui d'ADR-021, bien au-dessus du plancher
            // normatif : une sonde posée sur une méthode courte coûte plus qu'elle
            // n'apprend, et c'est leur NOMBRE qui fait le surcoût mesuré.
            if (ProbeEligibility.evaluate(classNode, method,
                    ProbeEligibility.DEFAULT_MIN_INSTRUCTIONS)
                    != ProbeEligibility.Refusal.NONE) {
                continue;
            }
            int probeId = source.probeIdFor(classNode.name, method.name, method.desc);
            if (probeId == ProbeIdSource.NO_PROBE) {
                continue;
            }
            if (ProbeInjector.inject(classNode, method, probeId,
                    ProbeEligibility.DEFAULT_MIN_INSTRUCTIONS)) {
                probed++;
            }
        }
        if (probed > 0) {
            CLASSES_TRANSFORMED.incrementAndGet();
            METHODS_PROBED.addAndGet(probed);
        }
    }
}
