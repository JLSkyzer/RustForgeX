package dev.rustforgex.command;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import dev.rustforgex.RfxRuntime;
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
public final class RfxCommands {

    /** Niveau de permission requis sur serveur (PARTIE 5.36). */
    public static final int PERMISSION_LEVEL = 3;

    private RfxCommands() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Enregistre les commandes du jalon auprès de Brigadier.
     *
     * @param dispatcher répartiteur fourni par {@code RegisterCommandsEvent}
     */
    /** Préfixe des clés de traduction du rapport exporté. */
    private static final String REPORT_PREFIX = "rustforgex.report.";

    public static void register(CommandDispatcher<CommandSourceStack> dispatcher) {
        dispatcher.register(
                Commands.literal("rfx")
                        .requires(source -> source.hasPermission(PERMISSION_LEVEL))
                        .then(Commands.literal("status")
                                .executes(context -> {
                                    showStatus(context.getSource());
                                    return 1;
                                }))
                        .then(Commands.literal("mods")
                                .executes(context -> {
                                    showMods(context.getSource());
                                    return 1;
                                }))
                        .then(Commands.literal("top")
                                .executes(context -> {
                                    showTop(context.getSource(), TopReport.DEFAULT_LIMIT);
                                    return 1;
                                })
                                // Le nombre d'unités est réglable : quinze suffisent pour
                                // repérer un coupable, pas pour comprendre une répartition.
                                .then(Commands.argument("count", IntegerArgumentType.integer(1, 100))
                                        .executes(context -> {
                                            showTop(context.getSource(),
                                                    IntegerArgumentType.getInteger(context, "count"));
                                            return 1;
                                        })))
                        .then(Commands.literal("profile")
                                .executes(context -> {
                                    requestProfiling(context.getSource(), DEFAULT_PROFILE_SECONDS);
                                    return 1;
                                })
                                .then(Commands.argument("seconds", IntegerArgumentType.integer(5, 600))
                                        .executes(context -> {
                                            requestProfiling(context.getSource(),
                                                    IntegerArgumentType.getInteger(context, "seconds"));
                                            return 1;
                                        })))
                        .then(Commands.literal("report")
                                .executes(context -> {
                                    writeReport(context.getSource());
                                    return 1;
                                })));
    }

    /**
     * Écrit un rapport de métriques et dit où il est (C-34, C-35).
     *
     * <p>L'écriture est une entrée-sortie : elle a lieu parce qu'un joueur l'a demandée,
     * jamais pendant un tick. Un échec d'écriture est dit, il n'interrompt rien.
     */
    private static void writeReport(CommandSourceStack source) {
        RfxRuntime runtime = RfxRuntime.instance();
        if (runtime == null) {
            source.sendSuccess(
                    () -> Component.translatable(StatusReport.KEY_PREFIX + "not_started"), false);
            return;
        }
        try {
            java.nio.file.Path file = runtime.writeReport();
            // Le chemin affiché est celui de la machine de l'opérateur : c'est lui qui
            // doit retrouver le fichier. R-571 porte sur le CONTENU exporté, pas sur ce
            // qu'on dit à celui qui vient de le demander chez lui.
            source.sendSuccess(() -> Component.translatable(
                    REPORT_PREFIX + "written", Component.literal(file.toString())), false);
        } catch (java.io.IOException e) {
            source.sendFailure(Component.translatable(
                    REPORT_PREFIX + "failed", Component.literal(e.toString())));
        }
    }

    /** Durée par défaut d'une session de diagnostic, en secondes. */
    private static final int DEFAULT_PROFILE_SECONDS = 60;

    /**
     * Ouvre une session de chronométrage, à la demande explicite de l'opérateur.
     *
     * <p>Le message dit ce que ça coûte : demander de la profondeur, c'est accepter que
     * le mod pèse davantage pendant ce temps. Le taire serait le pire des services.
     */
    private static void requestProfiling(CommandSourceStack source, int seconds) {
        RfxRuntime runtime = RfxRuntime.instance();
        if (runtime == null || !runtime.requestProfilingDepth(seconds)) {
            source.sendFailure(Component.translatable(TopReport.KEY_PREFIX + "profile_refused"));
            return;
        }
        source.sendSuccess(() -> Component.translatable(
                TopReport.KEY_PREFIX + "profile_started", seconds), false);
    }

    /** Envoie le classement des unités les plus coûteuses (C-35). */
    private static void showTop(CommandSourceStack source, int count) {
        RfxRuntime runtime = RfxRuntime.instance();
        List<Component> lines = runtime == null
                ? List.of(Component.translatable(TopReport.KEY_PREFIX + "unavailable"))
                : TopReport.lines(runtime.topWorkloads(count),
                        runtime.instrumentation() == null
                                ? null : runtime.instrumentation().registry());

        for (Component line : lines) {
            source.sendSuccess(() -> line, false);
        }
    }

    /** Envoie l'inventaire des mods (C-41) à l'émetteur de la commande. */
    private static void showMods(CommandSourceStack source) {
        RfxRuntime runtime = RfxRuntime.instance();
        List<Component> lines = ModsReport.lines(
                runtime == null ? null : runtime.modDiscovery());

        for (Component line : lines) {
            source.sendSuccess(() -> line, false);
        }
    }

    /** Envoie le rapport de statut à l'émetteur de la commande. */
    private static void showStatus(CommandSourceStack source) {
        RfxRuntime runtime = RfxRuntime.instance();
        List<Component> lines = runtime == null
                ? List.of(Component.translatable(StatusReport.KEY_PREFIX + "not_started"))
                : runtime.status();

        for (Component line : lines) {
            source.sendSuccess(() -> line, false);
        }
    }
}
