package dev.rustforgex.command;

import com.mojang.brigadier.CommandDispatcher;
import dev.rustforgex.RuntimeRfx;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;

import java.util.List;

/**
 * C-38 : commandes en jeu de RUSTFORGE-X.
 *
 * <p>Cahier des charges : PARTIE 5.36. Tests : T-420 à T-422.
 * Maturité : {@code STABLE} pour {@code /rfx status}, seule commande du jalon M0.
 *
 * <p>Les autres commandes ({@code /rfx top}, {@code /rfx why}, {@code /rfx mods}…)
 * interrogent des composants qui n'existent pas encore : elles seront ajoutées au
 * jalon qui les rend capables de répondre. Enregistrer dès maintenant une commande
 * qui ne saurait rien dire tromperait l'utilisateur (contrat agent 3.1).
 */
public final class CommandesRfx {

    /** Niveau de permission requis sur serveur (PARTIE 5.36). */
    public static final int NIVEAU_PERMISSION = 3;

    private CommandesRfx() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Enregistre les commandes du jalon auprès de Brigadier.
     *
     * @param dispatcher répartiteur fourni par {@code RegisterCommandsEvent}
     */
    public static void enregistrer(CommandDispatcher<CommandSourceStack> dispatcher) {
        dispatcher.register(
                Commands.literal("rfx")
                        .requires(source -> source.hasPermission(NIVEAU_PERMISSION))
                        .then(Commands.literal("status")
                                .executes(contexte -> {
                                    afficherStatut(contexte.getSource());
                                    return 1;
                                })));
    }

    /** Envoie le rapport de statut à l'émetteur de la commande. */
    private static void afficherStatut(CommandSourceStack source) {
        RuntimeRfx runtime = RuntimeRfx.instance();
        List<String> lignes = runtime == null
                ? List.of("RUSTFORGE-X n'a pas démarré : aucun statut disponible.")
                : runtime.statut();

        for (String ligne : lignes) {
            source.sendSuccess(() -> Component.literal(ligne), false);
        }
    }
}
