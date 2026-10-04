package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.Container;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BooleanProperty;

/**
 * C-36 : poses et relevés communs aux ouvrages de test ({@link BenchFixture}).
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Les blocs sont posés par l'API du jeu, pas par des commandes : une commande mal
 * formée échoue sans exception, et le test jugerait un ouvrage incomplet. Ici chaque
 * pose est relue et compte comme refusée si elle n'a pas pris.
 */
final class FixtureBlocks {

    private FixtureBlocks() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /** Pose un état et vérifie qu'il a pris ; {@code 1} sinon. */
    static int place(ServerLevel level, BlockPos pos, BlockState state) {
        level.setBlock(pos, state, Block.UPDATE_ALL);
        return level.getBlockState(pos).getBlock() == state.getBlock() ? 0 : 1;
    }

    /**
     * Dalle de pierre en {@code floorY} et vide au-dessus jusqu'à {@code topY}, sur tout
     * le carré de chunks : l'ouvrage part d'un terrain identique dans chaque exécution.
     *
     * @return le nombre de poses refusées
     */
    static int clearBox(ServerLevel level, BenchFixture fixture) {
        int failed = 0;
        int min = fixture.minChunk() * 16;
        int max = fixture.maxChunk() * 16 + 15;
        for (int x = min; x <= max; x++) {
            for (int z = min; z <= max; z++) {
                failed += place(level, new BlockPos(x, fixture.minY(), z),
                        Blocks.STONE.defaultBlockState());
                for (int y = fixture.minY() + 1; y <= fixture.maxY(); y++) {
                    failed += place(level, new BlockPos(x, y, z), Blocks.AIR.defaultBlockState());
                }
            }
        }
        return failed;
    }

    /**
     * Range des objets dans un conteneur déjà posé, à partir de l'emplacement 0.
     *
     * @return {@code 1} si le bloc n'est pas un conteneur, ou trop petit ; {@code 0} sinon
     */
    static int fill(ServerLevel level, BlockPos pos, ItemStack... stacks) {
        if (!(level.getBlockEntity(pos) instanceof Container container)
                || container.getContainerSize() < stacks.length) {
            return 1;
        }
        for (int slot = 0; slot < stacks.length; slot++) {
            container.setItem(slot, stacks[slot]);
        }
        return 0;
    }

    /**
     * Empreinte du contenu d'un conteneur : objet, nombre et données de chaque
     * emplacement. {@code 0} si le bloc n'est pas un conteneur.
     *
     * <p>Valable <strong>dans une exécution seulement</strong> : l'objet y est pris par
     * son identité, qui change d'un lancement de la JVM à l'autre. Elle sert à
     * {@link PartWatch} pour voir qu'un contenu change, jamais à comparer deux
     * exécutions — c'est le rôle de {@link WorldDigest#region}.
     */
    static long contents(ServerLevel level, BlockPos pos) {
        if (!(level.getBlockEntity(pos) instanceof Container container)) {
            return 0L;
        }
        long hash = 17L;
        for (int slot = 0; slot < container.getContainerSize(); slot++) {
            ItemStack stack = container.getItem(slot);
            hash = hash * 31 + (stack.isEmpty() ? 0 : stack.getItem().hashCode());
            hash = hash * 31 + stack.getCount();
            hash = hash * 31 + (stack.getTag() == null ? 0 : stack.getTag().hashCode());
        }
        return hash;
    }

    /** {@code 1} si la propriété booléenne du bloc est vraie, {@code 0} sinon. */
    static long flag(ServerLevel level, BlockPos pos, BooleanProperty property) {
        BlockState state = level.getBlockState(pos);
        return state.hasProperty(property) && state.getValue(property) ? 1 : 0;
    }
}
