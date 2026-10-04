package dev.rustforgex.bench;

import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.phys.Vec3;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;

/**
 * C-36 : ouvrage du test G-04 — « téléportation entre dimensions ».
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Quatre entités marquées — support d'armure, cochon sans IA, wagonnet coffre chargé,
 * affichage d'objet — sont posées dans l'Overworld, puis envoyées tous les
 * {@value #HOP_EVERY} ticks dans la dimension suivante : Nether, End, Overworld, et ainsi
 * de suite. La téléportation passe par {@code execute in <dimension> run tp}, le chemin
 * du jeu — celui que les mods observent et modifient.
 *
 * <p>L'empreinte ajoute à celle de la zone l'état de chaque voyageur : dimension, type,
 * données sans l'UUID — tiré au hasard à l'invocation. Les points d'arrivée sont des
 * chunks forcés : au Nether en (0, 0), au-dessus du plafond de roche ; dans l'End en
 * (50, 50), loin de l'île centrale où le combat du dragon, aléatoire, se déroule.
 */
public final class DimensionHop implements BenchFixture {

    /** Dalle de pierre de l'Overworld, très au-dessus de tout relief généré. */
    static final int FLOOR_Y = 300;

    /** Haut de la tranche vidée et hachée. */
    static final int TOP_Y = 304;

    /** Intervalle entre deux sauts de dimension, en ticks. */
    static final int HOP_EVERY = 100;

    private static final String TAG = "rfxbench_g04";

    private static final List<String> TRAVELLERS = List.of(
            "minecraft:armor_stand ~ ~ ~ {NoGravity:1b,ShowArms:1b,Tags:[\"%s\",\"%s_0\"]}",
            "minecraft:pig ~2 ~ ~ {NoAI:1b,NoGravity:1b,Silent:1b,Tags:[\"%s\",\"%s_1\"]}",
            "minecraft:chest_minecart ~4 ~ ~ {NoGravity:1b,Tags:[\"%s\",\"%s_2\"],"
                    + "Items:[{Slot:0b,id:\"minecraft:diamond\",Count:3b}]}",
            "minecraft:item_display ~6 ~ ~ {item:{id:\"minecraft:apple\",Count:1b},"
                    + "Tags:[\"%s\",\"%s_3\"]}");

    /** Une dimension du trajet : sa clé, le chunk forcé et le point d'arrivée. */
    private record Stop(ResourceKey<Level> key, int chunkX, int chunkZ, Vec3 arrival) {
    }

    private static final Stop OVERWORLD =
            new Stop(Level.OVERWORLD, 0, 0, new Vec3(4.5, FLOOR_Y + 1, 4.5));
    private static final Stop NETHER = new Stop(Level.NETHER, 0, 0, new Vec3(4.5, 200, 4.5));
    private static final Stop END = new Stop(Level.END, 50, 50, new Vec3(804.5, 200, 804.5));

    /** Ordre des sauts : le premier part de l'Overworld vers le Nether. */
    private static final List<Stop> ROUTE = List.of(NETHER, END, OVERWORLD);

    /** Ordre de l'empreinte, indépendant du trajet. */
    private static final List<Stop> STOPS = List.of(OVERWORLD, NETHER, END);

    private final PartWatch watch = new PartWatch("overworld", "nether", "end");

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

    /** Vide la zone de l'Overworld et force les chunks d'arrivée du Nether et de l'End. */
    @Override
    public int build(ServerLevel level) {
        int failed = FixtureBlocks.clearBox(level, this);
        for (Stop stop : List.of(NETHER, END)) {
            ServerLevel target = level.getServer().getLevel(stop.key());
            if (target == null) {
                failed++;
                continue;
            }
            target.setChunkForced(stop.chunkX(), stop.chunkZ(), true);
        }
        return failed;
    }

    @Override
    public int launchSteps() {
        return 1;
    }

    /** Les arrivées du Nether et de l'End doivent tourner avant le premier saut. */
    @Override
    public boolean canLaunch(ServerLevel level) {
        for (Stop stop : List.of(NETHER, END)) {
            ServerLevel target = level.getServer().getLevel(stop.key());
            if (target == null || !target.shouldTickBlocksAt(
                    ChunkPos.asLong(stop.chunkX(), stop.chunkZ()))) {
                return false;
            }
        }
        return true;
    }

    /** Invoque les voyageurs dans la zone de l'Overworld. */
    @Override
    public int launch(ServerLevel level, int step) {
        int failed = 0;
        for (String traveller : TRAVELLERS) {
            String command = "summon " + String.format(Locale.ROOT, traveller, TAG, TAG);
            if (!CommandCapture.run(level.getServer(), level, OVERWORLD.arrival(), command)
                    .succeeded()) {
                failed++;
            }
        }
        return failed;
    }

    /** Tous les {@value #HOP_EVERY} ticks, chaque voyageur saute à la dimension suivante. */
    @Override
    public int act(ServerLevel level, int t) {
        if (t == 0 || t % HOP_EVERY != 0) {
            return 0;
        }
        Stop next = ROUTE.get((t / HOP_EVERY - 1) % ROUTE.size());
        int failed = 0;
        for (int i = 0; i < TRAVELLERS.size(); i++) {
            Vec3 at = next.arrival().add(2.0 * i, 0, 0);
            String command = String.format(Locale.ROOT,
                    "execute in %s run tp @e[tag=%s_%d] %.1f %.1f %.1f",
                    next.key().location(), TAG, i, at.x, at.y, at.z);
            if (!CommandCapture.run(level.getServer(), level, OVERWORLD.arrival(), command)
                    .succeeded()) {
                failed++;
            }
        }
        return failed;
    }

    @Override
    public void observe(ServerLevel level) {
        long[] counts = new long[STOPS.size()];
        for (int i = 0; i < STOPS.size(); i++) {
            counts[i] = travellers(level, STOPS.get(i)).size();
        }
        watch.record(counts);
    }

    @Override
    public String activity() {
        return watch.summary();
    }

    @Override
    public boolean everyPartMoved() {
        return watch.everyPartMoved();
    }

    /** La zone de l'Overworld, puis chaque voyageur, dimension par dimension. */
    @Override
    public long digest(ServerLevel level, LevelChunk chunk) {
        StringBuilder state = new StringBuilder();
        state.append(WorldDigest.region(chunk, minY(), maxY()));
        for (Stop stop : STOPS) {
            state.append("\n#").append(stop.key().location());
            for (Entity entity : travellers(level, stop)) {
                state.append('\n').append(EntityType.getKey(entity.getType())).append(' ')
                        .append(describe(entity));
            }
        }
        return Long.parseUnsignedLong(ResultSweep.hash(state.toString()), 16);
    }

    /** Les voyageurs présents dans une dimension, triés par leur marque individuelle. */
    private static List<Entity> travellers(ServerLevel level, Stop stop) {
        ServerLevel target = level.getServer().getLevel(stop.key());
        List<Entity> found = new ArrayList<>();
        if (target == null) {
            return found;
        }
        target.getEntities(EntityTypeTest.forClass(Entity.class),
                entity -> entity.getTags().contains(TAG), found);
        found.sort(Comparator.comparing(entity -> entity.getTags().stream()
                .filter(tag -> tag.startsWith(TAG + "_")).findFirst().orElse("")));
        return found;
    }

    /**
     * Données d'un voyageur, stables d'une exécution à l'autre : sans son UUID, et avec
     * ses attributs triés par nom — le jeu les écrit dans l'ordre d'une table indexée par
     * l'identité des objets, qui change à chaque lancement.
     */
    static String describe(Entity entity) {
        CompoundTag data = entity.saveWithoutId(new CompoundTag());
        data.remove("UUID");
        if (data.contains("Attributes", Tag.TAG_LIST)) {
            ListTag attributes = data.getList("Attributes", Tag.TAG_COMPOUND);
            List<CompoundTag> sorted = new ArrayList<>();
            for (int i = 0; i < attributes.size(); i++) {
                sorted.add(attributes.getCompound(i));
            }
            sorted.sort(Comparator.comparing(attribute -> attribute.getString("Name")));
            ListTag ordered = new ListTag();
            ordered.addAll(sorted);
            data.put("Attributes", ordered);
        }
        return data.toString();
    }
}
