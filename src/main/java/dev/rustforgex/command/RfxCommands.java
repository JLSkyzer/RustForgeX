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
 * Maturité : {@code STABLE}.
 *
 * <p>Seules les commandes dont le composant existe sont enregistrées : {@code status},
 * {@code mods}, {@code top}, {@code profile}, {@code discover}, {@code report} et
 * {@code panic-test}. Les autres ({@code why}, {@code workload}, {@code mode},
 * {@code set}, {@code disable}, {@code enable}, {@code bench}, {@code validate},
 * {@code cache}) interrogent ou pilotent des composants des jalons suivants : elles
 * seront ajoutées au jalon qui les rend capables de répondre. Enregistrer dès
 * maintenant une commande qui ne saurait rien dire tromperait l'utilisateur (contrat
 * agent 3.1, ADR-035).
 *
 * <p>Toute commande qui modifie l'état du runtime est journalisée avec son auteur et
 * son horodatage (R-600), par {@link #audit}.
 */
public final class RfxCommands {

    /** Niveau de permission requis sur serveur (PARTIE 5.36). */
    public static final int PERMISSION_LEVEL = 3;

    private static final org.slf4j.Logger LOGGER = com.mojang.logging.LogUtils.getLogger();

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
                        .then(Commands.literal("discover")
                                .executes(context -> {
                                    showDiscovery(context.getSource(),
                                            DiscoveryReport.DEFAULT_LIMIT);
                                    return 1;
                                })
                                // Repartir de zéro : un recensement cumulé depuis le
                                // démarrage mélange le chargement du monde et le jeu.
                                .then(Commands.literal("reset")
                                        .executes(context -> {
                                            resetDiscovery(context.getSource());
                                            return 1;
                                        }))
                                .then(Commands.argument("count", IntegerArgumentType.integer(1, 100))
                                        .executes(context -> {
                                            showDiscovery(context.getSource(),
                                                    IntegerArgumentType.getInteger(context, "count"));
                                            return 1;
                                        })))
                        .then(Commands.literal("report")
                                .executes(context -> {
                                    writeReport(context.getSource());
                                    return 1;
                                }))
                        .then(Commands.literal("panic-test")
                                .executes(context -> {
                                    panicTest(context.getSource());
                                    return 1;
                                })));
    }

    /**
     * Journalise une commande qui modifie l'état du runtime : qui, quoi, quand (R-600).
     *
     * <p>Le journal du serveur est la trace que l'exploitant relit ; l'horodatage y est
     * écrit en clair, au format ISO 8601, pour ne pas dépendre de la mise en forme du
     * journal.
     *
     * @param source émetteur de la commande
     * @param command la commande, sans la barre oblique
     */
    static void audit(CommandSourceStack source, String command) {
        LOGGER.info("RUSTFORGE-X : commande « /{} » exécutée par {} le {}.", command,
                source.getTextName(), java.time.Instant.now());
    }

    /**
     * Vérifie le confinement des panics, en mode {@code debug} seulement.
     *
     * <p>Mutante : la panic est comptée par le runtime, et dix en soixante secondes le
     * font passer en {@code HALTED}. Journalisée à ce titre (R-600), même refusée — la
     * tentative est une information.
     */
    private static void panicTest(CommandSourceStack source) {
        RfxRuntime runtime = RfxRuntime.instance();
        ContainmentCheck.Outcome outcome = runtime == null
                ? new ContainmentCheck.Outcome(ContainmentCheck.Verdict.UNAVAILABLE, null, 0, null, -1)
                : runtime.panicTest();
        audit(source, "rfx panic-test (" + outcome.verdict() + ")");
        for (Component line : ContainmentCheck.lines(outcome)) {
            if (outcome.verdict() == ContainmentCheck.Verdict.NOT_CONFINED) {
                source.sendFailure(line);
            } else {
                source.sendSuccess(() -> line, false);
            }
        }
    }

    /**
     * Préchauffe les commandes au démarrage du serveur : construit une fois, sans rien
     * envoyer, ce que {@code status}, {@code mods}, {@code top} et {@code discover}
     * construisent, et prépare le rapport.
     *
     * <p>Le premier appel d'une commande payait le chargement de ses classes sur le fil
     * du serveur : 6,3 ms pour {@code /rfx status}, au-delà des 5 ms de R-601, contre
     * 0,5 ms ensuite (mesure T-421 du 2026-10-10). Le démarrage n'est pas une commande ;
     * ce coût y est à sa place. Rien n'est modifié : pas de session de profilage, pas de
     * remise à zéro, pas de fichier.
     *
     * @return la durée du préchauffage, en millisecondes ; 0 sans runtime
     */
    public static long warmUp() {
        RfxRuntime runtime = RfxRuntime.instance();
        if (runtime == null) {
            return 0L;
        }
        long start = System.nanoTime();
        runtime.status();
        ModsReport.lines(runtime.modDiscovery());
        TopReport.lines(runtime.topWorkloads(TopReport.DEFAULT_LIMIT),
                runtime.instrumentation() == null ? null : runtime.instrumentation().registry(),
                runtime.lastProfilingSession());
        DiscoveryReport.lines(runtime.unknownFrames(DiscoveryReport.DEFAULT_LIMIT));
        // panic-test ne se préchauffe pas en l'exécutant : ce serait une panic. Seules
        // ses classes sont chargées, et l'incident est construit sans être consigné.
        ContainmentCheck.lines(new ContainmentCheck.Outcome(
                ContainmentCheck.Verdict.CONFINED, ContainmentCheck.DEBUG_MODE, 0, "", 0));
        dev.rustforgex.diag.Incident.nativePanic("warm-up").key();
        runtime.warmUpReports();
        return (System.nanoTime() - start) / 1_000_000L;
    }

    /**
     * Tous les chemins exécutables sous {@code /rfx}, chaque argument entier remplacé par
     * son minimum — {@code "rfx status"}, {@code "rfx top 1"}… Trouvés en parcourant
     * l'arbre enregistré : une commande ajoutée demain est éprouvée (T-420 à T-422) sans
     * qu'on pense à l'ajouter.
     *
     * @throws IllegalStateException si un argument n'est pas un entier : le parcours ne
     *     saurait pas quelle valeur lui donner
     */
    public static List<String> executablePaths(CommandDispatcher<CommandSourceStack> dispatcher) {
        List<String> paths = new java.util.ArrayList<>();
        walk(dispatcher.getRoot().getChild("rfx"), "rfx", paths);
        return paths;
    }

    private static void walk(com.mojang.brigadier.tree.CommandNode<CommandSourceStack> node,
            String path, List<String> out) {
        if (node.getCommand() != null) {
            out.add(path);
        }
        for (com.mojang.brigadier.tree.CommandNode<CommandSourceStack> child
                : node.getChildren()) {
            if (child instanceof com.mojang.brigadier.tree.LiteralCommandNode<?> literal) {
                walk(child, path + " " + literal.getLiteral(), out);
            } else if (child instanceof com.mojang.brigadier.tree.ArgumentCommandNode<?, ?> argument
                    && argument.getType() instanceof IntegerArgumentType integer) {
                walk(child, path + " " + integer.getMinimum(), out);
            } else {
                throw new IllegalStateException(
                        "argument non entier sous /rfx : " + child.getUsageText());
            }
        }
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
        // L'écriture se fait hors du fil du serveur (R-601) ; le message de fin y revient,
        // car un message ne s'envoie que depuis le fil du serveur.
        runtime.writeReportAsync().whenComplete((file, error) -> {
            Runnable answer = () -> {
                if (error == null) {
                    // Le chemin affiché est celui de la machine de l'opérateur : c'est lui
                    // qui doit retrouver le fichier. R-571 porte sur le CONTENU exporté,
                    // pas sur ce qu'on dit à celui qui vient de le demander chez lui.
                    source.sendSuccess(() -> Component.translatable(
                            REPORT_PREFIX + "written", Component.literal(file.toString())),
                            false);
                } else {
                    Throwable cause = error.getCause() == null ? error : error.getCause();
                    source.sendFailure(Component.translatable(
                            REPORT_PREFIX + "failed", Component.literal(cause.toString())));
                }
            };
            if (source.getServer() == null) {
                answer.run();
            } else {
                source.getServer().execute(answer);
            }
        });
    }

    /** Durée par défaut d'une session de diagnostic, en secondes. */
    private static final int DEFAULT_PROFILE_SECONDS = 60;

    /**
     * Ouvre une session de chronométrage, à la demande explicite de l'opérateur.
     *
     * <p>Le message dit ce que ça coûte : demander de la profondeur, c'est accepter que
     * le mod pèse davantage pendant ce temps. Le taire serait le pire des services.
     *
     * <p>Il dit aussi ce qu'il ne garantit <strong>pas</strong>. Le gouverneur coupe la
     * session dès que le budget est dépassé (ADR-020), ce qui arrive vite quand des
     * milliers de sondes passent en chronométré. Annoncer « chronométrage pendant 60 s »
     * était une promesse que le runtime ne tient pas, et son non-respect était muet.
     */
    private static void requestProfiling(CommandSourceStack source, int seconds) {
        audit(source, "rfx profile " + seconds);
        RfxRuntime runtime = RfxRuntime.instance();
        if (runtime == null || !runtime.requestProfilingDepth(seconds)) {
            source.sendFailure(Component.translatable(TopReport.KEY_PREFIX + "profile_refused"));
            return;
        }
        source.sendSuccess(() -> Component.translatable(
                TopReport.KEY_PREFIX + "profile_started", seconds), false);
    }

    /**
     * Envoie le recensement des méthodes chaudes non sondées (C-05).
     *
     * <p>C'est la question d'avant {@code /rfx top} : celui-ci dit ce qui coûte parmi ce
     * qu'on mesure, celui-là dit ce qu'on ne mesure pas.
     */
    private static void showDiscovery(CommandSourceStack source, int count) {
        RfxRuntime runtime = RfxRuntime.instance();
        List<Component> lines = DiscoveryReport.lines(
                runtime == null ? null : runtime.unknownFrames(count));

        for (Component line : lines) {
            source.sendSuccess(() -> line, false);
        }
    }

    /** Vide le recensement, pour mesurer une situation précise plutôt qu'une moyenne. */
    private static void resetDiscovery(CommandSourceStack source) {
        audit(source, "rfx discover reset");
        RfxRuntime runtime = RfxRuntime.instance();
        if (runtime == null || !runtime.resetDiscovery()) {
            source.sendFailure(
                    Component.translatable(DiscoveryReport.KEY_PREFIX + "unavailable"));
            return;
        }
        source.sendSuccess(
                () -> Component.translatable(DiscoveryReport.KEY_PREFIX + "reset"), false);
    }

    /** Envoie le classement des unités les plus coûteuses (C-35). */
    private static void showTop(CommandSourceStack source, int count) {
        RfxRuntime runtime = RfxRuntime.instance();
        List<Component> lines = runtime == null
                ? List.of(Component.translatable(TopReport.KEY_PREFIX + "unavailable"))
                : TopReport.lines(runtime.topWorkloads(count),
                        runtime.instrumentation() == null
                                ? null : runtime.instrumentation().registry(),
                        runtime.lastProfilingSession());

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
