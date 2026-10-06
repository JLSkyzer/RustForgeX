package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraftforge.common.capabilities.ForgeCapabilities;
import net.minecraftforge.energy.IEnergyStorage;
import net.minecraftforge.items.IItemHandler;
import net.minecraftforge.items.ItemHandlerHelper;
import net.minecraftforge.registries.ForgeRegistries;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.Optional;

/**
 * C-36 : machines de mods du test G-07 — « automatisation lourde (machines de mods) sur
 * 30 minutes » —, avec Create et Mekanism.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <h2>Pourquoi des noms de mods ici</h2>
 *
 * <p>INV-12 interdit au <strong>moteur</strong> toute condition sur un nom de mod. Ce
 * fichier n'est pas le moteur : c'est un ouvrage de test, et le choix de Create et de
 * Mekanism est celui de l'utilisateur (2026-10-06). Les blocs sont résolus par leur nom
 * de registre à l'exécution, sans dépendance de compilation : un mod absent, ou une
 * propriété d'état inconnue, compte comme une pose refusée et rend la comparaison
 * invalide — jamais un test qui passe sur un ouvrage incomplet.
 *
 * <h2>L'ouvrage</h2>
 *
 * <p>Dans les chunks (0, 0) à (1, 1), sur une dalle en Y = {@value #FLOOR_Y} :
 * <ul>
 *   <li><strong>presse</strong> (Create) : moteur créatif, presse mécanique au-dessus d'un
 *       dépôt chargé de lingots de fer, qui deviennent des plaques ;
 *   <li><strong>mélangeur</strong> (Create) : moteur créatif, mélangeur mécanique
 *       au-dessus d'un bassin d'andésite et de pépites de fer — alliage d'andésite ;
 *   <li><strong>enrichissement</strong> (Mekanism) : cube d'énergie créatif, chambre
 *       d'enrichissement chargée de minerai de fer ;
 *   <li><strong>broyage</strong> (Mekanism) : cube d'énergie créatif, broyeur chargé de
 *       lingots de fer.
 * </ul>
 *
 * <p>Toutes ces recettes ont une sortie certaine, sans tirage : la comparaison est
 * stricte. La preuve que chaque machine travaille est le changement de son contenu.
 */
public final class ModMachines implements BenchFixture {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Dalle de pierre, très au-dessus de tout relief généré. */
    static final int FLOOR_Y = 300;

    /** Haut de la tranche vidée et hachée. */
    static final int TOP_Y = 306;

    private static final int Y = FLOOR_Y + 1;

    /**
     * La presse et le mélangeur travaillent un bloc au-dessus de leur dépôt ou bassin :
     * leur tête descend dans cet espace. Posés juste au-dessus, ils tournaient sans rien
     * transformer (essai du 2026-10-06).
     */
    private static final BlockPos DEPOT = new BlockPos(3, Y, 4);
    private static final BlockPos PRESS = DEPOT.above(2);
    private static final BlockPos PRESS_MOTOR = PRESS.west();

    private static final BlockPos BASIN = new BlockPos(8, Y, 4);
    private static final BlockPos MIXER = BASIN.above(2);
    /** Le mélangeur s'engrène comme un rouage : il tourne par un rouage voisin. */
    private static final BlockPos MIXER_COG = MIXER.east();
    private static final BlockPos MIXER_MOTOR = MIXER_COG.above();

    private static final BlockPos ENRICHER = new BlockPos(14, Y, 4);
    private static final BlockPos ENRICHER_CUBE = ENRICHER.west();

    private static final BlockPos CRUSHER = new BlockPos(14, Y, 9);
    private static final BlockPos CRUSHER_CUBE = CRUSHER.west();

    private final PartWatch watch = new PartWatch("dépôt", "bassin", "enrichissement",
            "broyeur");

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

        // La presse prend sa rotation sur l'axe de son orientation : orientée est, par
        // un arbre venant de l'ouest. Orientée nord, elle restait à vitesse nulle (essai
        // du 2026-10-06).
        failed += put(level, DEPOT, "create:depot");
        failed += put(level, PRESS, "create:mechanical_press", "facing=east");
        failed += put(level, PRESS_MOTOR, "create:creative_motor", "facing=east");
        failed += insert(level, DEPOT, "minecraft:iron_ingot", 16);

        // Le mélangeur n'accepte pas d'arbre : un moteur posé dessus formait un réseau
        // d'un seul bloc (essai du 2026-10-06). Il tourne par le rouage voisin.
        failed += put(level, BASIN, "create:basin");
        failed += put(level, MIXER, "create:mechanical_mixer");
        failed += put(level, MIXER_COG, "create:cogwheel", "axis=y");
        failed += put(level, MIXER_MOTOR, "create:creative_motor", "facing=down");
        // Le mélangeur exige une vitesse « moyenne » (MechanicalMixerBlock rend
        // SpeedLevel.MEDIUM, lu par javap) ; le moteur créatif part à 16 tr/min et le
        // laissait tourner sans jamais démarrer (essai du 2026-10-06). Réglé à 64 avant
        // son premier tick.
        failed += tune(level, MIXER_MOTOR, "ScrollValue", 64);

        // Un cube créatif posé par le code démarre vide, et un cube créatif ne change
        // jamais de contenu : le remplir était accepté et sans effet (essai du
        // 2026-10-06, machines sans énergie). Un cube basique stocke ce qu'il reçoit —
        // 4 millions de joules, plusieurs fois les 64 opérations — et l'éjecte par sa
        // face avant vers la machine.
        failed += put(level, ENRICHER, "mekanism:enrichment_chamber", "facing=east");
        failed += put(level, ENRICHER_CUBE, "mekanism:basic_energy_cube", "facing=east");
        failed += energize(level, ENRICHER_CUBE);
        failed += insert(level, ENRICHER, "minecraft:iron_ore", 64);

        failed += put(level, CRUSHER, "mekanism:crusher", "facing=east");
        failed += put(level, CRUSHER_CUBE, "mekanism:basic_energy_cube", "facing=east");
        failed += energize(level, CRUSHER_CUBE);
        failed += insert(level, CRUSHER, "minecraft:iron_ingot", 64);
        return failed;
    }

    /**
     * Le bassin est chargé deux ticks après la pose. Le mélangeur ne réexamine son bassin
     * que lorsque le contenu change : chargé dans le tick de la pose, avant que la
     * rotation ne lui parvienne, il restait immobile au-dessus d'un bassin plein (essai
     * du 2026-10-06).
     */
    @Override
    public int launchSteps() {
        return 2;
    }

    @Override
    public int launch(ServerLevel level, int step) {
        if (step != 2) {
            return 0;
        }
        return insert(level, BASIN, "minecraft:andesite", 64)
                + insert(level, BASIN, "minecraft:iron_nugget", 64);
    }

    @Override
    public void observe(ServerLevel level) {
        watch.record(contents(level, DEPOT), contents(level, BASIN),
                contents(level, ENRICHER), contents(level, CRUSHER));
    }

    @Override
    public String activity() {
        return watch.summary();
    }

    @Override
    public boolean everyPartMoved() {
        return watch.everyPartMoved();
    }

    /** L'empreinte de la zone ; en diagnostic, les données des machines en clair. */
    @Override
    public long digest(ServerLevel level, LevelChunk chunk) {
        if (Boolean.parseBoolean(System.getProperty(DigestRecorder.PROPERTY_DETAILS, "false"))
                && chunk.getPos().x == 0 && chunk.getPos().z == 0) {
            for (BlockPos pos : new BlockPos[] {DEPOT, PRESS, PRESS_MOTOR, BASIN, MIXER, MIXER_COG,
                MIXER_MOTOR, ENRICHER, ENRICHER_CUBE, CRUSHER, CRUSHER_CUBE}) {
                BlockEntity entity = level.getBlockEntity(pos);
                LOGGER.info("G-07 {} {} : {}", pos.toShortString(), level.getBlockState(pos),
                        entity == null ? "-" : entity.saveWithFullMetadata());
            }
        }
        return WorldDigest.region(chunk, minY(), maxY(), ModMachines::withoutDisplayAngle);
    }

    /**
     * Retire la position d'affichage de l'objet posé sur un dépôt de Create : son angle
     * ({@code HeldItem.Angle}) et son décalage latéral ({@code Offset}, {@code PrevOffset}).
     * Deux lancements sans RUSTFORGE-X ont donné 296 et 246 pour l'angle, 0,098 et 0,052
     * pour le décalage, et ce sont les trois seuls champs de tout l'ouvrage à différer
     * (2026-10-06) : l'endroit où l'objet est dessiné, tiré à sa pose. Le reste du dépôt —
     * objet, nombre, verrou, tampon de sortie — reste dans l'empreinte.
     */
    private static CompoundTag withoutDisplayAngle(CompoundTag data) {
        if (data.contains("HeldItem", Tag.TAG_COMPOUND)) {
            CompoundTag held = data.getCompound("HeldItem");
            held.remove("Angle");
            held.remove("Offset");
            held.remove("PrevOffset");
        }
        return data;
    }

    /**
     * Pose le bloc {@code id} avec ses propriétés {@code nom=valeur}.
     *
     * @return {@code 1} si le bloc n'existe pas, si une propriété est inconnue ou si la
     *     pose n'a pas pris
     */
    private static int put(ServerLevel level, BlockPos pos, String id, String... properties) {
        ResourceLocation key = ResourceLocation.tryParse(id);
        Block block = key == null ? null : ForgeRegistries.BLOCKS.getValue(key);
        if (block == null || block == Blocks.AIR) {
            LOGGER.warn("G-07 : bloc {} introuvable", id);
            return 1;
        }
        BlockState state = block.defaultBlockState();
        for (String property : properties) {
            String[] pair = property.split("=", 2);
            Property<?> definition = block.getStateDefinition().getProperty(pair[0]);
            Optional<BlockState> next = definition == null ? Optional.empty()
                    : with(state, definition, pair[1]);
            if (next.isEmpty()) {
                LOGGER.warn("G-07 : propriété {} refusée par {}", property, id);
                return 1;
            }
            state = next.get();
        }
        return FixtureBlocks.place(level, pos, state);
    }

    private static <T extends Comparable<T>> Optional<BlockState> with(BlockState state,
            Property<T> property, String value) {
        return property.getValue(value).map(parsed -> state.setValue(property, parsed));
    }

    /** Faces essayées pour charger une machine, dans un ordre fixe. */
    private static final Direction[] FACES = {Direction.UP, Direction.NORTH, Direction.SOUTH,
        Direction.WEST, Direction.EAST, Direction.DOWN};

    /**
     * Range des objets dans une machine par sa capacité d'objets, celle que les tuyaux et
     * entonnoirs des mods utilisent, face par face dans un ordre fixe.
     *
     * <p>Sans face précisée, Mekanism refuse tout (essai du 2026-10-06) : ses machines
     * n'acceptent des objets que par une face configurée en entrée. La première face qui
     * accepte le tout est retenue.
     *
     * @return {@code 1} si aucune face n'accepte la totalité
     */
    private static int insert(ServerLevel level, BlockPos pos, String itemId, int count) {
        ResourceLocation key = ResourceLocation.tryParse(itemId);
        Item item = key == null ? null : ForgeRegistries.ITEMS.getValue(key);
        BlockEntity entity = level.getBlockEntity(pos);
        if (item == null || entity == null) {
            LOGGER.warn("G-07 : impossible de charger {} dans {}", itemId, pos.toShortString());
            return 1;
        }
        for (Direction face : FACES) {
            IItemHandler handler =
                    entity.getCapability(ForgeCapabilities.ITEM_HANDLER, face).orElse(null);
            if (handler == null) {
                continue;
            }
            ItemStack stack = new ItemStack(item, count);
            if (ItemHandlerHelper.insertItem(handler, stack, true).isEmpty()) {
                ItemHandlerHelper.insertItem(handler, stack, false);
                return 0;
            }
        }
        LOGGER.warn("G-07 : aucune face de {} n'accepte {} × {}", pos.toShortString(), count,
                itemId);
        return 1;
    }

    /**
     * Écrit une valeur entière dans les données d'une entité de bloc, puis la relit.
     *
     * @return {@code 1} si l'entité est absente ou si la valeur n'a pas pris
     */
    private static int tune(ServerLevel level, BlockPos pos, String key, int value) {
        BlockEntity entity = level.getBlockEntity(pos);
        if (entity == null) {
            return 1;
        }
        CompoundTag data = entity.saveWithoutMetadata();
        data.putInt(key, value);
        entity.load(data);
        entity.setChanged();
        return entity.saveWithoutMetadata().getInt(key) == value ? 0 : 1;
    }

    /**
     * Remplit une réserve d'énergie par sa capacité d'énergie Forge, face par face dans un
     * ordre fixe.
     *
     * @return {@code 1} si aucune face n'accepte d'énergie
     */
    private static int energize(ServerLevel level, BlockPos pos) {
        BlockEntity entity = level.getBlockEntity(pos);
        if (entity == null) {
            return 1;
        }
        for (Direction face : FACES) {
            IEnergyStorage storage =
                    entity.getCapability(ForgeCapabilities.ENERGY, face).orElse(null);
            if (storage != null && storage.receiveEnergy(Integer.MAX_VALUE, true) > 0) {
                storage.receiveEnergy(Integer.MAX_VALUE, false);
                return 0;
            }
        }
        LOGGER.warn("G-07 : aucune face de {} n'accepte d'énergie", pos.toShortString());
        return 1;
    }

    /** Contenu d'une machine par sa capacité d'objets, valable dans une exécution. */
    private static long contents(ServerLevel level, BlockPos pos) {
        BlockEntity entity = level.getBlockEntity(pos);
        IItemHandler handler = entity == null ? null
                : entity.getCapability(ForgeCapabilities.ITEM_HANDLER, null).orElse(null);
        if (handler == null) {
            return 0L;
        }
        long hash = 17L;
        for (int slot = 0; slot < handler.getSlots(); slot++) {
            ItemStack stack = handler.getStackInSlot(slot);
            hash = hash * 31 + (stack.isEmpty() ? 0 : stack.getItem().hashCode());
            hash = hash * 31 + stack.getCount();
        }
        return hash;
    }
}
