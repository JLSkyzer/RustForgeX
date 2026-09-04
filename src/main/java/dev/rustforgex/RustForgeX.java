package dev.rustforgex;

import com.mojang.logging.LogUtils;
import dev.rustforgex.command.CommandesRfx;
import dev.rustforgex.forge.GardeHook;
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
 * le runtime. Chaque accroche est protégée par une {@link GardeHook} : une exception de
 * RUSTFORGE-X ne doit jamais empêcher le jeu de démarrer ou de tourner (FM-02).
 */
@Mod(RustForgeX.MODID)
public class RustForgeX {

    /** Identifiant du mod. DOIT correspondre à {@code mod_id} dans gradle.properties. */
    public static final String MODID = "rustforgex";

    private static final Logger LOGGER = LogUtils.getLogger();

    private final GardeHook gardeDemarrage = new GardeHook("setup", LOGGER::warn);
    private final GardeHook gardeCommandes = new GardeHook("registerCommands", LOGGER::warn);
    private final GardeHook gardeArret = new GardeHook("serverStopping", LOGGER::warn);

    /** Construit le mod et s'attache aux deux bus d'événements de Forge. */
    public RustForgeX() {
        IEventBus busMod = FMLJavaModLoadingContext.get().getModEventBus();
        busMod.addListener(this::surSetupCommun);
        MinecraftForge.EVENT_BUS.register(this);
    }

    /**
     * Démarre le runtime pendant la phase de setup commune.
     *
     * <p>{@code enqueueWork} garantit l'exécution sur le fil principal de chargement,
     * après que Forge a terminé la construction de tous les mods.
     *
     * @param evenement événement de setup commun
     */
    private void surSetupCommun(final FMLCommonSetupEvent evenement) {
        evenement.enqueueWork(() -> gardeDemarrage.executer(this::demarrerRuntime));
    }

    /** Séquence de démarrage : configuration, vérification de Forge, C-02. */
    private void demarrerRuntime() {
        Path racine = FMLPaths.GAMEDIR.get().resolve(MODID);
        boolean coteClient = FMLEnvironment.dist.isClient();
        String versionForge = ForgeVersion.getVersion();

        RuntimeRfx runtime = RuntimeRfx.demarrer(racine, coteClient, versionForge);

        for (String avertissement : runtime.avertissements()) {
            LOGGER.warn("Configuration : {}", avertissement);
        }
        for (String etape : runtime.rapport().journal()) {
            LOGGER.debug("Démarrage : {}", etape);
        }

        if (runtime.actif()) {
            LOGGER.info("RUSTFORGE-X actif ({} ms) : {}",
                    runtime.rapport().dureeMs(), runtime.rapport().message());
        } else {
            // Un démarrage non nominal n'est pas une erreur du jeu : il doit être
            // lisible sans être alarmant, et dire explicitement que rien n'est cassé.
            LOGGER.warn("RUSTFORGE-X inactif ({}) : {} Le jeu tourne normalement, sans RUSTFORGE-X.",
                    runtime.rapport().etat(), runtime.rapport().message());
        }
    }

    /**
     * Enregistre les commandes du mod.
     *
     * <p>Priorité la plus haute : l'enregistrement doit avoir lieu avant qu'un autre
     * mod ne puisse perturber le répartiteur (PARTIE 5.1).
     *
     * @param evenement événement d'enregistrement des commandes
     */
    @SubscribeEvent(priority = EventPriority.HIGHEST)
    public void surEnregistrementCommandes(final RegisterCommandsEvent evenement) {
        gardeCommandes.executer(() -> CommandesRfx.enregistrer(evenement.getDispatcher()));
    }

    /**
     * Arrête le runtime natif à l'extinction du serveur.
     *
     * <p>Priorité la plus basse : RUSTFORGE-X se retire après tous les autres mods.
     *
     * @param evenement événement d'arrêt du serveur
     */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void surArretServeur(final ServerStoppingEvent evenement) {
        gardeArret.executer(() -> {
            RuntimeRfx runtime = RuntimeRfx.instance();
            if (runtime != null) {
                runtime.arreter();
                LOGGER.info("RUSTFORGE-X arrêté proprement.");
            }
        });
    }
}
