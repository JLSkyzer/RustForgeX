package dev.rustforgex;

import com.mojang.logging.LogUtils;
import dev.rustforgex.command.RfxCommands;
import dev.rustforgex.forge.HookGuard;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.RegisterCommandsEvent;
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
 * <p>Ce jalon (M0) n'installe <strong>aucun hook de tick</strong>. Le livrable M0 exige
 * que « le jeu tourne exactement comme sans le mod, overhead mesuré proche de zéro » :
 * s'accrocher au tick pour n'y rien faire coûterait du temps à chaque tick sans rien
 * apporter. Les phases {@code rfx_tick_begin} / {@code rfx_phase} / {@code rfx_tick_end}
 * (IF-02) seront branchées avec C-04 et C-05, au jalon M1, quand elles auront quelque
 * chose à observer.
 *
 * <p>Ce que ce composant fait à ce jalon : démarrer le runtime (C-02), vérifier la
 * version de Forge (FM-01), enregistrer {@code /rfx status} (C-38) et arrêter proprement
 * le runtime. Chaque accroche est protégée par un {@link HookGuard} : une exception de
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
            if (runtime != null) {
                runtime.shutdown();
                LOGGER.info("RUSTFORGE-X arrêté proprement.");
            }
        });
    }
}
