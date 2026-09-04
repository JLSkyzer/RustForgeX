package dev.rustforgex;

import com.mojang.logging.LogUtils;
import dev.rustforgex.command.RfxCommands;
import dev.rustforgex.forge.HookGuard;
import dev.rustforgex.forge.TickCycle;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.RegisterCommandsEvent;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.event.server.ServerStoppingEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.IEventBus;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.event.lifecycle.FMLCommonSetupEvent;
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

    private final HookGuard startupGuard = new HookGuard("setup", LOGGER::warn);
    private final HookGuard commandsGuard = new HookGuard("registerCommands", LOGGER::warn);
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
        IEventBus modBus = FMLJavaModLoadingContext.get().getModEventBus();
        modBus.addListener(this::onCommonSetup);
        MinecraftForge.EVENT_BUS.register(this);
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
        } else {
            // Un démarrage non nominal n'est pas une erreur du jeu : il doit être
            // lisible sans être alarmant, et dire explicitement que rien n'est cassé.
            LOGGER.warn("RUSTFORGE-X inactif ({}) : {} Le jeu tourne normalement, sans RUSTFORGE-X.",
                    runtime.report().state(), runtime.report().message());
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
                LOGGER.debug("Cycle de tick : {} ticks observés, {} appels refusés.",
                        ticks, cycle.rejectedCalls());
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
            runtime.shutdown();
            LOGGER.info("RUSTFORGE-X arrêté proprement après {} ticks observés ({} appels refusés).",
                    ticks, rejected);
        });
    }
}
