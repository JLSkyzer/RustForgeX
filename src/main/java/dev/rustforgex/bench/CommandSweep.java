package dev.rustforgex.bench;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.ParseResults;
import com.mojang.brigadier.suggestion.Suggestion;
import com.mojang.brigadier.suggestion.Suggestions;
import com.mojang.brigadier.tree.ArgumentCommandNode;
import com.mojang.brigadier.tree.CommandNode;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.phys.Vec3;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

/**
 * C-36 : toutes les commandes enregistrées, vanilla et mods, éprouvées sans être
 * exécutées (G-11).
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <h2>Pourquoi ne pas les exécuter</h2>
 *
 * <p>Exécuter à l'aveugle les commandes de 290 mods arrêterait le serveur, effacerait
 * des données, ou dépendrait d'un joueur absent. Chaque commande est donc éprouvée par
 * ce qui ne change rien au monde :
 * <ul>
 *   <li><strong>structure</strong> : son arbre de nœuds, arguments et types d'arguments,
 *       tel que l'a enregistré son mod ;
 *   <li><strong>suggestions</strong> : la complétion de son premier argument — le code
 *       des fournisseurs de suggestions des mods ;
 *   <li><strong>help</strong> : la sortie de {@code help <commande>}, qui calcule son
 *       usage en évaluant les droits de chaque branche, sans l'exécuter.
 * </ul>
 *
 * <p>La quatrième composante, <strong>execution</strong>, porte la sortie des commandes
 * vanilla réellement exécutées par {@link CommandBench}. Chaque élément du fichier a les
 * quatre composantes ; celles qui ne le concernent pas valent {@code "-"} partout, donc
 * ne pèsent sur aucun verdict.
 */
final class CommandSweep implements ResultSweep {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Commandes éprouvées par tick. */
    static final int PER_TICK = 20;

    /** Profondeur maximale décrite d'un arbre ; les redirections ne sont pas suivies. */
    private static final int MAX_DEPTH = 32;

    private static final String NONE = "-";

    /** Position d'où {@code help} est demandé : celle des commandes de G-11. */
    private static final Vec3 ORIGIN = new Vec3(2.5, CommandBench.FLOOR_Y + 1, 2.5);

    private final CommandBench bench;
    private final List<CommandNode<CommandSourceStack>> roots = new ArrayList<>();
    private final List<String> keys = new ArrayList<>();
    private final List<List<String>> rows = new ArrayList<>();
    private final StringBuilder clear = new StringBuilder();
    private CommandDispatcher<CommandSourceStack> dispatcher;
    private int next;
    private boolean reported;

    CommandSweep(CommandBench bench) {
        this.bench = bench;
    }

    @Override
    public String suffix() {
        return "-commands";
    }

    @Override
    public List<String> components() {
        return List.of("structure", "suggestions", "help", "execution");
    }

    /** Relève les commandes enregistrées, triées par nom. */
    @Override
    public void start(ServerLevel level) {
        dispatcher = level.getServer().getCommands().getDispatcher();
        roots.addAll(dispatcher.getRoot().getChildren());
        roots.sort(Comparator.comparing(CommandNode::getName));
        LOGGER.info("G-11 : {} commandes enregistrées à éprouver.", roots.size());
    }

    @Override
    public boolean step(ServerLevel level) {
        CommandSourceStack console = level.getServer().createCommandSourceStack();
        int end = Math.min(roots.size(), next + PER_TICK);
        for (; next < end; next++) {
            CommandNode<CommandSourceStack> root = roots.get(next);
            StringBuilder tree = new StringBuilder();
            describe(root, tree, 0);
            String suggested = suggestions(root.getName(), console);
            String help = CommandCapture.run(level.getServer(), level, ORIGIN,
                    "help " + root.getName()).describe();
            keys.add("root/" + root.getName());
            rows.add(List.of(ResultSweep.hash(tree.toString()), ResultSweep.hash(suggested),
                    ResultSweep.hash(help), NONE));
            clear.append("== root/").append(root.getName()).append("\n-- structure\n")
                    .append(tree).append("\n-- suggestions\n").append(suggested)
                    .append("\n-- help\n").append(help).append('\n');
        }
        boolean finished = finished();
        if (finished && !reported) {
            reported = true;
            LOGGER.info("G-11 : {} commandes éprouvées.", roots.size());
        }
        return finished;
    }

    /** Complétion du premier argument, sans attendre un fournisseur asynchrone. */
    private String suggestions(String name, CommandSourceStack console) {
        try {
            ParseResults<CommandSourceStack> parse = dispatcher.parse(name + " ", console);
            Suggestions suggestions = dispatcher.getCompletionSuggestions(parse).getNow(null);
            if (suggestions == null) {
                // Ne jamais bloquer le fil du serveur sur un fournisseur de suggestions :
                // une suggestion non prête est un verdict, comparable comme un autre.
                return "en-attente";
            }
            StringBuilder text = new StringBuilder();
            for (Suggestion suggestion : suggestions.getList()) {
                text.append(suggestion.getText()).append('\n');
            }
            return text.toString();
        } catch (RuntimeException | LinkageError e) {
            return "erreur|" + e.getClass().getName();
        }
    }

    /**
     * Décrit un nœud et ses enfants : nature, nom, type d'argument, exécutable ou non,
     * cible d'une redirection. Les enfants sont triés par nom ; une redirection n'est pas
     * suivie, sans quoi {@code execute} décrirait tout l'arbre, et en boucle.
     */
    private static void describe(CommandNode<?> node, StringBuilder out, int depth) {
        out.append(node.getClass().getSimpleName()).append(':').append(node.getName());
        if (node instanceof ArgumentCommandNode<?, ?> argument) {
            out.append('<').append(argument.getType().getClass().getName()).append('>');
        }
        if (node.getCommand() != null) {
            out.append('!');
        }
        if (node.getRedirect() != null) {
            out.append("->").append(node.getRedirect().getName());
        }
        if (depth >= MAX_DEPTH) {
            out.append("[…]");
            return;
        }
        List<? extends CommandNode<?>> children = new ArrayList<>(node.getChildren());
        children.sort(Comparator.comparing(CommandNode::getName));
        out.append('[');
        for (CommandNode<?> child : children) {
            describe(child, out, depth + 1);
            out.append(',');
        }
        out.append(']');
    }

    @Override
    public int expected() {
        return roots.size() + bench.outcomes().size();
    }

    @Override
    public int judged() {
        return keys.size() + bench.outcomes().size();
    }

    @Override
    public boolean finished() {
        return dispatcher != null && next >= roots.size();
    }

    /** Structure, suggestions et aide de chaque commande, puis la sortie des exécutées. */
    @Override
    public String details() {
        StringBuilder text = new StringBuilder(clear);
        for (CommandCapture.Outcome outcome : bench.outcomes()) {
            text.append("== exec ").append(outcome.command()).append('\n')
                    .append(outcome.describe()).append('\n');
        }
        return text.toString();
    }

    /** Les commandes enregistrées, puis les commandes exécutées par {@link CommandBench}. */
    @Override
    public String json() {
        List<String> allKeys = new ArrayList<>(keys);
        List<List<String>> allRows = new ArrayList<>(rows);
        List<CommandCapture.Outcome> outcomes = bench.outcomes();
        for (int i = 0; i < outcomes.size(); i++) {
            CommandCapture.Outcome outcome = outcomes.get(i);
            allKeys.add(String.format(java.util.Locale.ROOT, "exec/%02d %s", i,
                    outcome.command()));
            allRows.add(List.of(NONE, NONE, NONE, ResultSweep.hash(outcome.describe())));
        }
        return ResultSweep.table(allKeys, allRows);
    }
}
