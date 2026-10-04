package dev.rustforgex.bench;

import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.phys.Vec2;
import net.minecraft.world.phys.Vec3;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * C-36 : exécute une commande comme la console du serveur et en capture la sortie (G-11).
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>La source a les droits de la console (niveau 4), une position fixe et n'informe
 * pas les administrateurs : les messages de succès comme d'échec sont gardés, rien
 * n'est diffusé. {@link LoadProfile} exécute aussi des commandes, mais avec la source du
 * serveur, qui jette leur sortie : ici, la sortie est ce qu'on compare.
 */
final class CommandCapture implements CommandSource {

    private final List<String> messages = new ArrayList<>();

    private CommandCapture() {
    }

    /**
     * Résultat d'une commande : son issue, la valeur rendue par le jeu, et les messages.
     *
     * @param succeeded {@code true} si Brigadier a déclaré la commande réussie
     * @param result valeur rendue ; {@link Integer#MIN_VALUE} si la commande a levé
     */
    record Outcome(String command, boolean succeeded, int result, List<String> messages) {

        /** Description stable, comparable d'une exécution à l'autre. */
        String describe() {
            return (succeeded ? "ok|" : "échec|") + result + "|" + String.join("\n", messages);
        }
    }

    /**
     * Exécute {@code command} depuis {@code position}, avec les droits de la console.
     *
     * <p>L'issue vient du rappel de fin de commande de Brigadier, pas de la valeur
     * rendue : une commande réussie peut rendre zéro — {@code scoreboard objectives
     * remove} rend le nombre d'objectifs restants. Une commande qui ne s'analyse pas
     * n'appelle aucun rappel : elle compte comme un échec.
     */
    static Outcome run(MinecraftServer server, ServerLevel level, Vec3 position, String command) {
        CommandCapture capture = new CommandCapture();
        AtomicBoolean succeeded = new AtomicBoolean();
        CommandSourceStack source = new CommandSourceStack(capture, position, Vec2.ZERO, level,
                4, "rfx-bench", Component.literal("rfx-bench"), server, null)
                .withCallback((context, success, value) -> {
                    if (success) {
                        succeeded.set(true);
                    }
                });
        int result;
        try {
            result = server.getCommands().performPrefixedCommand(source, command);
        } catch (RuntimeException | LinkageError e) {
            // Le jeu rattrape lui-même les erreurs de syntaxe ; ce qui arrive ici vient
            // d'une commande qui lève. C'est son verdict, comparable comme un autre.
            result = Integer.MIN_VALUE;
            succeeded.set(false);
            capture.messages.add("exception:" + e.getClass().getName());
        }
        return new Outcome(command, succeeded.get(), result, List.copyOf(capture.messages));
    }

    @Override
    public void sendSystemMessage(Component message) {
        messages.add(message.getString());
    }

    @Override
    public boolean acceptsSuccess() {
        return true;
    }

    @Override
    public boolean acceptsFailure() {
        return true;
    }

    @Override
    public boolean shouldInformAdmins() {
        return false;
    }
}
