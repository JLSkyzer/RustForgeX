package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.ComparatorBlock;
import net.minecraft.world.level.block.DispenserBlock;
import net.minecraft.world.level.block.HopperBlock;
import net.minecraft.world.level.block.ObserverBlock;
import net.minecraft.world.level.block.RepeaterBlock;
import net.minecraft.world.level.block.piston.PistonBaseBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;

import java.util.ArrayList;
import java.util.List;

import static dev.rustforgex.bench.FixtureBlocks.flag;
import static dev.rustforgex.bench.FixtureBlocks.place;

/**
 * C-36 : circuit de redstone du test G-06 — « circuit de redstone complexe (horloge,
 * comparateurs, pistons) sur 10 000 ticks ».
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <h2>Pourquoi ce test</h2>
 *
 * <p>La redstone vanilla est déterministe : à monde égal, chaque tick produit le même
 * état. C'est donc un test d'<strong>égalité stricte</strong>, sans le bruit de la
 * génération. Et c'est là que l'ordre compte le plus : ticks planifiés, mises à jour de
 * voisins, entités de bloc. Un runtime qui un jour réordonne du travail le casserait ici
 * avant de le casser ailleurs.
 *
 * <h2>Le circuit</h2>
 *
 * <p>Posé sur une dalle de pierre en Y = {@value #FLOOR_Y}, dans les chunks (0, 0) à
 * (1, 1), loin au-dessus du bruit de la génération (ADR-032 : rien ne diffère au-dessus
 * de Y = 160).
 * <ol>
 *   <li><strong>Horloge à répéteurs</strong> : un anneau de 28 répéteurs, retards 1 à 4,
 *       quatre poussières aux coins. Une impulsion y tourne indéfiniment, période de
 *       140 ticks ;
 *   <li><strong>piston</strong> : un observateur regarde un répéteur de l'anneau et
 *       commande un piston collant qui fait aller et venir un bloc de laine ;
 *   <li><strong>comparateur</strong> : un autre observateur déclenche un dropper qui
 *       verse dans un entonnoir, lequel reverse dans le dropper ; un comparateur lit
 *       l'entonnoir et allume une lampe ;
 *   <li><strong>horloge à observateurs</strong> : deux observateurs face à face,
 *       impulsion tous les deux ticks, qui commandent une lampe et un second piston.
 * </ol>
 */
public final class RedstoneCircuit implements BenchFixture {

    /** Dalle de pierre ; le circuit est posé juste au-dessus. */
    static final int FLOOR_Y = 199;

    /** Haut de la boîte vidée et hachée. */
    static final int TOP_Y = 203;

    private static final int Y = FLOOR_Y + 1;

    /** Bloc de redstone posé un tick pour lancer l'impulsion de l'anneau. */
    private static final BlockPos TRIGGER = new BlockPos(1, Y, 2);

    private final PartWatch watch = new PartWatch("anneau", "piston-anneau", "laine-anneau",
            "dropper", "entonnoir", "lampe-comparateur", "observateur-horloge",
            "piston-horloge");

    @Override
    public int minChunk() {
        return 0;
    }

    @Override
    public int maxChunk() {
        return 1;
    }

    @Override
    public int minY() {
        return FLOOR_Y;
    }

    @Override
    public int maxY() {
        return TOP_Y;
    }

    @Override
    public int build(ServerLevel level) {
        int failed = FixtureBlocks.clearBox(level, this);

        // 1. Anneau : bord nord vers l'est, bord est vers le sud, bord sud vers l'ouest,
        // bord ouest vers le nord. Un répéteur reçoit par l'arrière, du côté de son
        // `facing` : celui qui transmet vers l'est regarde vers l'ouest.
        List<BlockPos> path = new ArrayList<>();
        List<Direction> back = new ArrayList<>();
        for (int x = 3; x <= 11; x++) {
            path.add(new BlockPos(x, Y, 2));
            back.add(Direction.WEST);
        }
        for (int z = 3; z <= 7; z++) {
            path.add(new BlockPos(12, Y, z));
            back.add(Direction.NORTH);
        }
        for (int x = 11; x >= 3; x--) {
            path.add(new BlockPos(x, Y, 8));
            back.add(Direction.EAST);
        }
        for (int z = 7; z >= 3; z--) {
            path.add(new BlockPos(2, Y, z));
            back.add(Direction.SOUTH);
        }
        for (int i = 0; i < path.size(); i++) {
            failed += place(level, path.get(i), Blocks.REPEATER.defaultBlockState()
                    .setValue(RepeaterBlock.FACING, back.get(i))
                    .setValue(RepeaterBlock.DELAY, 1 + i % 4));
        }
        for (BlockPos corner : List.of(new BlockPos(2, Y, 2), new BlockPos(12, Y, 2),
                new BlockPos(12, Y, 8), new BlockPos(2, Y, 8))) {
            failed += place(level, corner, Block.updateFromNeighbourShapes(
                    Blocks.REDSTONE_WIRE.defaultBlockState(), level, corner));
        }

        // 2. Piston : l'observateur regarde le répéteur (7, 2) et sort par l'arrière,
        // dans le piston collant qui pousse la laine vers le sud.
        failed += place(level, new BlockPos(7, Y, 3), observer(Direction.NORTH));
        failed += place(level, new BlockPos(7, Y, 4), stickyPiston(Direction.SOUTH));
        failed += place(level, new BlockPos(7, Y, 5), Blocks.WHITE_WOOL.defaultBlockState());

        // 3. Comparateur : observateur sur le répéteur (4, 8), dropper derrière lui, qui
        // verse vers l'est dans l'entonnoir ; l'entonnoir reverse vers l'ouest dans le
        // dropper ; le comparateur lit l'entonnoir et alimente la lampe au nord.
        failed += place(level, new BlockPos(4, Y, 7), observer(Direction.SOUTH));
        failed += place(level, new BlockPos(4, Y, 6), Blocks.DROPPER.defaultBlockState()
                .setValue(DispenserBlock.FACING, Direction.EAST));
        failed += place(level, new BlockPos(5, Y, 6), Blocks.HOPPER.defaultBlockState()
                .setValue(HopperBlock.FACING, Direction.WEST));
        failed += place(level, new BlockPos(5, Y, 5), Blocks.COMPARATOR.defaultBlockState()
                .setValue(ComparatorBlock.FACING, Direction.SOUTH));
        failed += place(level, new BlockPos(5, Y, 4), Blocks.REDSTONE_LAMP.defaultBlockState());
        failed += FixtureBlocks.fill(level, new BlockPos(4, Y, 6),
                new ItemStack(Items.COBBLESTONE, 16));

        // 4. Horloge à observateurs : le second, posé en dernier, déclenche le premier.
        // La lampe reste allumée en continu — elle met quatre ticks à s'éteindre et
        // l'horloge la réalimente tous les deux ticks — : c'est l'observateur, pas
        // elle, que la surveillance regarde.
        failed += place(level, new BlockPos(20, Y, 4), observer(Direction.EAST));
        failed += place(level, new BlockPos(19, Y, 4), Blocks.REDSTONE_LAMP.defaultBlockState());
        failed += place(level, new BlockPos(22, Y, 4), stickyPiston(Direction.EAST));
        failed += place(level, new BlockPos(23, Y, 4), Blocks.WHITE_WOOL.defaultBlockState());
        failed += place(level, new BlockPos(21, Y, 4), observer(Direction.WEST));
        return failed;
    }

    /** Deux étapes : le bloc de redstone posé contre le coin nord-ouest, puis retiré. */
    @Override
    public int launchSteps() {
        return 2;
    }

    @Override
    public int launch(ServerLevel level, int step) {
        return place(level, TRIGGER, step == 1 ? Blocks.REDSTONE_BLOCK.defaultBlockState()
                : Blocks.AIR.defaultBlockState());
    }

    @Override
    public void observe(ServerLevel level) {
        watch.record(
                ringMask(level),
                flag(level, new BlockPos(7, Y, 4), BlockStateProperties.EXTENDED),
                level.getBlockState(new BlockPos(7, Y, 6)).is(Blocks.WHITE_WOOL) ? 1 : 0,
                FixtureBlocks.contents(level, new BlockPos(4, Y, 6)),
                FixtureBlocks.contents(level, new BlockPos(5, Y, 6)),
                flag(level, new BlockPos(5, Y, 4), BlockStateProperties.LIT),
                flag(level, new BlockPos(20, Y, 4), BlockStateProperties.POWERED),
                flag(level, new BlockPos(22, Y, 4), BlockStateProperties.EXTENDED));
    }

    @Override
    public String activity() {
        return watch.summary();
    }

    @Override
    public boolean everyPartMoved() {
        return watch.everyPartMoved();
    }

    private static long ringMask(ServerLevel level) {
        long mask = 0;
        int bit = 0;
        for (int x = 3; x <= 11; x++) {
            mask |= flag(level, new BlockPos(x, Y, 2), BlockStateProperties.POWERED) << bit++;
            mask |= flag(level, new BlockPos(x, Y, 8), BlockStateProperties.POWERED) << bit++;
        }
        for (int z = 3; z <= 7; z++) {
            mask |= flag(level, new BlockPos(12, Y, z), BlockStateProperties.POWERED) << bit++;
            mask |= flag(level, new BlockPos(2, Y, z), BlockStateProperties.POWERED) << bit++;
        }
        return mask;
    }

    private static BlockState observer(Direction face) {
        return Blocks.OBSERVER.defaultBlockState().setValue(ObserverBlock.FACING, face);
    }

    private static BlockState stickyPiston(Direction face) {
        return Blocks.STICKY_PISTON.defaultBlockState().setValue(PistonBaseBlock.FACING, face);
    }
}
