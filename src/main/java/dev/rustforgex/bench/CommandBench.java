package dev.rustforgex.bench;

import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.phys.Vec3;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.List;

/**
 * C-36 : commandes vanilla du test G-11 — « commandes vanilla et de mods » —, exécutées
 * dans une zone vidée, sortie capturée.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}. Les commandes des
 * mods sont éprouvées à part, sans être exécutées, par {@link CommandSweep}.
 *
 * <p>Une liste fixe de commandes au résultat déterministe : blocs, copies, données,
 * tableau des scores, {@code execute}, entités. Chacune a une issue attendue — réussite,
 * ou échec voulu pour éprouver le chemin d'erreur. Une issue contraire compte comme une
 * pose refusée : la comparaison est alors déclarée invalide, plutôt que de juger égales
 * trois exécutions qui n'ont pas fait ce qu'on croit.
 *
 * <p>Les effets sur le monde se voient dans l'empreinte de la zone, en Y = {@value #FLOOR_Y}
 * dans le chunk (0, 0) ; la sortie de chaque commande, dans le fichier de
 * {@link CommandSweep}.
 */
public final class CommandBench implements BenchFixture {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Dalle de pierre, très au-dessus de tout relief généré. */
    static final int FLOOR_Y = 300;

    /** Haut de la tranche vidée et hachée. */
    static final int TOP_Y = 304;

    /** Origine des coordonnées relatives des commandes, au centre du bloc (2, 301, 2). */
    private static final Vec3 ORIGIN = new Vec3(2.5, FLOOR_Y + 1, 2.5);

    /** Une commande et son issue attendue. */
    private record Expected(String command, boolean succeeds) {
    }

    private static final List<Expected> COMMANDS = List.of(
            ok("setblock ~ ~ ~ minecraft:stone"),
            ok("fill ~1 ~ ~ ~5 ~2 ~3 minecraft:oak_planks"),
            ok("clone ~1 ~ ~ ~5 ~2 ~3 ~1 ~ ~6"),
            ok("fill ~1 ~ ~ ~5 ~2 ~3 minecraft:glass replace minecraft:oak_planks"),
            ok("setblock ~8 ~ ~ minecraft:chest"),
            ok("item replace block ~8 ~ ~ container.0 with minecraft:diamond 5"),
            ok("data get block ~8 ~ ~ Items"),
            ok("data merge block ~8 ~ ~ {CustomName:'\"bench\"'}"),
            ok("execute if block ~8 ~ ~ minecraft:chest run setblock ~8 ~1 ~ minecraft:gold_block"),
            ok("execute unless block ~ ~ ~ minecraft:air run fill ~10 ~ ~ ~12 ~ ~2 "
                    + "minecraft:redstone_block"),
            ok("scoreboard objectives add rfxbench dummy"),
            ok("scoreboard players set alpha rfxbench 7"),
            ok("scoreboard players operation alpha rfxbench *= alpha rfxbench"),
            ok("scoreboard players get alpha rfxbench"),
            ok("execute store result block ~8 ~ ~ Items[0].Count byte 1 run "
                    + "scoreboard players get alpha rfxbench"),
            ok("data get block ~8 ~ ~ Items[0].Count"),
            // La valeur d'une règle est rendue comme résultat : une règle entière et
            // positive, pour qu'une réussite ne se confonde pas avec un zéro.
            ok("gamerule maxEntityCramming"),
            ok("summon minecraft:armor_stand ~10 ~ ~10 {NoGravity:1b,Tags:[\"rfxbench\"]}"),
            ok("execute as @e[tag=rfxbench] run data get entity @s Pos"),
            ok("kill @e[tag=rfxbench]"),
            ok("scoreboard objectives remove rfxbench"),
            fails("setblock ~ ~ ~ minecraft:not_a_block"),
            // Échec voulu qui ne dépend que de la zone chargée. Un `fill` trop grand
            // sortait du monde dans un chunk voisin, chargé ou non selon l'exécution :
            // le jeu répondait « hors du monde » ou « non chargé » (campagne G-11 du
            // 2026-10-04, la référence 2 seule différait).
            fails("clone ~1 ~ ~ ~5 ~2 ~3 ~2 ~ ~"),
            fails("scoreboard players get nobody rfxbench"));

    private final List<CommandCapture.Outcome> outcomes = new ArrayList<>();

    private static Expected ok(String command) {
        return new Expected(command, true);
    }

    private static Expected fails(String command) {
        return new Expected(command, false);
    }

    /** Les sorties des commandes, dans l'ordre de la liste ; vide avant la pose. */
    List<CommandCapture.Outcome> outcomes() {
        return List.copyOf(outcomes);
    }

    @Override
    public int minChunk() {
        return 0;
    }

    @Override
    public int maxChunk() {
        return 0;
    }

    @Override
    public int minY() {
        return FLOOR_Y;
    }

    @Override
    public int maxY() {
        return TOP_Y;
    }

    /** Vide la zone puis exécute les commandes, dans l'ordre, dans le même tick. */
    @Override
    public int build(ServerLevel level) {
        int failed = FixtureBlocks.clearBox(level, this);
        for (Expected expected : COMMANDS) {
            CommandCapture.Outcome outcome =
                    CommandCapture.run(level.getServer(), level, ORIGIN, expected.command());
            outcomes.add(outcome);
            if (outcome.succeeded() != expected.succeeds()) {
                failed++;
                LOGGER.warn("G-11 : issue inattendue pour « {} » : {}", expected.command(),
                        outcome.describe());
            }
        }
        return failed;
    }

    @Override
    public int launchSteps() {
        return 0;
    }

    @Override
    public int launch(ServerLevel level, int step) {
        return 0;
    }

    /** Rien ne bouge après la pose : l'ouvrage est le résultat des commandes. */
    @Override
    public void observe(ServerLevel level) {
        // L'activité de G-11 est dans la sortie des commandes, pas dans un mouvement.
    }

    @Override
    public String activity() {
        long succeeded = outcomes.stream().filter(CommandCapture.Outcome::succeeded).count();
        return outcomes.size() + " commandes, " + succeeded + " abouties";
    }

    /** Aucune partie mobile ; la garde est l'issue attendue de chaque commande. */
    @Override
    public boolean everyPartMoved() {
        return true;
    }
}
