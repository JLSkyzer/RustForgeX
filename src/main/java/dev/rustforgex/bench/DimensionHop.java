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

    /**
     * La zone de l'Overworld seule ; les voyageurs sont relevés au même instant, chacun à
     * part, par {@link Trail}.
     *
     * <p>Chaque voyageur est une composante distincte : des mods tirent au hasard une
     * partie des données d'un mob à son invocation (difficulté de Scaling Health, réglages
     * d'Enhanced AI dans {@code ForgeData}, constaté sur deux lancements). Hachés ensemble,
     * ce bruit aurait rendu bruité le relevé entier ; séparés, le cochon est jugé bruité et
     * les trois autres voyageurs gardent l'égalité stricte.
     */
    @Override
    public long digest(ServerLevel level, LevelChunk chunk) {
        List<String> row = new ArrayList<>();
        for (int i = 0; i < TRAVELLERS.size(); i++) {
            row.add(ResultSweep.hash(locate(level, i)));
        }
        trail.rows.add(row);
        return WorldDigest.region(chunk, minY(), maxY());
    }

    /** Dimension et données du voyageur {@code index}, ou « absent ». */
    private static String locate(ServerLevel level, int index) {
        for (Stop stop : STOPS) {
            for (Entity entity : travellers(level, stop)) {
                if (entity.getTags().contains(TAG + "_" + index)) {
                    return stop.key().location() + " " + EntityType.getKey(entity.getType())
                            + " " + describe(entity);
                }
            }
        }
        return "absent";
    }

    private final Trail trail = new Trail();

    /** La passe qui écrit l'état des voyageurs, un relevé par ligne. */
    ResultSweep trail() {
        return trail;
    }

    /**
     * État des voyageurs à chaque relevé : une ligne par instant, une composante par
     * voyageur. Rien à parcourir : les lignes sont ajoutées par {@link #digest}.
     */
    private static final class Trail implements ResultSweep {

        private final List<List<String>> rows = new ArrayList<>();

        @Override
        public String suffix() {
            return "-travellers";
        }

        @Override
        public List<String> components() {
            return List.of("armor_stand", "pig", "chest_minecart", "item_display");
        }

        @Override
        public void start(ServerLevel level) {
            // Rien à relever d'avance.
        }

        @Override
        public boolean step(ServerLevel level) {
            return true;
        }

        @Override
        public int expected() {
            return rows.size();
        }

        @Override
        public int judged() {
            return rows.size();
        }

        @Override
        public boolean finished() {
            return true;
        }

        @Override
        public String json() {
            List<String> keys = new ArrayList<>();
            for (int i = 0; i < rows.size(); i++) {
                keys.add("relevé-" + i);
            }
            return ResultSweep.table(keys, rows);
        }
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
     * Données d'un voyageur, stables d'une exécution à l'autre : sans son UUID, tiré au
     * hasard à l'invocation.
     *
     * <p>Les attributs sont réduits à ce qui est déterministe : le nom de chaque attribut,
     * et le nom de chacun de ses modificateurs, triés. Le jeu les écrit dans l'ordre d'un
     * {@code HashMap} indexé par des objets sans {@code hashCode} propre ; et des mods en
     * tirent au hasard la base, le montant et l'UUID à l'invocation d'un mob (constaté
     * sur deux lancements : Enhanced AI, Scaling Health). Ce qui reste — quels
     * modificateurs, combien — suffit à voir un modificateur ajouté ou perdu.
     */
    static String describe(Entity entity) {
        CompoundTag data = entity.saveWithoutId(new CompoundTag());
        data.remove("UUID");
        if (data.contains("Attributes", Tag.TAG_LIST)) {
            ListTag attributes = data.getList("Attributes", Tag.TAG_COMPOUND);
            List<String> kept = new ArrayList<>();
            for (int i = 0; i < attributes.size(); i++) {
                CompoundTag attribute = attributes.getCompound(i);
                ListTag modifiers = attribute.getList("Modifiers", Tag.TAG_COMPOUND);
                List<String> names = new ArrayList<>();
                for (int k = 0; k < modifiers.size(); k++) {
                    names.add(modifiers.getCompound(k).getString("Name"));
                }
                names.sort(Comparator.naturalOrder());
                kept.add(attribute.getString("Name") + names);
            }
            kept.sort(Comparator.naturalOrder());
            data.putString("Attributes", String.join(";", kept));
        }
        return data.toString();
    }
}
