package dev.rustforgex.launch;

import cpw.mods.modlauncher.serviceapi.ILaunchPluginService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodNode;

import java.util.EnumSet;
import java.util.concurrent.atomic.AtomicLong;

/**
 * C-04 : transformateur de bytecode, côté couche de plugins (ADR-017).
 *
 * <p>Cahier des charges : PARTIE 5.4. Exigences : R-310 (la sémantique observable ne
 * change pas), R-311 et R-312 (méthodes écartées), FM-09 (un échec de transformation
 * n'interrompt jamais le chargement). Maturité : {@code STABLE}.
 *
 * <p>C'est le seul point d'où l'on voit passer <strong>toutes</strong> les classes
 * chargées. ModLauncher le découvre par {@code ServiceLoader} dans la couche de
 * plugins, à condition que le JAR ait été promu à cette couche — voir
 * {@link RfxTransformationService}.
 *
 * <h2>Inerte par construction</h2>
 *
 * <p>La couche de plugins est chargée avant les mods : au moment où cette classe est
 * instanciée, ni le runtime natif, ni la configuration, ni même le mod n'existent. Le
 * plugin ne transforme donc <strong>rien</strong> tant que {@link #arm(ProbeIdSource)}
 * n'a pas été appelée depuis la couche du jeu.
 *
 * <p>Les classes chargées avant l'armement ne sont pas sondées. C'est le choix
 * conservateur, et il coûte peu : l'essentiel des classes de jeu se charge
 * paresseusement, bien après le démarrage.
 *
 * <h2>Ce que ce code ne fait jamais</h2>
 *
 * <p>Il ne lève pas. Une exception qui s'échapperait d'ici remonterait dans le
 * chargeur de classes de ModLauncher et empêcherait le jeu de démarrer — pour un
 * défaut de profilage. Toute défaillance est comptée, la classe est rendue telle
 * quelle, et le jeu continue.
 */
public final class RfxLaunchPlugin implements ILaunchPluginService {

    /** Nom du plugin, tel que ModLauncher et sa piste d'audit le désignent. */
    public static final String NAME = "rustforgex";

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-launch");

    /**
     * Phases traitées : {@code AFTER} uniquement.
     *
     * <p>{@code AFTER} suit l'application des {@code ITransformer}, donc le bytecode
     * observé est plus proche de l'état final vu par la JVM — ce qu'exige DM-05.
     */
    private static final EnumSet<Phase> AFTER_ONLY = EnumSet.of(Phase.AFTER);

    private static final EnumSet<Phase> NONE = EnumSet.noneOf(Phase.class);

    /**
     * Fournisseur d'identifiants, ou {@code null} tant que le mod ne l'a pas remis.
     *
     * <p>{@code volatile} : l'armement vient du fil de chargement du mod, les lectures
     * de n'importe quel fil chargeant une classe.
     */
    private static volatile ProbeIdSource probeIds;

    /**
     * {@code true} dès que ModLauncher a instancié ce plugin.
     *
     * <p>Distinguer « la classe est chargeable » de « le plugin est installé » n'est
     * pas un détail : le JAR peut être présent sans avoir rejoint la couche
     * d'amorçage, auquel cas ModLauncher ne l'instancie jamais et aucune classe ne lui
     * est soumise. Armer un plugin dans cet état réussirait sans rien produire, et le
     * mod annoncerait une instrumentation active qui ne sonde rien.
     */
    private static volatile boolean instantiated;

    private static final AtomicLong CLASSES_SEEN = new AtomicLong();
    private static final AtomicLong CLASSES_TRANSFORMED = new AtomicLong();
    private static final AtomicLong METHODS_PROBED = new AtomicLong();
    private static final AtomicLong TRANSFORM_FAILURES = new AtomicLong();

    /**
     * Arme le plugin.
     *
     * <p>Appelée par le mod une fois le runtime natif prêt. Avant cet appel, le plugin
     * laisse passer chaque classe intacte.
     *
     * @param source fournisseur d'identifiants de sonde, jamais {@code null}
     */
    public static void arm(ProbeIdSource source) {
        probeIds = source;
        LOGGER.info("Transformateur armé, exemplaire {}.", identity());
    }

    /**
     * Identité de l'exemplaire chargé de cette classe.
     *
     * <p>Le plugin vit dans la couche de plugins et le mod dans celle du jeu. Si les
     * deux couches en chargeaient chacune un exemplaire, l'armement écrirait dans un
     * état statique que le transformateur ne lirait jamais — panne silencieuse, et la
     * plus difficile à voir : tout semble fonctionner, rien n'est sondé. Cette trace
     * rend les deux exemplaires distinguables dans le journal.
     */
    private static String identity() {
        ClassLoader loader = RfxLaunchPlugin.class.getClassLoader();
        return (loader == null ? "bootstrap" : loader.getName() + "@" + loader.getClass().getSimpleName())
                + "#" + Integer.toHexString(System.identityHashCode(RfxLaunchPlugin.class));
    }

    /** Désarme le plugin : les classes chargées ensuite ne sont plus transformées. */
    public static void disarm() {
        probeIds = null;
    }

    /** @return {@code true} si le mod a remis un fournisseur d'identifiants */
    public static boolean armed() {
        return probeIds != null;
    }

    /**
     * Indique si ModLauncher a réellement installé ce plugin.
     *
     * @return {@code true} si le constructeur a été appelé par {@code ServiceLoader}
     */
    public static boolean installed() {
        return instantiated;
    }

    /** @return le nombre de classes vues passer (`rfx.instr.classes_seen`) */
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
    public String name() {
        return NAME;
    }

    @Override
    public EnumSet<Phase> handlesClass(Type classType, boolean isEmpty) {
        if (isEmpty || probeIds == null) {
            // Classe synthétique sans corps, ou plugin non armé : rien à faire, et le
            // dire tout de suite évite à ModLauncher de construire l'arbre de la classe.
            return NONE;
        }
        CLASSES_SEEN.incrementAndGet();
        if (ProbeEligibility.isBootstrapClass(classType.getInternalName())) {
            return NONE;
        }
        return AFTER_ONLY;
    }

    @Override
    public int processClassWithFlags(
            Phase phase, ClassNode classNode, Type classType, String reason) {

        ProbeIdSource source = probeIds;
        if (source == null || phase != Phase.AFTER) {
            return ComputeFlags.NO_REWRITE;
        }

        try {
            return transform(classNode, source);
        } catch (RuntimeException | LinkageError e) {
            // FM-09 : la classe est rendue telle quelle. Un défaut de RUSTFORGE-X ne
            // doit jamais empêcher une classe de se charger.
            TRANSFORM_FAILURES.incrementAndGet();
            return ComputeFlags.NO_REWRITE;
        }
    }

    /** Instrumente les méthodes éligibles de la classe. */
    private static int transform(ClassNode classNode, ProbeIdSource source) {
        int probed = 0;

        for (MethodNode method : classNode.methods) {
            if (ProbeEligibility.evaluate(classNode, method) != ProbeEligibility.Refusal.NONE) {
                continue;
            }
            int probeId = source.probeIdFor(classNode.name, method.name, method.desc);
            if (probeId == ProbeIdSource.NO_PROBE) {
                continue;
            }
            if (ProbeInjector.inject(classNode, method, probeId)) {
                probed++;
            }
        }

        if (probed == 0) {
            return ComputeFlags.NO_REWRITE;
        }

        CLASSES_TRANSFORMED.incrementAndGet();
        METHODS_PROBED.addAndGet(probed);
        // L'injection ajoute une variable locale et un gestionnaire d'exception : la
        // taille de pile et le nombre de locaux doivent être recalculés. Les cadres le
        // sont aussi, faute de quoi une méthode déjà porteuse de sauts verrait ses
        // `StackMapTable` désynchronisées et la vérification échouerait au chargement.
        return ComputeFlags.COMPUTE_FRAMES | ComputeFlags.COMPUTE_MAXS;
    }

    /**
     * Constructeur public sans argument, exigé par {@code ServiceLoader}.
     *
     * <p>Un service déclaré par un fichier {@code META-INF/services} est instancié par
     * ce constructeur : la forme {@code public static provider()} n'est reconnue que
     * pour les services déclarés dans un {@code module-info}.
     *
     * <p>Il ne fait rien, et c'est voulu. Il s'exécute avant que le jeu, sa
     * configuration ou le runtime natif n'existent : tout état construit ici serait
     * construit trop tôt.
     */
    public RfxLaunchPlugin() {
        instantiated = true;
        LOGGER.info("Plugin de lancement installé, exemplaire {} — en attente d'armement.",
                identity());
    }
}
