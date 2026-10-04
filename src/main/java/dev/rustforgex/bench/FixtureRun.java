package dev.rustforgex.bench;

import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.GameRules;
import net.minecraft.world.level.chunk.LevelChunk;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/**
 * C-36 : déroulé d'un test qui pose un ouvrage ({@link BenchFixture}) et le regarde
 * tourner — G-06, G-12.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <ol>
 *   <li>{@link #force} fige les ticks aléatoires et l'apparition des créatures, et force
 *       le carré de chunks de l'ouvrage ;
 *   <li>on attend que ces chunks <strong>tournent</strong>, pas seulement qu'ils soient
 *       chargés : un chunk complet dont les entités ne sont pas encore chargées garde ses
 *       ticks planifiés en attente, et l'ouvrage démarrerait plus tard dans une
 *       exécution que dans l'autre ;
 *   <li>repos de {@value #SETTLE_TICKS} ticks, pose, lancement ;
 *   <li>relevé de l'activité à chaque tick, empreinte de la tranche de l'ouvrage tous les
 *       {@value #SNAPSHOT_EVERY} ticks et à la fin.
 * </ol>
 *
 * <p>Tout est compté à partir du lancement, jamais de l'heure du serveur : la
 * génération des chunks ne prend pas le même temps d'une exécution à l'autre, mais
 * l'ouvrage vit exactement les mêmes ticks.
 */
final class FixtureRun {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /**
     * Intervalle entre deux empreintes, en ticks.
     *
     * <p>Premier, pour ne tomber en phase avec aucune horloge d'un ouvrage : une empreinte
     * prise toujours au même point d'un cycle ferait paraître immobile un ouvrage qui
     * tourne.
     */
    static final int SNAPSHOT_EVERY = 499;

    /** Repos entre les chunks prêts et la pose de l'ouvrage, en ticks. */
    static final int SETTLE_TICKS = 100;

    private final BenchFixture fixture;
    private final String testId;
    private final int ticks;
    private final int timeoutTicks;

    private final List<long[]> snapshots = new ArrayList<>();
    private final List<String> names = new ArrayList<>();
    private final List<String> activity = new ArrayList<>();

    private int age;
    private int readyAt = -1;
    private int builtAt = -1;
    private int startedAt = -1;
    private int launchStep;
    private int failures;
    private boolean timedOut;
    private boolean done;

    FixtureRun(BenchFixture fixture, String testId, int ticks, int timeoutTicks) {
        this.fixture = fixture;
        this.testId = testId;
        this.ticks = ticks;
        this.timeoutTicks = timeoutTicks;
    }

    /** Fige le jeu et force le carré de l'ouvrage ; la génération suit en arrière-plan. */
    void force(MinecraftServer server, ServerLevel level) {
        level.getGameRules().getRule(GameRules.RULE_RANDOMTICKING).set(0, server);
        level.getGameRules().getRule(GameRules.RULE_DOMOBSPAWNING).set(false, server);
        for (int x = fixture.minChunk(); x <= fixture.maxChunk(); x++) {
            for (int z = fixture.minChunk(); z <= fixture.maxChunk(); z++) {
                level.setChunkForced(x, z, true);
            }
        }
        LOGGER.info("{} : {} chunks marqués forcés, génération en arrière-plan.",
                testId, expected());
    }

    /**
     * Un tick du déroulé, appelé à chaque fin de tick après {@link #force}.
     *
     * @return {@code true} une fois le test terminé, ou abandonné faute de chunks prêts
     */
    boolean tick(ServerLevel level) {
        if (done) {
            return true;
        }
        age++;
        if (readyAt < 0) {
            if (countTicking(level) == expected()) {
                readyAt = age;
                LOGGER.info("{} : chunks prêts en {} ticks, repos de {} ticks.",
                        testId, age, SETTLE_TICKS);
            } else if (age >= timeoutTicks) {
                timedOut = true;
                done = true;
            }
            return done;
        }
        if (age - readyAt < SETTLE_TICKS) {
            return false;
        }
        if (builtAt < 0) {
            failures = fixture.build(level);
            builtAt = age;
            if (fixture.launchSteps() == 0) {
                start(level);
            }
            return false;
        }
        if (startedAt < 0) {
            if (launchStep == 0 && !fixture.canLaunch(level)) {
                return false;
            }
            launchStep++;
            failures += fixture.launch(level, launchStep);
            if (launchStep >= fixture.launchSteps()) {
                start(level);
            }
            return false;
        }
        int t = age - startedAt;
        fixture.observe(level);
        if (t % SNAPSHOT_EVERY == 0 || t == ticks) {
            snapshot(level, t);
        }
        done = t >= ticks;
        return done;
    }

    private void start(ServerLevel level) {
        startedAt = age;
        fixture.observe(level);
        LOGGER.info("{} : ouvrage posé et lancé, {} pose(s) refusée(s).", testId, failures);
    }

    /** Une empreinte de la tranche de l'ouvrage, chunk par chunk, et son activité. */
    private void snapshot(ServerLevel level, int t) {
        long[] perChunk = new long[expected()];
        int i = 0;
        for (int x = fixture.minChunk(); x <= fixture.maxChunk(); x++) {
            for (int z = fixture.minChunk(); z <= fixture.maxChunk(); z++) {
                LevelChunk chunk = level.getChunkSource().getChunkNow(x, z);
                perChunk[i++] = chunk == null ? 0L
                        : WorldDigest.region(chunk, fixture.minY(), fixture.maxY());
            }
        }
        snapshots.add(perChunk);
        names.add("t" + t);
        String summary = fixture.activity();
        activity.add("t" + t + " : " + summary);
        LOGGER.info("{} t={} : changements d'état {}", testId, t, summary);
    }

    /** Chunks du carré qui font tourner leurs blocs et entités de bloc. */
    private int countTicking(ServerLevel level) {
        int ticking = 0;
        for (int x = fixture.minChunk(); x <= fixture.maxChunk(); x++) {
            for (int z = fixture.minChunk(); z <= fixture.maxChunk(); z++) {
                if (level.shouldTickBlocksAt(ChunkPos.asLong(x, z))) {
                    ticking++;
                }
            }
        }
        return ticking;
    }

    int expected() {
        int side = fixture.maxChunk() - fixture.minChunk() + 1;
        return side * side;
    }

    /** Chunks du carré chargés et complets. */
    int ready(ServerLevel level) {
        int ready = 0;
        for (int x = fixture.minChunk(); x <= fixture.maxChunk(); x++) {
            for (int z = fixture.minChunk(); z <= fixture.maxChunk(); z++) {
                if (level.getChunkSource().getChunkNow(x, z) != null) {
                    ready++;
                }
            }
        }
        return ready;
    }

    boolean timedOut() {
        return timedOut;
    }

    int failures() {
        return failures;
    }

    /** {@code true} si une partie de l'ouvrage n'a jamais changé d'état. */
    boolean inert() {
        return !fixture.everyPartMoved();
    }

    /** Une composante par empreinte, nommée par son instant ({@code t499} …). */
    List<String> components() {
        return List.copyOf(names);
    }

    List<String> activity() {
        return List.copyOf(activity);
    }

    /** Table JSON des chunks : pour chacun, ses empreintes dans l'ordre des instants. */
    String chunksJson() {
        StringBuilder chunks = new StringBuilder();
        int i = 0;
        for (int x = fixture.minChunk(); x <= fixture.maxChunk(); x++) {
            for (int z = fixture.minChunk(); z <= fixture.maxChunk(); z++) {
                chunks.append(i == 0 ? "\n" : ",\n").append("    \"").append(x).append(',')
                        .append(z).append("\": [");
                for (int k = 0; k < snapshots.size(); k++) {
                    chunks.append(k == 0 ? "\"" : ", \"")
                            .append(String.format(Locale.ROOT, "%016x", snapshots.get(k)[i]))
                            .append('"');
                }
                chunks.append(']');
                i++;
            }
        }
        return chunks.toString();
    }
}
