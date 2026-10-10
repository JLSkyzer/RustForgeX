package dev.rustforgex;

import com.mojang.logging.LogUtils;
import dev.rustforgex.bench.MacroRecorder;
import dev.rustforgex.command.RfxCommands;
import dev.rustforgex.forge.EventDispatchTable;
import dev.rustforgex.forge.EventObserver;
import dev.rustforgex.forge.ForgeModSource;
import dev.rustforgex.forge.HookGuard;
import dev.rustforgex.forge.ModDiscovery;
import dev.rustforgex.forge.TickCycle;
import dev.rustforgex.instrument.Instrumentation;
import dev.rustforgex.instrument.ProbeRegistry;
import dev.rustforgex.instrument.RfxProbes;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.RegisterCommandsEvent;
import net.minecraftforge.event.server.ServerStartedEvent;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.event.server.ServerStoppingEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.IEventBus;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.event.lifecycle.FMLCommonSetupEvent;
import net.minecraftforge.fml.event.lifecycle.FMLLoadCompleteEvent;
import net.minecraftforge.fml.javafmlmod.FMLJavaModLoadingContext;
import net.minecraftforge.fml.loading.FMLEnvironment;
import net.minecraftforge.fml.loading.FMLPaths;
import net.minecraftforge.versions.forge.ForgeVersion;
import org.slf4j.Logger;

import java.nio.file.Path;

/**
 * C-01 : point d'ancrage unique entre Forge et RUSTFORGE-X.
 *
 * <p>Cahier des charges : PARTIE 5.1. Tests : T-100 à T-103. Maturité : {@code STABLE}.
 *
 * <p>Ce composant démarre le runtime (C-02), vérifie la version de Forge (FM-01),
 * délimite la fenêtre de tick (IF-02, via {@link TickCycle}), enregistre
 * {@code /rfx status} (C-38) et arrête proprement le runtime.
 *
 * <p>Les accroches de tick sont posées au jalon M1, celui de l'observation : elles ont
 * désormais quelque chose à mesurer. Elles se contentent d'ouvrir et de fermer la
 * fenêtre — aucun travail du jeu n'est modifié, aucune tâche n'est encore soumise.
 *
 * <p>Chaque accroche est protégée par un {@link HookGuard} : une exception de
 * RUSTFORGE-X ne doit jamais empêcher le jeu de démarrer ou de tourner (FM-02).
 */
@Mod(RustForgeX.MODID)
public class RustForgeX {

    /** Identifiant du mod. DOIT correspondre à {@code mod_id} dans gradle.properties. */
    public static final String MODID = "rustforgex";

    private static final Logger LOGGER = LogUtils.getLogger();

    /**
     * Classes déjà perdues à la construction du mod.
     *
     * <p>Le transformateur est enregistré par ModLauncher bien avant que Forge construise
     * les mods ; il ne peut sonder quoi que ce soit qu'une fois <em>armé</em>, ce qui
     * suppose le runtime natif démarré. Tout ce qui se charge entre les deux ressort
     * inchangé, définitivement. Ce relevé, comparé à celui de l'armement, dit combien
     * avancer le démarrage rapporterait — et combien est hors de toute portée.
     */
    private final long classesMissedAtConstruct;

    private final HookGuard startupGuard = new HookGuard("setup", LOGGER::warn);
    private final HookGuard discoveryGuard = new HookGuard("loadComplete", LOGGER::warn);
    private final HookGuard commandsGuard = new HookGuard("registerCommands", LOGGER::warn);

    /** Préchauffage des commandes : son propre garde, pour ne pas compter contre l'enregistrement. */
    private final HookGuard warmUpGuard = new HookGuard("warmUpCommands", LOGGER::warn);
    private final HookGuard shutdownGuard = new HookGuard("serverStopping", LOGGER::warn);
    private final HookGuard tickPreGuard = new HookGuard("serverTickPre", LOGGER::warn);
    private final HookGuard tickPostGuard = new HookGuard("serverTickPost", LOGGER::warn);

    /**
     * Intervalle, en ticks, de la trace de progression du cycle de tick.
     *
     * <p>Six cents ticks, soit trente secondes de jeu nominal. La trace est émise en
     * {@code DEBUG} : elle ne parle qu'à qui la cherche, et son coût — un modulo par
     * tick — ne se mesure pas. C'est le « journal de tick » que la PARTIE 5.32 prévoit
     * pour la télémétrie.
     */
    private static final long TICK_LOG_INTERVAL = 600;

    /** Construit le mod et s'attache aux deux bus d'événements de Forge. */
    public RustForgeX() {
        this.classesMissedAtConstruct = Instrumentation.classesMissedSoFar();
        IEventBus modBus = FMLJavaModLoadingContext.get().getModEventBus();
        MinecraftForge.EVENT_BUS.register(this);
        // C-36 niveau B : sans la propriété qui l'arme, cet appel ne fait rien et
        // n'abonne personne. Il est ici, et non dans le setup, parce qu'un benchmark
        // doit pouvoir mesurer un serveur où RUSTFORGE-X est désactivé.
        MacroRecorder.armIfRequested();
        // Test de gameplay G-03 : même raison, et c'est même sa condition — la référence
        // est une exécution où RUSTFORGE-X est désactivé (PARTIE 20.3.4).
        dev.rustforgex.bench.DigestRecorder.armIfRequested();
        dev.rustforgex.bench.CommandTiming.armIfRequested();

        if (earlyArmRequested()) {
            logArmingSchedule();
            startupGuard.run(this::startRuntime);
        } else {
            modBus.addListener(this::onCommonSetup);
        }
        modBus.addListener(this::onLoadComplete);
    }

    /**
     * Décide du moment de l'armement, avant tout démarrage.
     *
     * <p>La lecture est isolée dans sa propre garde : la configuration est du fichier,
     * donc de l'entrée-sortie, et un fichier illisible ne doit pas empêcher le mod de
     * se construire. En cas de doute on retombe sur l'ordonnancement mesuré, qui est
     * celui du setup commun.
     *
     * @return {@code true} si {@code instrumentation.early_arm} est demandé
     */
    private boolean earlyArmRequested() {
        try {
            return RfxRuntime.loadConfiguration(FMLPaths.GAMEDIR.get().resolve(MODID))
                    .getBoolean("instrumentation.early_arm");
        } catch (RuntimeException e) {
            LOGGER.warn("RUSTFORGE-X : configuration illisible à la construction ({}). "
                    + "Armement au setup commun, comme par défaut.", e.toString());
            return false;
        }
    }

    /**
     * Démarre le runtime pendant la phase de setup commune.
     *
     * <p>{@code enqueueWork} garantit l'exécution sur le fil principal de chargement,
     * après que Forge a terminé la construction de tous les mods.
     *
     * @param event événement de setup commun
     */
    private void onCommonSetup(final FMLCommonSetupEvent event) {
        event.enqueueWork(() -> startupGuard.run(this::startRuntime));
    }

    /**
     * Ce que coûte le moment de l'armement.
     *
     * <p>Le transformateur est enregistré par ModLauncher dès le lancement, mais il ne
     * sonde rien tant que le runtime natif ne lui a pas donné d'identifiants de sonde.
     * Entre les deux, les classes traversent et ressortent inchangées — définitivement,
     * puisqu'une classe ne se charge qu'une fois.
     *
     * <p>Mesuré sur le serveur de banc, 288 mods : sur 76 327 classes visées,
     * <strong>32 712</strong> passent avant l'armement au setup commun, dont seulement
     * 4 875 avant la construction de ce mod. Les 27 837 restantes se chargent pendant
     * les trente-huit secondes de construction des autres mods — et ce sont
     * précisément celles que l'enregistrement des blocs, entités et blocs-entités fait
     * charger.
     *
     * <p>L'armement anticipé les récupère. Il n'est pas le défaut pour autant : le coût
     * de tick d'ADR-021 a été certifié sur l'ordonnancement tardif, et élargir la
     * surface sondée le rouvre. Le défaut reste donc ce qui est mesuré, et l'option
     * existe pour que la campagne tranche.
     */
    private void logArmingSchedule() {
        LOGGER.debug("Armement anticipé demandé : le runtime démarre à la construction "
                + "du mod. {} classes étaient déjà passées.", classesMissedAtConstruct);
    }

    /**
     * Dresse l'inventaire des mods une fois que Forge a fini de les charger (C-41).
     *
     * <p>{@code FMLLoadCompleteEvent} est le moment que la PARTIE 5.39 désigne : c'est
     * le premier où la liste des mods est complète. Plus tôt, l'inventaire serait
     * partiel ; plus tard, des classes auraient déjà été sondées sans propriétaire.
     *
     * @param event événement de fin de chargement
     */
    private void onLoadComplete(final FMLLoadCompleteEvent event) {
        discoveryGuard.run(this::discoverMods);
    }

    /** Inventorie les mods et journalise ce qui a été trouvé. */
    private void discoverMods() {
        RfxRuntime runtime = RfxRuntime.instance();
        if (runtime == null) {
            // Le runtime n'a pas démarré : rien à quoi rattacher un inventaire.
            return;
        }
        ModDiscovery discovery = runtime.discoverMods(new ForgeModSource());

        // R-621 borne la découverte à 500 ms pour 250 mods. La dire à chaque démarrage
        // est le seul moyen de savoir si elle tient sur une vraie installation, plutôt
        // que sur les 250 mods synthétiques de T-442.
        LOGGER.info("RUSTFORGE-X : {} mods inventoriés en {} ms ({} modules, {} paquets "
                        + "connus). Aucune classe n'a été chargée pour cela.",
                discovery.modCount(), discovery.durationMs(),
                discovery.knownModules(), discovery.knownPackages());
    }

    /**
     * Dit quelle part des sondes n'a pas trouvé de mod propriétaire (C-41).
     *
     * <p>La PARTIE 5.39 accepte C-41 si moins de 5 % des classes chaudes sont en
     * {@code unknown}. Ce chiffre-ci ne prononce pas cette acceptance — il compte des
     * méthodes, pas des classes, et toutes, pas seulement les chaudes — mais il est le
     * seul indicateur disponible que l'attribution fonctionne, et il est mesuré.
     *
     * <p>Relevé <strong>à l'arrêt</strong>, et pas plus tôt. Mesuré à la fin du
     * chargement, il ne portait que sur 160 sondes : l'essentiel des classes se charge
     * pendant la partie, et un taux calculé avant elles ne dirait rien.
     */
    private static void logAttribution(RfxRuntime runtime) {
        ProbeRegistry registry = runtime.instrumentation().registry();
        if (registry == null || registry.requested() == 0L) {
            return;
        }
        LOGGER.info("RUSTFORGE-X : {} sondes demandées à ce stade, dont {} sur une classe "
                        + "non rattachée à un mod ({}%).",
                registry.requested(), registry.unattributed(),
                String.format(java.util.Locale.ROOT, "%.1f", registry.unknownOwnerPct()));
    }

    /** Séquence de démarrage : configuration, vérification de Forge, C-02. */
    private void startRuntime() {
        Path root = FMLPaths.GAMEDIR.get().resolve(MODID);
        boolean clientSide = FMLEnvironment.dist.isClient();
        String forgeVersion = ForgeVersion.getVersion();

        RfxRuntime runtime = RfxRuntime.start(root, clientSide, forgeVersion);

        for (String warning : runtime.warnings()) {
            LOGGER.warn("Configuration : {}", warning);
        }
        for (String step : runtime.report().journal()) {
            LOGGER.debug("Démarrage : {}", step);
        }

        if (runtime.active()) {
            LOGGER.info("RUSTFORGE-X actif ({} ms) : {}",
                    runtime.report().durationMs(), runtime.report().message());
            logInstrumentation(runtime);
        } else {
            // Un démarrage non nominal n'est pas une erreur du jeu : il doit être
            // lisible sans être alarmant, et dire explicitement que rien n'est cassé.
            LOGGER.warn("RUSTFORGE-X inactif ({}) : {} Le jeu tourne normalement, sans RUSTFORGE-X.",
                    runtime.report().state(), runtime.report().message());
        }
    }

    /**
     * Trace périodique du cycle de tick et de l'instrumentation.
     *
     * <p>Elle dit ce que les assertions ne pensent pas à demander : combien de classes
     * sont passées par le transformateur, combien de méthodes en sont ressorties
     * sondées, et combien de fois la table des niveaux a changé. Un profileur qui ne
     * sonde rien et un profileur qui sonde tout produisent le même silence.
     */
    private static void logTickTrace(long ticks, TickCycle cycle) {
        RfxRuntime runtime = RfxRuntime.instance();
        Instrumentation instrumentation = runtime == null ? null : runtime.instrumentation();
        if (instrumentation == null || !instrumentation.armed()) {
            LOGGER.debug("Cycle de tick : {} ticks observés, {} appels refusés.",
                    ticks, cycle.rejectedCalls());
            return;
        }
        LOGGER.debug(
                "Cycle de tick : {} ticks observés, {} appels refusés. Instrumentation : "
                        + "{} classes vues, {} méthodes sondées, {} échecs, "
                        + "{} mises à jour de niveaux. Sondes : {} dans la table, "
                        + "{} armées, puits {}.",
                ticks, cycle.rejectedCalls(),
                instrumentation.classesSeen(), instrumentation.methodsProbed(),
                instrumentation.transformFailures(), cycle.levelUpdates(),
                RfxProbes.probeCount(), RfxProbes.armedCount(),
                RfxProbes.active() ? "installé" : "absent");
        logEvents(runtime);
    }

    /**
     * Trace de l'observation du bus d'événements (C-06, étape 1).
     *
     * <p>Un observateur qui compte zéro événement et un observateur absent produisent le
     * même silence : cette ligne les distingue, comme celle de l'instrumentation.
     */
    private static void logEvents(RfxRuntime runtime) {
        EventObserver observer = runtime.eventObserver();
        if (observer == null || !observer.observing()) {
            return;
        }
        EventDispatchTable table = observer.table();
        LOGGER.debug("Événements : {} distribués sur {} types, {} chronométrés, "
                        + "{} chronométrages abandonnés.",
                table.dispatched(), table.knownTypes(), table.timed(), table.abandoned());
    }

    /**
     * Journalise l'état de l'instrumentation (ADR-017).
     *
     * <p>RUSTFORGE-X est livré en deux fichiers. Un utilisateur qui n'installe que le
     * mod obtient un runtime qui démarre, mesure la machine, répond à
     * {@code /rfx status} — et ne sonde rien. C'est le pire des symptômes, parce qu'il
     * ressemble à un fonctionnement normal : il est dit explicitement, et il est dit
     * que le jeu n'en souffre pas.
     */
    private void logInstrumentation(RfxRuntime runtime) {
        switch (runtime.instrumentation().state()) {
            case ARMED -> LOGGER.info(
                    "RUSTFORGE-X : instrumentation active, les classes chargées ensuite "
                            + "seront sondées. {} classes étaient déjà passées avant "
                            + "l'armement et resteront hors de portée, dont {} avant même "
                            + "la construction du mod.",
                    runtime.instrumentation().classesMissed(), classesMissedAtConstruct);
            case PLUGIN_MISSING -> LOGGER.warn(
                    "RUSTFORGE-X : le fichier « {}-launch.jar » est absent du dossier mods. "
                            + "Aucune méthode ne sera sondée : le mod observe la machine et "
                            + "les ticks, mais pas le code des mods. Le jeu tourne "
                            + "normalement. Installez les deux fichiers pour profiler.",
                    MODID);
            case PLUGIN_NOT_INSTALLED -> LOGGER.warn(
                    "RUSTFORGE-X : « {}-launch.jar » est présent mais n'a pas été chargé "
                            + "par ModLauncher. Aucune méthode ne sera sondée. Vérifiez "
                            + "qu'il est bien dans le dossier mods, à côté du mod, et non "
                            + "dans un sous-dossier. Le jeu tourne normalement.",
                    MODID);
            case RUNTIME_INACTIVE -> LOGGER.debug(
                    "RUSTFORGE-X : instrumentation non armée, le runtime natif est inactif.");
        }
    }

    /**
     * Enregistre les commandes du mod.
     *
     * <p>Priorité la plus haute : l'enregistrement doit avoir lieu avant qu'un autre
     * mod ne puisse perturber le répartiteur (PARTIE 5.1).
     *
     * @param event événement d'enregistrement des commandes
     */
    @SubscribeEvent(priority = EventPriority.HIGHEST)
    public void onRegisterCommands(final RegisterCommandsEvent event) {
        commandsGuard.run(() -> RfxCommands.register(event.getDispatcher()));
    }

    /**
     * Préchauffe les commandes une fois le serveur démarré, pour que leur premier appel
     * reste sous les 5 ms de R-601 (voir {@link RfxCommands#warmUp()}).
     *
     * @param event fin du démarrage du serveur
     */
    @SubscribeEvent
    public void onServerStarted(final ServerStartedEvent event) {
        warmUpGuard.run(() -> LOGGER.info(
                "Commandes /rfx préchauffées en {} ms (R-601).", RfxCommands.warmUp()));
    }

    /**
     * Ouvre la fenêtre de tick, avant que tout autre mod ne travaille.
     *
     * <p>Priorité la plus haute : la fenêtre doit encadrer le tick complet.
     *
     * @param event événement de tick serveur
     */
    @SubscribeEvent(priority = EventPriority.HIGHEST)
    public void onServerTickPre(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.START) {
            return;
        }
        TickCycle cycle = cycle();
        if (cycle != null) {
            tickPreGuard.run(cycle::onTickPre);
        }
    }

    /**
     * Ferme la fenêtre de tick, après que tout autre mod a terminé.
     *
     * <p>Priorité la plus basse, pour la raison symétrique.
     *
     * @param event événement de tick serveur
     */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void onServerTickPost(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.END) {
            return;
        }
        TickCycle cycle = cycle();
        if (cycle == null) {
            return;
        }
        tickPostGuard.run(() -> {
            cycle.onTickPost();
            long ticks = cycle.currentTick();
            if (ticks % TICK_LOG_INTERVAL == 0) {
                logTickTrace(ticks, cycle);
            }
        });
    }

    /** Cycle de tick courant, ou {@code null} si le runtime natif n'est pas actif. */
    private static TickCycle cycle() {
        RfxRuntime runtime = RfxRuntime.instance();
        return runtime == null ? null : runtime.tickCycle();
    }

    /**
     * Arrête le runtime natif à l'extinction du serveur.
     *
     * <p>Priorité la plus basse : RUSTFORGE-X se retire après tous les autres mods.
     *
     * @param event événement d'arrêt du serveur
     */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void onServerStopping(final ServerStoppingEvent event) {
        shutdownGuard.run(() -> {
            RfxRuntime runtime = RfxRuntime.instance();
            if (runtime == null) {
                return;
            }
            TickCycle cycle = runtime.tickCycle();
            long ticks = cycle == null ? 0 : cycle.currentTick();
            long rejected = cycle == null ? 0 : cycle.rejectedCalls();
            // Avant l'arrêt : les compteurs se lisent sur un runtime encore entier.
            logAttribution(runtime);
            runtime.shutdown();
            LOGGER.info("RUSTFORGE-X arrêté proprement après {} ticks observés ({} appels refusés).",
                    ticks, rejected);
        });
    }
}
