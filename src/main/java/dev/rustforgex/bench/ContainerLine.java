package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.Container;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.alchemy.PotionUtils;
import net.minecraft.world.item.alchemy.Potions;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.HopperBlock;
import net.minecraft.world.level.block.state.BlockState;

import static dev.rustforgex.bench.FixtureBlocks.contents;
import static dev.rustforgex.bench.FixtureBlocks.fill;
import static dev.rustforgex.bench.FixtureBlocks.place;

/**
 * C-36 : ligne de conteneurs du test G-12 — « crafting, inventaires, conteneurs ».
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}. Le crafting, lui,
 * est éprouvé à part par {@link CraftingSweep}.
 *
 * <p>Trois lignes alimentées par des entonnoirs, sur une dalle en Y = {@value #FLOOR_Y}
 * dans les chunks (0, 0) à (1, 1), au-dessus du bruit de la génération (ADR-032) :
 * <ol>
 *   <li><strong>cuisson</strong> : un coffre verse 32 fers bruts dans un fourneau par le
 *       haut, un second lui donne le charbon par le côté, un entonnoir sous le fourneau
 *       vide les lingots dans un troisième coffre ;
 *   <li><strong>transfert</strong> : un coffre de piles mélangées se vide par entonnoir
 *       dans un tonneau, que deux entonnoirs vident dans un dernier coffre ;
 *   <li><strong>alambic</strong> : des fioles d'eau et de la poudre de blaze par le côté,
 *       des verrues du Nether par le haut, l'alambic distille.
 * </ol>
 *
 * <p>Tout y est déterministe : délais des entonnoirs, durées de cuisson et de
 * distillation. L'empreinte de la tranche se compare donc en égalité stricte.
 */
public final class ContainerLine implements BenchFixture {

    /** Dalle de pierre ; les entonnoirs de sortie y sont encastrés. */
    static final int FLOOR_Y = 199;

    /** Haut de la boîte vidée et hachée : les coffres d'entrée sont en Y + 2. */
    static final int TOP_Y = 204;

    private static final int Y = FLOOR_Y + 1;

    private static final BlockPos FURNACE = new BlockPos(4, Y, 4);
    private static final BlockPos ORE_IN = new BlockPos(4, Y + 2, 4);
    private static final BlockPos FUEL_IN = new BlockPos(3, Y + 1, 4);
    private static final BlockPos INGOTS_OUT = new BlockPos(4, FLOOR_Y, 5);

    private static final BlockPos MIXED_IN = new BlockPos(12, Y + 2, 4);
    private static final BlockPos BARREL = new BlockPos(12, Y, 4);
    private static final BlockPos MIXED_OUT = new BlockPos(14, FLOOR_Y, 4);

    private static final BlockPos STAND = new BlockPos(20, Y, 4);
    private static final BlockPos WART_IN = new BlockPos(20, Y + 2, 4);
    private static final BlockPos BOTTLES_IN = new BlockPos(19, Y + 1, 4);

    // « lingots » prouve la cuisson, « distillées » la distillation : les autres parties
    // changent dès qu'un objet entre, même si l'appareil ne fait rien.
    private final PartWatch watch = new PartWatch("minerai", "charbon", "fourneau",
            "lingots", "piles", "tonneau", "piles-sortie", "verrues", "fioles", "alambic",
            "distillées");

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

        // 1. Cuisson. Un entonnoir pousse vers son `facing` et tire du bloc au-dessus.
        failed += place(level, FURNACE, Blocks.FURNACE.defaultBlockState());
        failed += place(level, FURNACE.above(), hopper(Direction.DOWN));
        failed += place(level, ORE_IN, Blocks.CHEST.defaultBlockState());
        failed += place(level, FUEL_IN.below(), hopper(Direction.EAST));
        failed += place(level, FUEL_IN, Blocks.CHEST.defaultBlockState());
        failed += place(level, FURNACE.below(), hopper(Direction.SOUTH));
        failed += place(level, INGOTS_OUT, Blocks.CHEST.defaultBlockState());
        failed += fill(level, ORE_IN, new ItemStack(Items.RAW_IRON, 32));
        failed += fill(level, FUEL_IN, new ItemStack(Items.COAL, 16));

        // 2. Transfert : coffre, entonnoir, tonneau, deux entonnoirs, coffre.
        failed += place(level, BARREL, Blocks.BARREL.defaultBlockState());
        failed += place(level, BARREL.above(), hopper(Direction.DOWN));
        failed += place(level, MIXED_IN, Blocks.CHEST.defaultBlockState());
        failed += place(level, BARREL.below(), hopper(Direction.EAST));
        failed += place(level, BARREL.below().east(), hopper(Direction.EAST));
        failed += place(level, MIXED_OUT, Blocks.CHEST.defaultBlockState());
        failed += fill(level, MIXED_IN, new ItemStack(Items.COBBLESTONE, 64),
                new ItemStack(Items.OAK_LOG, 20), new ItemStack(Items.DIAMOND, 5));

        // 3. Alambic : l'entonnoir du dessus remplit l'ingrédient, celui du côté les
        // fioles puis le combustible. Aucun entonnoir dessous : il tirerait les fioles
        // avant la fin de la distillation.
        failed += place(level, STAND, Blocks.BREWING_STAND.defaultBlockState());
        failed += place(level, STAND.above(), hopper(Direction.DOWN));
        failed += place(level, WART_IN, Blocks.CHEST.defaultBlockState());
        failed += place(level, BOTTLES_IN.below(), hopper(Direction.EAST));
        failed += place(level, BOTTLES_IN, Blocks.CHEST.defaultBlockState());
        failed += fill(level, WART_IN, new ItemStack(Items.NETHER_WART, 2));
        failed += fill(level, BOTTLES_IN, water(), water(), water(),
                new ItemStack(Items.BLAZE_POWDER, 1));
        return failed;
    }

    /** Les entonnoirs démarrent seuls : aucun lancement. */
    @Override
    public int launchSteps() {
        return 0;
    }

    @Override
    public int launch(ServerLevel level, int step) {
        return 0;
    }

    @Override
    public void observe(ServerLevel level) {
        watch.record(contents(level, ORE_IN), contents(level, FUEL_IN),
                contents(level, FURNACE), contents(level, INGOTS_OUT),
                contents(level, MIXED_IN), contents(level, BARREL),
                contents(level, MIXED_OUT), contents(level, WART_IN),
                contents(level, BOTTLES_IN), contents(level, STAND), awkward(level));
    }

    /** Fioles de l'alambic devenues « étranges » : la distillation a eu lieu. */
    private static long awkward(ServerLevel level) {
        if (!(level.getBlockEntity(STAND) instanceof Container stand)) {
            return 0;
        }
        long brewed = 0;
        for (int slot = 0; slot < 3; slot++) {
            if (PotionUtils.getPotion(stand.getItem(slot)) == Potions.AWKWARD) {
                brewed++;
            }
        }
        return brewed;
    }

    @Override
    public String activity() {
        return watch.summary();
    }

    @Override
    public boolean everyPartMoved() {
        return watch.everyPartMoved();
    }

    private static BlockState hopper(Direction face) {
        return Blocks.HOPPER.defaultBlockState().setValue(HopperBlock.FACING, face);
    }

    private static ItemStack water() {
        return PotionUtils.setPotion(new ItemStack(Items.POTION), Potions.WATER);
    }
}
