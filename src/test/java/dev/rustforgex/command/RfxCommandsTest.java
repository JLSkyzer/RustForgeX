package dev.rustforgex.command;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.ComponentContents;
import net.minecraft.network.chat.contents.TranslatableContents;
import net.minecraft.world.phys.Vec2;
import net.minecraft.world.phys.Vec3;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.List;

/**
 * Tests des commandes en jeu (C-38, PARTIE 5.36) : T-420 et T-422.
 *
 * <p>Toutes les commandes de {@code /rfx} sont trouvées en parcourant l'arbre enregistré,
 * pas listées à la main : une commande ajoutée demain sera éprouvée sans que personne
 * pense à l'ajouter ici. Le runtime n'est pas démarré dans ces tests ; chaque commande
 * doit alors le dire, pas se taire.
 *
 * <p>T-421 (aucune commande au-delà de 5 ms sur le fil du serveur) ne se juge pas ici :
 * sans runtime, les commandes ne font presque rien. Il se mesure sur un serveur réel.
 */
class RfxCommandsTest {

    /** Source de commande qui garde les messages reçus. */
    private static final class Capture implements CommandSource {

        final List<Component> messages = new ArrayList<>();

        @Override
        public void sendSystemMessage(Component message) {
            messages.add(message);
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

    private static CommandSourceStack source(Capture capture, int permission) {
        return new CommandSourceStack(capture, Vec3.ZERO, Vec2.ZERO, null, permission,
                "test", Component.literal("test"), null, null);
    }

    private static CommandDispatcher<CommandSourceStack> registered() {
        CommandDispatcher<CommandSourceStack> dispatcher = new CommandDispatcher<>();
        RfxCommands.register(dispatcher);
        return dispatcher;
    }

    /** Toutes les commandes exécutables sous {@code /rfx} (parcours de l'arbre). */
    private static List<String> everyCommand(CommandDispatcher<CommandSourceStack> dispatcher) {
        return RfxCommands.executablePaths(dispatcher);
    }

    @Test
    @DisplayName("L'arbre de /rfx est parcouru en entier")
    void theWholeTreeIsWalked() {
        List<String> commands = everyCommand(registered());

        assertTrue(commands.contains("rfx status"), commands.toString());
        assertTrue(commands.contains("rfx top 1"), commands.toString());
        assertTrue(commands.contains("rfx discover reset"), commands.toString());
        assertTrue(commands.size() >= 9, "au moins neuf chemins exécutables : " + commands);
    }

    /** T-420 : chaque commande répond, par un texte traduisible (section 5 du CLAUDE.md). */
    @Test
    @DisplayName("T-420 : chaque commande répond, même sans runtime démarré")
    void everyCommandAnswers() throws CommandSyntaxException {
        CommandDispatcher<CommandSourceStack> dispatcher = registered();
        for (String command : everyCommand(dispatcher)) {
            Capture capture = new Capture();
            dispatcher.execute(command, source(capture, RfxCommands.PERMISSION_LEVEL));

            assertFalse(capture.messages.isEmpty(), "« " + command + " » n'a rien répondu");
            for (Component message : capture.messages) {
                assertTrue(translatable(message),
                        "« " + command + " » répond par un texte codé en dur : " + message);
            }
        }
    }

    /**
     * Un message traduisible, ou un conteneur vide dont tous les enfants le sont :
     * {@code sendFailure} enveloppe le message dans un composant vide stylé en rouge.
     */
    private static boolean translatable(Component message) {
        if (message.getContents() instanceof TranslatableContents) {
            return true;
        }
        return message.getContents() == ComponentContents.EMPTY
                && !message.getSiblings().isEmpty()
                && message.getSiblings().stream().allMatch(RfxCommandsTest::translatable);
    }

    /** T-422 : sous le niveau 3, aucune commande de /rfx n'est accessible. */
    @Test
    @DisplayName("T-422 : sous le niveau 3, toutes les commandes sont refusées")
    void everyCommandIsRefusedBelowTheRequiredLevel() {
        CommandDispatcher<CommandSourceStack> dispatcher = registered();
        for (String command : everyCommand(dispatcher)) {
            Capture capture = new Capture();
            CommandSourceStack player = source(capture, RfxCommands.PERMISSION_LEVEL - 1);

            assertThrows(CommandSyntaxException.class,
                    () -> dispatcher.execute(command, player),
                    "« " + command + " » accessible au niveau "
                            + (RfxCommands.PERMISSION_LEVEL - 1));
            assertTrue(capture.messages.isEmpty(), "« " + command + " » a répondu sans droit");
        }
    }

    /** T-422 : au niveau requis et au-dessus, toutes les commandes s'exécutent. */
    @Test
    @DisplayName("T-422 : au niveau 3 et au-delà, toutes les commandes s'exécutent")
    void everyCommandRunsFromTheRequiredLevel() throws CommandSyntaxException {
        CommandDispatcher<CommandSourceStack> dispatcher = registered();
        for (int level = RfxCommands.PERMISSION_LEVEL; level <= 4; level++) {
            for (String command : everyCommand(dispatcher)) {
                assertEquals(1, dispatcher.execute(command, source(new Capture(), level)),
                        "« " + command + " » au niveau " + level);
            }
        }
    }
}
