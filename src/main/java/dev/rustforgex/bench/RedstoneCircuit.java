package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.Container;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.ComparatorBlock;
import net.minecraft.world.level.block.DispenserBlock;
import net.minecraft.world.level.block.HopperBlock;
import net.minecraft.world.level.block.ObserverBlock;
import net.minecraft.world.level.block.RepeaterBlock;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.DispenserBlockEntity;
import net.minecraft.world.level.block.piston.PistonBaseBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

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
 *
 * <p>Les blocs sont posés par l'API du jeu, pas par des commandes : une commande mal
 * formée échoue sans exception, et le test jugerait un circuit mort. Ici chaque pose
 * est relue, et {@link #build} dit combien n'ont pas pris.
 */
public final class RedstoneCircuit {

    /** Chunks du circuit, bornes incluses. */
    public static final int MIN_CHUNK = 0;

    /** Chunks du circuit, bornes incluses. */
    public static final int MAX_CHUNK = 1;

    /** Dalle de pierre ; le circuit est posé juste au-dessus. */
    public static final int FLOOR_Y = 199;

    /** Haut de la boîte vidée et hachée. */
    public static final int TOP_Y = 203;

    private static final int Y = FLOOR_Y + 1;

    private static final int MIN_BLOCK = MIN_CHUNK * 16;

    private static final int MAX_BLOCK = MAX_CHUNK * 16 + 15;

    /** Bloc de redstone posé un tick pour lancer l'impulsion de l'anneau. */
    private static final BlockPos TRIGGER = new BlockPos(1, Y, 2);

    private RedstoneCircuit() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Construit le circuit, dans un seul tick et toujours dans le même ordre.
     *
     * @return le nombre de poses qui n'ont pas pris ; zéro attendu
     */
    public static int build(ServerLevel level) {
        int failed = 0;
        for (int x = MIN_BLOCK; x <= MAX_BLOCK; x++) {
            for (int z = MIN_BLOCK; z <= MAX_BLOCK; z++) {
                failed += place(level, new BlockPos(x, FLOOR_Y, z), Blocks.STONE.defaultBlockState());
                for (int y = Y; y <= TOP_Y; y++) {
                    failed += place(level, new BlockPos(x, y, z), Blocks.AIR.defaultBlockState());
                }
            }
        }

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
        BlockEntity dropper = level.getBlockEntity(new BlockPos(4, Y, 6));
        if (dropper instanceof DispenserBlockEntity container) {
            container.setItem(0, new ItemStack(Items.COBBLESTONE, 16));
        } else {
            failed++;
        }

        // 4. Horloge à observateurs : le second, posé en dernier, déclenche le premier.
        // La lampe reste allumée en continu — elle met quatre ticks à s'éteindre et
        // l'horloge la réalimente tous les deux ticks — : c'est l'observateur, pas
        // elle, que Watch surveille.
        failed += place(level, new BlockPos(20, Y, 4), observer(Direction.EAST));
        failed += place(level, new BlockPos(19, Y, 4), Blocks.REDSTONE_LAMP.defaultBlockState());
        failed += place(level, new BlockPos(22, Y, 4), stickyPiston(Direction.EAST));
        failed += place(level, new BlockPos(23, Y, 4), Blocks.WHITE_WOOL.defaultBlockState());
        failed += place(level, new BlockPos(21, Y, 4), observer(Direction.WEST));
        return failed;
    }

    /**
     * Lance ou coupe l'impulsion de l'anneau : bloc de redstone posé un tick contre la
     * poussière du coin nord-ouest, puis retiré.
     *
     * @return {@code 1} si la pose n'a pas pris, sinon {@code 0}
     */
    public static int trigger(ServerLevel level, boolean on) {
        return place(level, TRIGGER, on ? Blocks.REDSTONE_BLOCK.defaultBlockState()
                : Blocks.AIR.defaultBlockState());
    }

    /**
     * Compte, tick après tick, les changements d'état de chaque partie du circuit.
     *
     * <p>Une égalité stricte entre trois circuits morts ne prouverait rien : ces
     * compteurs prouvent que chaque partie a tourné. Ils ne servent qu'au journal et au
     * fichier ; le verdict, lui, porte sur les empreintes.
     */
    public static final class Watch {

        private static final String[] NAMES = {"anneau", "piston-anneau", "laine-anneau",
            "dropper", "entonnoir", "lampe-comparateur", "observateur-horloge", "piston-horloge"};

        private final long[] last = new long[NAMES.length];
        private final long[] changes = new long[NAMES.length];
        private boolean primed;

        /** Relève l'état des parties et compte celles qui ont changé depuis le tick précédent. */
        public void observe(ServerLevel level) {
            long[] now = {
                ringMask(level),
                flag(level, new BlockPos(7, Y, 4), BlockStateProperties.EXTENDED),
                level.getBlockState(new BlockPos(7, Y, 6)).is(Blocks.WHITE_WOOL) ? 1 : 0,
                items(level, new BlockPos(4, Y, 6)),
                items(level, new BlockPos(5, Y, 6)),
                flag(level, new BlockPos(5, Y, 4), BlockStateProperties.LIT),
                flag(level, new BlockPos(20, Y, 4), BlockStateProperties.POWERED),
                flag(level, new BlockPos(22, Y, 4), BlockStateProperties.EXTENDED),
            };
            for (int i = 0; i < now.length; i++) {
                if (primed && now[i] != last[i]) {
                    changes[i]++;
                }
                last[i] = now[i];
            }
            primed = true;
        }

        /** Les compteurs, par exemple {@code "anneau 812, piston-anneau 28, ..."}. */
        public String summary() {
            StringBuilder text = new StringBuilder();
            for (int i = 0; i < NAMES.length; i++) {
                text.append(i == 0 ? "" : ", ").append(NAMES[i]).append(' ').append(changes[i]);
            }
            return text.toString();
        }

        /** {@code true} si chaque partie a changé au moins une fois. */
        public boolean everyPartMoved() {
            for (long count : changes) {
                if (count == 0) {
                    return false;
                }
            }
            return true;
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

        private static long flag(ServerLevel level, BlockPos pos,
                net.minecraft.world.level.block.state.properties.BooleanProperty property) {
            BlockState state = level.getBlockState(pos);
            return state.hasProperty(property) && state.getValue(property) ? 1 : 0;
        }

        private static long items(ServerLevel level, BlockPos pos) {
            long count = 0;
            if (level.getBlockEntity(pos) instanceof Container container) {
                for (int i = 0; i < container.getContainerSize(); i++) {
                    count += container.getItem(i).getCount();
                }
            }
            return count;
        }
    }

    /** Pose un état et vérifie qu'il a pris ; {@code 1} sinon. */
    private static int place(ServerLevel level, BlockPos pos, BlockState state) {
        level.setBlock(pos, state, Block.UPDATE_ALL);
        return level.getBlockState(pos).getBlock() == state.getBlock() ? 0 : 1;
    }

    private static BlockState observer(Direction face) {
        return Blocks.OBSERVER.defaultBlockState().setValue(ObserverBlock.FACING, face);
    }

    private static BlockState stickyPiston(Direction face) {
        return Blocks.STICKY_PISTON.defaultBlockState().setValue(PistonBaseBlock.FACING, face);
    }
}
