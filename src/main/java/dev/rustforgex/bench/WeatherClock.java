package dev.rustforgex.bench;

import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.GameRules;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.DaylightDetectorBlock;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.timers.TimerCallback;
import net.minecraft.world.level.timers.TimerCallbacks;
import net.minecraft.world.level.timers.TimerQueue;

import static dev.rustforgex.bench.FixtureBlocks.flag;
import static dev.rustforgex.bench.FixtureBlocks.place;

/**
 * C-36 : ouvrage du test G-14 — « météo, cycle jour/nuit, événements planifiés ».
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <h2>Ce qui est déterministe, et ce qui ne l'est pas</h2>
 *
 * <p>Le cycle jour/nuit l'est : l'heure avance d'un cran par tick. La météo naturelle ne
 * l'est pas : ses durées sont tirées par l'aléa du monde, semé différemment à chaque
 * lancement. Le cycle météo du jeu est donc coupé, et la météo <strong>pilotée par des
 * événements planifiés</strong> — ce qui éprouve les deux à la fois. Les événements
 * passent par la file du jeu ({@link TimerQueue}), celle de {@code /schedule}.
 *
 * <h2>L'ouvrage</h2>
 *
 * <p>Dans le chunk (0, 0), en Y = {@value #FLOOR_Y}, sous le ciel ouvert — rien n'y est
 * jamais généré au-dessus :
 * <ul>
 *   <li>deux capteurs de lumière du jour, l'un normal, l'autre inversé : leur puissance
 *       suit l'heure et l'assombrissement du ciel par la pluie ;
 *   <li>une lampe que chaque événement allume ou éteint, en posant ou retirant un bloc de
 *       redstone : la trace de l'événement lui-même.
 * </ul>
 *
 * <p>Les capteurs se mettent à jour quand l'horloge <strong>absolue</strong> du jeu est
 * multiple de vingt. Le lancement attend donc ce moment ({@link #canLaunch}) et fixe
 * l'heure, la pluie et son intensité : d'une exécution à l'autre, l'ouvrage vit alors les
 * mêmes ticks dans le même ciel.
 *
 * <h2>Le calendrier</h2>
 *
 * <p>Un événement tous les {@value #EVENT_EVERY} ticks après le lancement ; en plus de la
 * lampe : pluie à 1 500, éclaircie à 3 500, six heures de jeu sautées à 5 000, pluie à
 * 8 500, éclaircie à 9 500. Départ à l'heure {@value #START_DAY_TIME}, avant l'aube :
 * l'ouvrage voit le lever du soleil, la pluie de jour, le coucher et la nuit. Pas
 * d'orage : la foudre tombe au hasard.
 */
public final class WeatherClock implements BenchFixture {

    /** Dalle de pierre, très au-dessus de tout relief généré. */
    static final int FLOOR_Y = 300;

    /** Haut de la tranche vidée et hachée. */
    static final int TOP_Y = 304;

    private static final int Y = FLOOR_Y + 1;

    /** Heure du jeu au lancement : peu avant l'aube. */
    static final long START_DAY_TIME = 22_000L;

    /** Intervalle entre deux événements planifiés, en ticks. */
    static final int EVENT_EVERY = 500;

    /** Événements planifiés : un tous les {@value #EVENT_EVERY} ticks jusqu'à 10 500. */
    static final int EVENTS = 21;

    private static final BlockPos SENSOR_DAY = new BlockPos(2, Y, 2);
    private static final BlockPos SENSOR_NIGHT = new BlockPos(4, Y, 2);
    private static final BlockPos LAMP = new BlockPos(6, Y, 6);
    private static final BlockPos SWITCH = new BlockPos(6, Y, 5);

    /** Pluie sans fin, ou ciel clair sans fin : le cycle météo du jeu est coupé. */
    private static final int FOREVER = 1_000_000;

    private static boolean serializerRegistered;

    private final PartWatch watch =
            new PartWatch("capteur-jour", "capteur-nuit", "lampe-événements", "pluie");

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

    @Override
    public int build(ServerLevel level) {
        // Coupé dès la pose : une pluie tirée au hasard pendant l'attente du lancement
        // n'aurait plus le temps de commencer.
        level.getGameRules().getRule(GameRules.RULE_WEATHER_CYCLE).set(false, level.getServer());
        level.getGameRules().getRule(GameRules.RULE_DAYLIGHT).set(true, level.getServer());
        int failed = FixtureBlocks.clearBox(level, this);
        failed += place(level, SENSOR_DAY, Blocks.DAYLIGHT_DETECTOR.defaultBlockState());
        failed += place(level, SENSOR_NIGHT, Blocks.DAYLIGHT_DETECTOR.defaultBlockState()
                .setValue(DaylightDetectorBlock.INVERTED, true));
        failed += place(level, LAMP, Blocks.REDSTONE_LAMP.defaultBlockState());
        return failed;
    }

    @Override
    public int launchSteps() {
        return 1;
    }

    /** Les capteurs se mettent à jour quand l'horloge absolue est multiple de vingt. */
    @Override
    public boolean canLaunch(ServerLevel level) {
        return level.getGameTime() % 20 == 0;
    }

    /** Fixe l'heure et le ciel, puis planifie le premier événement. */
    @Override
    public int launch(ServerLevel level, int step) {
        level.setDayTime(START_DAY_TIME);
        level.setWeatherParameters(FOREVER, 0, false, false);
        level.setRainLevel(0.0F);
        level.setThunderLevel(0.0F);
        registerSerializer();
        long origin = level.getGameTime();
        queue(level.getServer()).schedule(Step.id(0), origin + EVENT_EVERY, new Step(0, origin));
        return 0;
    }

    @Override
    public void observe(ServerLevel level) {
        watch.record(level.getBlockState(SENSOR_DAY).getValue(BlockStateProperties.POWER),
                level.getBlockState(SENSOR_NIGHT).getValue(BlockStateProperties.POWER),
                flag(level, LAMP, BlockStateProperties.LIT),
                level.isRaining() ? 1 : 0);
    }

    @Override
    public String activity() {
        return watch.summary();
    }

    @Override
    public boolean everyPartMoved() {
        return watch.everyPartMoved();
    }

    /** Ce que fait l'événement de rang {@code index}, à {@code (index + 1) × 500} ticks. */
    static void fire(ServerLevel level, int index) {
        boolean on = !level.getBlockState(SWITCH).is(Blocks.REDSTONE_BLOCK);
        place(level, SWITCH, on ? Blocks.REDSTONE_BLOCK.defaultBlockState()
                : Blocks.AIR.defaultBlockState());
        switch ((index + 1) * EVENT_EVERY) {
            case 1_500, 8_500 -> level.setWeatherParameters(0, FOREVER, true, false);
            case 3_500, 9_500 -> level.setWeatherParameters(FOREVER, 0, false, false);
            case 5_000 -> level.setDayTime(level.getDayTime() + 6_000L);
            default -> {
                // Événement sans effet sur le ciel : la lampe seule en garde la trace.
            }
        }
    }

    private static TimerQueue<MinecraftServer> queue(MinecraftServer server) {
        return server.getWorldData().overworldData().getScheduledEvents();
    }

    /**
     * Enregistre le type d'événement auprès du jeu, une fois.
     *
     * <p>La file des événements est écrite à chaque sauvegarde du monde : un type inconnu
     * du registre y ferait échouer la sauvegarde automatique.
     */
    private static synchronized void registerSerializer() {
        if (!serializerRegistered) {
            TimerCallbacks.SERVER_CALLBACKS.register(new StepSerializer());
            serializerRegistered = true;
        }
    }

    /** Un événement planifié : exécute son rang, puis planifie le suivant. */
    static final class Step implements TimerCallback<MinecraftServer> {

        private final int index;
        private final long origin;

        Step(int index, long origin) {
            this.index = index;
            this.origin = origin;
        }

        static String id(int index) {
            return "rustforgex:bench_g14#" + index;
        }

        @Override
        public void handle(MinecraftServer server, TimerQueue<MinecraftServer> queue,
                long gameTime) {
            fire(server.overworld(), index);
            int next = index + 1;
            if (next < EVENTS) {
                queue.schedule(id(next), origin + (long) (next + 1) * EVENT_EVERY,
                        new Step(next, origin));
            }
        }
    }

    /** Écriture et relecture d'un {@link Step} dans la sauvegarde du monde. */
    private static final class StepSerializer
            extends TimerCallback.Serializer<MinecraftServer, Step> {

        // Forge 47.4 marque ce constructeur « à retirer » et propose
        // `ResourceLocation.fromNamespaceAndPath`. Mais celle-ci n'existe pas dans
        // Minecraft 1.20.1 : c'est un ajout de Forge en cours de branche 47, et le mod
        // déclare accepter tout Forge 47 (`forge_version_range=[47,)`). Le constructeur,
        // lui, existe dans toute cette plage.
        @SuppressWarnings("removal")
        StepSerializer() {
            super(new ResourceLocation("rustforgex", "bench_g14_step"), Step.class);
        }

        @Override
        public void serialize(CompoundTag tag, Step step) {
            tag.putInt("index", step.index);
            tag.putLong("origin", step.origin);
        }

        @Override
        public Step deserialize(CompoundTag tag) {
            return new Step(tag.getInt("index"), tag.getLong("origin"));
        }
    }
}
