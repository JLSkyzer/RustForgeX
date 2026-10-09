package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.ai.attributes.AttributeInstance;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.entity.animal.Pig;
import net.minecraft.world.level.GameRules;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.entity.EntityTypeTest;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

import static dev.rustforgex.bench.FixtureBlocks.place;

/**
 * C-36 : troupeau du test G-13 — « pathfinding de 500 entités ».
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Un enclos de 3 × 3 chunks en Y = {@value #FLOOR_Y}, coupé de deux murs percés de
 * passages étroits. {@value #SIZE} cochons, privés de leurs comportements ordinaires,
 * reçoivent tous les {@value #TRIP_TICKS} ticks une destination à l'autre bout de
 * l'enclos par la navigation du jeu : c'est le pathfinding réel qui trace leur chemin
 * à travers les passages. L'entassement est désactivé — aucun ne meurt étouffé dans la
 * foule — et la portée de recherche élargie pour que le chemin existe.
 *
 * <p>Une foule qui se bouscule n'est pas forcément déterministe : chaque cochon est donc
 * un élément du fichier de passe {@code -herd}, sa case (grille de deux blocs) à chaque
 * relevé une composante, et le test symétrique juge si RUSTFORGE-X est l'intrus plus
 * souvent que le hasard.
 */
public final class Herd implements BenchFixture {

    /** Dalle de pierre, très au-dessus de tout relief généré. */
    static final int FLOOR_Y = 300;

    /** Haut de la tranche vidée et hachée. */
    static final int TOP_Y = 304;

    /** Taille du troupeau (PARTIE 20.3.4). */
    static final int SIZE = 500;

    /** Ticks entre deux changements de destination. */
    static final int TRIP_TICKS = 600;

    private static final int Y = FLOOR_Y + 1;
    private static final int MAX = 47;
    private static final String TAG = "rfxbench_g13";

    /** Les deux destinations, à l'est puis à l'ouest de l'enclos. */
    private static final double[][] TARGETS = {{42.5, Y, 24.5}, {5.5, Y, 24.5}};

    private final List<List<String>> rows = new ArrayList<>();
    private final PartWatch watch = new PartWatch("ouest", "milieu", "est");
    private final Trail trail = new Trail();

    @Override
    public int minChunk() {
        return 0;
    }

    @Override
    public int maxChunk() {
        return 2;
    }

    @Override
    public int minY() {
        return FLOOR_Y;
    }

    @Override
    public int maxY() {
        return TOP_Y;
    }

    /** L'enclos : murs d'enceinte, deux murs intérieurs percés de passages étroits. */
    @Override
    public int build(ServerLevel level) {
        level.getGameRules().getRule(GameRules.RULE_MAX_ENTITY_CRAMMING).set(0, level.getServer());
        int failed = FixtureBlocks.clearBox(level, this);
        for (int i = 0; i <= MAX; i++) {
            for (int h = 0; h < 2; h++) {
                failed += wall(level, 0, i, h) + wall(level, MAX, i, h)
                        + wall(level, i, 0, h) + wall(level, i, MAX, h);
                if (i < 21 || i > 26) {
                    failed += wall(level, 16, i, h);
                }
                if ((i < 5 || i > 8) && (i < 39 || i > 42)) {
                    failed += wall(level, 32, i, h);
                }
            }
        }
        return failed;
    }

    private static int wall(ServerLevel level, int x, int z, int h) {
        return place(level, new BlockPos(x, Y + h, z), Blocks.STONE_BRICKS.defaultBlockState());
    }

    @Override
    public int launchSteps() {
        return 1;
    }

    /** Invoque le troupeau dans la partie ouest, un cochon par bloc, dans un ordre fixe. */
    @Override
    public int launch(ServerLevel level, int step) {
        int failed = 0;
        int index = 0;
        for (int z = 2; z <= MAX - 2 && index < SIZE; z++) {
            for (int x = 2; x <= 14 && index < SIZE; x++) {
                Pig pig = EntityType.PIG.create(level);
                if (pig == null) {
                    failed++;
                    continue;
                }
                pig.moveTo(x + 0.5, Y, z + 0.5, 0.0F, 0.0F);
                pig.addTag(TAG);
                pig.addTag(TAG + "_" + index);
                pig.setPersistenceRequired();
                if (!level.addFreshEntity(pig)) {
                    failed++;
                    continue;
                }
                // Après l'entrée dans le monde : les mods qui ajoutent leurs objectifs à
                // ce moment-là les perdent aussi. Seule la navigation du harnais reste.
                pig.goalSelector.removeAllGoals(goal -> true);
                pig.targetSelector.removeAllGoals(goal -> true);
                AttributeInstance range = pig.getAttribute(Attributes.FOLLOW_RANGE);
                if (range != null) {
                    range.setBaseValue(64.0);
                }
                pig.getNavigation().setMaxVisitedNodesMultiplier(8.0F);
                index++;
            }
        }
        return failed;
    }

    /** Au début de chaque trajet, chaque cochon reçoit la destination suivante. */
    @Override
    public int act(ServerLevel level, int t) {
        if (t % TRIP_TICKS != 1) {
            return 0;
        }
        double[] target = TARGETS[(t / TRIP_TICKS) % TARGETS.length];
        for (Mob mob : herd(level)) {
            mob.getNavigation().moveTo(target[0], target[1], target[2], 1.0);
        }
        return 0;
    }

    @Override
    public void observe(ServerLevel level) {
        long west = 0;
        long middle = 0;
        long east = 0;
        for (Mob mob : herd(level)) {
            if (mob.getX() < 16) {
                west++;
            } else if (mob.getX() < 32) {
                middle++;
            } else {
                east++;
            }
        }
        watch.record(west, middle, east);
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
     * La zone ; et, une fois par relevé, la case de chaque cochon, versée dans
     * {@link #trail()}.
     */
    @Override
    public long digest(ServerLevel level, LevelChunk chunk) {
        if (chunk.getPos().x == 0 && chunk.getPos().z == 0) {
            String[] cells = new String[SIZE];
            java.util.Arrays.fill(cells, "absent");
            for (Mob mob : herd(level)) {
                int index = index(mob);
                if (index >= 0 && index < SIZE) {
                    cells[index] = (int) Math.floor(mob.getX() / 2) + ","
                            + (int) Math.floor(mob.getZ() / 2);
                }
            }
            List<String> row = new ArrayList<>(SIZE);
            for (String cell : cells) {
                row.add(ResultSweep.hash(cell));
            }
            rows.add(row);
        }
        return WorldDigest.region(chunk, minY(), maxY());
    }

    private static List<Mob> herd(ServerLevel level) {
        List<Mob> found = new ArrayList<>();
        level.getEntities(EntityTypeTest.forClass(Mob.class),
                mob -> mob.getTags().contains(TAG), found);
        return found;
    }

    private static int index(Entity entity) {
        for (String tag : entity.getTags()) {
            if (tag.startsWith(TAG + "_")) {
                return Integer.parseInt(tag.substring(TAG.length() + 1));
            }
        }
        return -1;
    }

    /** La passe qui écrit la case de chaque cochon à chaque relevé. */
    ResultSweep trail() {
        return trail;
    }

    /** Une ligne par cochon, une composante par relevé. */
    private final class Trail implements ResultSweep {

        @Override
        public String suffix() {
            return "-herd";
        }

        @Override
        public List<String> components() {
            List<String> names = new ArrayList<>();
            for (int k = 0; k < rows.size(); k++) {
                names.add("relevé-" + k);
            }
            return names;
        }

        @Override
        public void start(ServerLevel level) {
            // Rien à relever d'avance : les lignes viennent des relevés de l'ouvrage.
        }

        @Override
        public boolean step(ServerLevel level) {
            return true;
        }

        @Override
        public int expected() {
            return SIZE;
        }

        @Override
        public int judged() {
            return rows.isEmpty() ? 0 : SIZE;
        }

        @Override
        public boolean finished() {
            return true;
        }

        @Override
        public String json() {
            List<String> keys = new ArrayList<>(SIZE);
            List<List<String>> table = new ArrayList<>(SIZE);
            for (int i = 0; i < SIZE; i++) {
                keys.add(String.format(Locale.ROOT, "pig-%03d", i));
                List<String> line = new ArrayList<>(rows.size());
                for (List<String> row : rows) {
                    line.add(row.get(i));
                }
                table.add(line);
            }
            return ResultSweep.table(keys, table);
        }
    }
}
