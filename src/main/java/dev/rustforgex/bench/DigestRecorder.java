package dev.rustforgex.bench;

import dev.rustforgex.RfxRuntime;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.server.ServerLifecycleHooks;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.List;
import java.util.Locale;

/**
 * C-36 : exécution d'un test de gameplay — G-03 (génération), G-01 (cinq minutes de
 * tick à vide), G-04 (téléportation entre dimensions), G-06 (circuit de redstone), G-07
 * (machines de mods), G-08
 * (sauvegarde et rechargement), G-09 (chunks chargés et déchargés en masse), G-11
 * (commandes), G-12 (crafting et conteneurs) ou G-14 (météo, jour et nuit, événements
 * planifiés) — jusqu'à l'empreinte de l'état.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Chaque scénario est exécuté deux fois, sans
 * RUSTFORGE-X puis avec, et le critère est l'<strong>égalité d'état</strong>, pas la
 * performance. Maturité : {@code STABLE}.
 *
 * <p>Distinct de {@link MacroRecorder} parce que l'un mesure le temps et l'autre l'état :
 * les deux n'ont ni les mêmes fenêtres, ni les mêmes critères, ni les mêmes sorties.
 *
 * <h2>Déroulé</h2>
 *
 * <ol>
 *   <li>au vingtième tick, le profil {@link LoadProfile#WORLDGEN} fige le monde et force
 *       un carré de chunks ;
 *   <li>tant que tous ne sont pas complets, on attend — c'est la génération ;
 *   <li>puis on laisse reposer : les ticks planifiés à la génération, l'eau et la lave
 *       qui coulent, doivent s'être écoulés avant qu'on regarde ;
 *   <li>empreinte, fichier, arrêt.
 * </ol>
 *
 * <p>Si la génération n'aboutit pas dans le délai, l'empreinte est prise quand même et le
 * fichier le <strong>dit</strong> : une comparaison sur un monde incomplet n'est pas
 * valide, mais un fichier absent serait pire — on ne saurait pas pourquoi.
 *
 * <h2>Armé sur demande, absent sinon</h2>
 *
 * <p>Sans {@link #PROPERTY_OUT}, cette classe ne s'abonne à rien.
 */
public final class DigestRecorder {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Chemin du fichier d'empreinte. Sans cette propriété, l'enregistreur reste absent. */
    public static final String PROPERTY_OUT = "rustforgex.bench.digest.out";

    /** Ticks de repos entre la génération complète et l'empreinte. */
    public static final String PROPERTY_SETTLE = "rustforgex.bench.digest.settle";

    /** Ticks accordés à la génération avant de prendre l'empreinte malgré tout. */
    public static final String PROPERTY_TIMEOUT = "rustforgex.bench.digest.timeout";

    /**
     * Scénario joué : {@code g03} (défaut), {@code g01}, {@code g06}, {@code g08},
     * {@code g04}, {@code g07}, {@code g09}, {@code g11}, {@code g12} ou {@code g14}.
     */
    public static final String PROPERTY_SCENARIO = "rustforgex.bench.scenario";

    /**
     * G-01 : « démarrage serveur dédié, chargement du monde, 5 minutes de tick à vide ».
     *
     * <p>Six mille ticks : cinq minutes au rythme nominal de vingt par seconde. Compter en
     * ticks plutôt qu'en secondes rend les exécutions comparables même si l'une rame : le
     * jeu y a fait exactement le même nombre de pas.
     */
    static final int G01_TICKS = 6_000;

    /**
     * Demi-côté du carré relu par G-01, en chunks, autour du chunk d'apparition.
     *
     * <p>Sans joueur, seuls les chunks d'apparition restent chargés, dans un rayon d'une
     * dizaine de chunks. Huit reste à l'intérieur avec de la marge : un chunk absent rend
     * la comparaison invalide, il ne la fausse pas.
     */
    static final int G01_RADIUS = 8;

    /** Durée de G-06 après le lancement du circuit, en ticks (PARTIE 20.3.4). */
    static final int G06_TICKS = 10_000;

    /**
     * Durée de G-12 après la pose de la ligne de conteneurs, en ticks : la cuisson de 32
     * minerais en prend 6 400, avec de la marge.
     */
    static final int G12_TICKS = 8_000;

    /** Durée de G-14 après le lancement : 21 événements planifiés, jusqu'à 10 500. */
    static final int G14_TICKS = 11_000;

    /**
     * Durée de G-11 après l'exécution des commandes : rien n'y bouge, deux empreintes
     * suffisent à voir que leur effet sur le monde tient.
     */
    static final int G11_TICKS = 1_000;

    /** Durée de G-04 après l'invocation des voyageurs : trente sauts de dimension. */
    static final int G04_TICKS = 3_000;

    /** Durée de G-07 après la pose des machines : 30 minutes de jeu (PARTIE 20.3.4). */
    static final int G07_TICKS = 36_000;

    /**
     * Durée d'un test à ouvrage (G-06, G-12), pour le mettre au point plus vite ; sinon
     * {@value #G06_TICKS} pour G-06 et {@value #G12_TICKS} pour G-12. Deux fichiers de
     * durées différentes n'ont pas les mêmes composantes, et la comparaison le dit.
     */
    public static final String PROPERTY_FIXTURE_TICKS = "rustforgex.bench.fixture.ticks";

    /** Déroulé de l'ouvrage de G-06 ou G-12 ; {@code null} pour les autres tests. */
    private FixtureRun fixtureRun;

    /** Crafting de G-12 ; {@code null} pour les autres tests. */
    private ResultSweep sweep;

    /** Les scénarios de la PARTIE 20.3.4 que ce harnais sait jouer. */
    enum Scenario {
        /** Génération de 2 000 chunks, comparée à la référence. */
        G03("G-03"),
        /** Démarrage, chargement du monde, cinq minutes de tick à vide. */
        G01("G-01"),
        /** Sauvegarde, arrêt, rechargement, comparaison d'état. */
        G08("G-08"),
        /** Circuit de redstone (horloges, comparateur, pistons) sur 10 000 ticks. */
        G06("G-06"),
        /** Crafting, inventaires, conteneurs. */
        G12("G-12"),
        /** Météo, cycle jour/nuit, événements planifiés. */
        G14("G-14"),
        /** Commandes vanilla et de mods. */
        G11("G-11"),
        /** Téléportation entre dimensions. */
        G04("G-04"),
        /** Chargement et déchargement massif de chunks, fenêtre en mouvement rapide. */
        G09("G-09"),
        /** Machines de mods (Create, Mekanism) sur 30 minutes. */
        G07("G-07");

        final String id;

        Scenario(String id) {
            this.id = id;
        }

        static Scenario of(String value) {
            String v = value == null ? "" : value.trim();
            if ("g01".equalsIgnoreCase(v)) {
                return G01;
            }
            if ("g06".equalsIgnoreCase(v)) {
                return G06;
            }
            if ("g07".equalsIgnoreCase(v)) {
                return G07;
            }
            if ("g09".equalsIgnoreCase(v)) {
                return G09;
            }
            if ("g04".equalsIgnoreCase(v)) {
                return G04;
            }
            if ("g11".equalsIgnoreCase(v)) {
                return G11;
            }
            if ("g14".equalsIgnoreCase(v)) {
                return G14;
            }
            if ("g12".equalsIgnoreCase(v)) {
                return G12;
            }
            return "g08".equalsIgnoreCase(v) ? G08 : G03;
        }
    }

    /**
     * Repos par défaut : dix secondes de jeu.
     *
     * <p>Assez pour qu'une source d'eau générée s'étale, ce qui prend quelques dizaines
     * de ticks ; et identique d'une exécution à l'autre, ce qui seul importe.
     */
    static final int DEFAULT_SETTLE_TICKS = 200;

    /** Délai par défaut : vingt minutes de ticks, très au-delà d'une génération normale. */
    static final int DEFAULT_TIMEOUT_TICKS = 24_000;

    /** Tick d'application du profil, après la mise en route du serveur. */
    private static final int APPLY_AT_TICK = 20;

    /** Cadence de vérification de la génération, en ticks. */
    private static final int POLL_EVERY_TICKS = 20;

    /**
     * Version du fichier. 2 : empreinte par section et ordonnée de la première section.
     *
     * <p>L'empreinte des blocs d'un chunk se compose désormais de celles de ses sections :
     * un fichier de version 1 ne se compare pas à un fichier de version 2.
     */
    private static final int SCHEMA = 2;

    private final Path out;
    private final String label;
    private final Scenario scenario;

    /**
     * Phase de G-08 : {@code save} (défaut) ou {@code load}.
     *
     * <p>G-08 se joue en deux démarrages du même monde. Le premier génère comme G-03,
     * prend l'empreinte, puis sauvegarde dans le même tick ; le second relit les mêmes
     * chunks depuis le disque, sans les faire tourner, et reprend l'empreinte. Ce que le
     * jeu sauvegarde doit être exactement ce qu'il relit : aucun hasard ici, l'égalité
     * est stricte.
     */
    public static final String PROPERTY_G08_PHASE = "rustforgex.bench.g08.phase";

    private final boolean reloadPhase =
            "load".equalsIgnoreCase(System.getProperty(PROPERTY_G08_PHASE, "save").trim());
    private final int settleTicks;
    private final int timeoutTicks;

    /** Carré relu, bornes incluses. Fixé à l'armement pour G-03, au chargement pour G-01. */
    private int minX;
    private int maxX;
    private int minZ;
    private int maxZ;

    private int seen;
    private int appliedAt = -1;
    private int readyAt = -1;
    private Deque<String> pending;
    private boolean finished;
    private boolean squareForced;

    /**
     * Marque le carré comme forcé, sans attendre que ses chunks soient générés.
     *
     * <p>{@code /forceload add} charge chaque chunk de façon SYNCHRONE, dans le tick qui
     * exécute la commande. Le serveur prend alors du retard sur son horaire, et le chien
     * de garde ne mesure pas un tick isolé mais ce retard cumulé, qui ne se rattrape pas
     * d'un tick lent à l'autre. Étaler les commandes n'y changeait rien : une référence
     * sans RUSTFORGE-X a été arrêtée après 120 s de retard accumulé sur six bandes.
     *
     * <p>{@code setChunkForced} pose le ticket de forçage et rend la main : les threads de
     * génération travaillent en arrière-plan, le serveur continue à vingt ticks par
     * seconde, et {@link #countReady} attend que les chunks soient complets. C'est aussi
     * plus proche du jeu réel, où la génération est asynchrone. Le carré forcé est le même
     * — {@link LoadProfile#forcedChunkRange} — et {@code forceload remove all} le libère
     * comme s'il avait été posé par la commande.
     */
    private void forceSquare(ServerLevel level) {
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                level.setChunkForced(x, z, true);
            }
        }
        LOGGER.info("{} : {} chunks marqués forcés, génération en arrière-plan.",
                scenario.id, expected());
    }

    private DigestRecorder(Path out, String label, Scenario scenario, int settleTicks,
            int timeoutTicks) {
        this.out = out;
        this.label = label;
        this.scenario = scenario;
        this.settleTicks = settleTicks;
        this.timeoutTicks = timeoutTicks;
        int[] range = LoadProfile.forcedChunkRange(LoadProfile.WORLDGEN_CHUNKS);
        this.minX = range[0];
        this.maxX = range[1];
        this.minZ = range[0];
        this.maxZ = range[1];
        if (scenario == Scenario.G09) {
            int[] strip = MovingWindow.strip();
            this.minX = strip[0];
            this.maxX = strip[1];
            this.minZ = strip[2];
            this.maxZ = strip[3];
        }
    }

    /** Fenêtre mobile de G-09 ; {@code null} pour les autres tests. */
    private MovingWindow window;

    /** Arme l'enregistreur si {@link #PROPERTY_OUT} est renseignée. */
    public static void armIfRequested() {
        String target = System.getProperty(PROPERTY_OUT, "");
        if (target.isBlank()) {
            return;
        }
        Scenario scenario = Scenario.of(System.getProperty(PROPERTY_SCENARIO));
        DigestRecorder recorder = new DigestRecorder(Path.of(target.trim()),
                System.getProperty(MacroRecorder.PROPERTY_LABEL, "unlabelled"),
                scenario,
                intProperty(PROPERTY_SETTLE, DEFAULT_SETTLE_TICKS),
                intProperty(PROPERTY_TIMEOUT, DEFAULT_TIMEOUT_TICKS));
        MinecraftForge.EVENT_BUS.register(recorder);
        LOGGER.info("Test {} armé, empreinte dans {}. Le serveur s'arrêtera ensuite.",
                scenario.id, target);
    }

    /** Fait avancer le scénario d'un pas, en fin de tick. */
    @SubscribeEvent(priority = EventPriority.LOWEST)
    public void onTick(final TickEvent.ServerTickEvent event) {
        if (event.phase != TickEvent.Phase.END || finished) {
            return;
        }
        seen++;
        MinecraftServer server = ServerLifecycleHooks.getCurrentServer();
        if (server == null) {
            return;
        }
        if (scenario == Scenario.G08 && reloadPhase) {
            tickReload(server);
            return;
        }
        if (scenario == Scenario.G01) {
            tickIdle(server);
            return;
        }
        if (scenario == Scenario.G06 || scenario == Scenario.G12
                || scenario == Scenario.G14 || scenario == Scenario.G11
                || scenario == Scenario.G04 || scenario == Scenario.G07) {
            tickFixture(server);
            return;
        }
        if (seen == APPLY_AT_TICK) {
            // Les `forceload add` sont retirés de la file et remplacés par forceSquare :
            // voir sa documentation. Le reste du profil — figer le jeu, vider les forçages
            // précédents — passe par les commandes, comme pour les profils de charge.
            pending = new ArrayDeque<>(LoadProfile.commandsFor(LoadProfile.WORLDGEN).stream()
                    .filter(command -> !command.startsWith("forceload add"))
                    .toList());
            appliedAt = seen;
        }
        if (pending == null) {
            return;
        }
        if (!pending.isEmpty()) {
            // UNE commande par tick. Chaque `forceload add` génère sa bande de chunks de
            // façon synchrone, dans le tick qui l'exécute : toutes ensemble, les 2 025
            // chunks tenaient en un ou deux ticks de 60 à 80 secondes, à un souffle du
            // chien de garde du serveur (120 s). Machine chargée, une référence sans
            // RUSTFORGE-X l'a franchi et le serveur a été arrêté. Une bande par tick reste
            // très en deçà, quelle que soit la charge.
            LoadProfile.pump(server, new ArrayDeque<>(java.util.List.of(pending.poll())));
            return;
        }
        if (scenario == Scenario.G09) {
            // G-09 : la course de la fenêtre avant tout ; la bande parcourue n'est forcée
            // en entier, puis hachée comme G-03, qu'une fois le retour achevé.
            if (window == null) {
                window = new MovingWindow();
                window.start();
            }
            if (!window.tick(server.overworld())) {
                return;
            }
        }
        if (!squareForced) {
            forceSquare(server.overworld());
            squareForced = true;
            return;
        }
        if (seen % POLL_EVERY_TICKS != 0) {
            return;
        }

        ServerLevel level = server.overworld();
        int ready = countReady(level);
        if (ready == expected() && readyAt < 0) {
            readyAt = seen;
            LOGGER.info("{} : {} chunks générés en {} ticks, repos de {} ticks.",
                    scenario.id, ready, seen - appliedAt, settleTicks);
        }
        boolean settled = readyAt >= 0 && seen - readyAt >= settleTicks;
        boolean timedOut = seen - appliedAt >= timeoutTicks;
        if (settled || timedOut) {
            finished = true;
            finish(server, level, ready, timedOut && !settled);
        }
    }

    /**
     * G-01 : on ne touche à rien, on laisse tourner, on regarde.
     *
     * <p>Rien n'est figé — c'est la différence avec G-03. Les règles du jeu sont celles
     * d'un serveur réel au repos : cycle jour-nuit, météo, ticks planifiés, entités de
     * bloc. Sans joueur, ni apparition naturelle ni tick aléatoire n'ont lieu : le jeu les
     * réserve aux chunks proches d'un joueur.
     */
    /**
     * G-08, phase de sauvegarde : appelée dans le même tick que l'empreinte.
     *
     * <p>Rien ne doit tourner entre l'empreinte et l'écriture sur disque, sans quoi un
     * écoulement d'eau ou un tick planifié ferait différer l'état sauvegardé de l'état
     * relevé, et la comparaison accuserait la sauvegarde d'un écart qui n'est pas d'elle.
     * Les commandes s'exécutent ici, de façon synchrone. Les chunks forcés sont libérés
     * d'abord : au rechargement, rien ne doit les remettre en mouvement avant qu'on les
     * relise.
     */
    private void saveForReload(MinecraftServer server) {
        LoadProfile.pump(server,
                new ArrayDeque<>(java.util.List.of("forceload remove all", "save-all flush")));
        LOGGER.info("G-08 : état relevé puis sauvegardé dans le même tick.");
    }

    /**
     * G-08, phase de rechargement : relit le carré depuis le disque, au premier tick.
     *
     * <p>{@code getChunk} charge un chunk complet sans le faire tourner : son niveau de
     * ticket reste en deçà du seuil des chunks actifs. On relit donc l'état sauvegardé,
     * pas ce qu'il deviendrait après quelques ticks.
     */
    private void tickReload(MinecraftServer server) {
        finished = true;
        ServerLevel level = server.overworld();
        appliedAt = 0;
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                level.getChunk(x, z);
            }
        }
        finish(server, level, countReady(level), false);
    }

    /**
     * G-06, G-11, G-12 et G-14 : un ouvrage posé dans le monde et regardé tourner,
     * déroulé par {@link FixtureRun}. G-12 juge en plus toutes les recettes d'atelier
     * ({@link CraftingSweep}), G-11 toutes les commandes enregistrées
     * ({@link CommandSweep}), étalées sur les premiers ticks.
     */
    private void tickFixture(MinecraftServer server) {
        ServerLevel level = server.overworld();
        if (seen == APPLY_AT_TICK) {
            CommandBench commands = new CommandBench();
            DimensionHop hop = new DimensionHop();
            BenchFixture fixture = switch (scenario) {
                case G06 -> new RedstoneCircuit();
                case G11 -> commands;
                case G04 -> hop;
                case G07 -> new ModMachines();
                case G14 -> new WeatherClock();
                default -> new ContainerLine();
            };
            int ticks = intProperty(PROPERTY_FIXTURE_TICKS, switch (scenario) {
                case G06 -> G06_TICKS;
                case G11 -> G11_TICKS;
                case G04 -> G04_TICKS;
                case G07 -> G07_TICKS;
                case G14 -> G14_TICKS;
                default -> G12_TICKS;
            });
            minX = fixture.minChunk();
            maxX = fixture.maxChunk();
            minZ = fixture.minChunk();
            maxZ = fixture.maxChunk();
            fixtureRun = new FixtureRun(fixture, scenario.id, ticks, timeoutTicks);
            fixtureRun.force(server, level);
            sweep = switch (scenario) {
                case G11 -> new CommandSweep(commands);
                case G04 -> hop.trail();
                case G12 -> new CraftingSweep();
                default -> null;
            };
            if (sweep != null) {
                sweep.start(level);
            }
            appliedAt = seen;
            return;
        }
        if (fixtureRun == null) {
            return;
        }
        boolean swept = sweep == null || sweep.step(level);
        if (fixtureRun.tick(level) && swept) {
            finished = true;
            finish(server, level, fixtureRun.ready(level), fixtureRun.timedOut());
        }
    }

    private void tickIdle(MinecraftServer server) {
        if (seen < G01_TICKS) {
            return;
        }
        finished = true;
        ServerLevel level = server.overworld();
        // Le carré est centré sur le chunk d'apparition, connu seulement une fois le monde
        // chargé. Il est écrit dans le fichier par ses clés : deux mondes à apparitions
        // différentes ne couvriraient pas les mêmes chunks, et la comparaison le dirait.
        int spawnX = level.getSharedSpawnPos().getX() >> 4;
        int spawnZ = level.getSharedSpawnPos().getZ() >> 4;
        minX = spawnX - G01_RADIUS;
        maxX = spawnX + G01_RADIUS;
        minZ = spawnZ - G01_RADIUS;
        maxZ = spawnZ + G01_RADIUS;
        appliedAt = 0;
        // Sans joueur, rien ne garantit que ces chunks soient chargés : sur le serveur de
        // banc, servercore désactive même les chunks d'apparition
        // (`disable-spawn-chunks: true`), et la première exécution de G-01 a relu zéro
        // chunk sur 289. On les recharge donc depuis le disque : c'est l'état persistant du
        // monde après cinq minutes de tick à vide. Ils ont été générés au démarrage
        // (« Preparing spawn area »), sur un rayon plus large que celui relu ici : les
        // relire ne génère rien.
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                level.getChunk(x, z);
            }
        }
        finish(server, level, countReady(level), false);
    }

    private int expected() {
        return (maxX - minX + 1) * (maxZ - minZ + 1);
    }

    private int countReady(ServerLevel level) {
        int ready = 0;
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                if (level.getChunkSource().getChunkNow(x, z) != null) {
                    ready++;
                }
            }
        }
        return ready;
    }

    /** Prend l'empreinte, écrit le fichier, arrête le serveur. Ne lève jamais. */
    private void finish(MinecraftServer server, ServerLevel level, int ready, boolean timedOut) {
        try {
            long start = System.nanoTime();
            Tables tables = fixtureRun != null ? fixtureTables() : digestAll(level);
            if (window != null) {
                // G-09 : la preuve que la course a chargé et déchargé, et la garde contre
                // une course qui n'aurait rien déchargé.
                tables = new Tables(tables.components(), tables.chunks(), tables.sections(),
                        tables.details(), List.of(window.activity()), 0, window.inert());
            }
            long digestMs = (System.nanoTime() - start) / 1_000_000L;
            write(out, label, level, expected(), ready, timedOut, digestMs, tables);
            if (sweep != null) {
                // Les éléments d'une passe — recettes, commandes — se jugent à part : ce
                // ne sont pas des chunks, et un élément bruité ne doit pas rendre bruitée
                // la comparaison de l'ouvrage.
                Path target = sweepPath(out, sweep.suffix());
                write(target, label + sweep.suffix(), level, sweep.expected(), sweep.judged(),
                        !sweep.finished(), 0L, new Tables(sweep.components(), sweep.json(),
                                null, "", List.of(), 0, false));
                LOGGER.info("{} : verdicts de {} éléments dans {}.", scenario.id,
                        sweep.judged(), target.toAbsolutePath());
                String clear = sweep.details();
                if (details && !clear.isEmpty()) {
                    Path text = target.resolveSibling(
                            target.getFileName().toString().replace(".json", "-details.txt"));
                    Files.writeString(text, clear, StandardCharsets.UTF_8);
                }
            }
            if (scenario == Scenario.G08 && !reloadPhase) {
                saveForReload(server);
            }
            LOGGER.info("{} terminé : empreinte de {} chunks en {} ms, dans {}.",
                    scenario.id, ready, digestMs, out.toAbsolutePath());
        } catch (IOException | RuntimeException e) {
            LOGGER.error("{} : empreinte ou écriture impossible, exécution perdue",
                    scenario.id, e);
        }
        MinecraftForge.EVENT_BUS.unregister(this);
        server.halt(false);
    }

    /**
     * Empreintes de tous les chunks, en deux tables JSON : composantes, et sections de
     * blocs.
     *
     * @param chunks une ligne par chunk : ses quatre empreintes, ou {@code null}
     * @param sections une ligne par chunk : l'empreinte de chacune de ses sections
     */
    private record Tables(List<String> components, String chunks, String sections,
            String details, List<String> activity, int failures, boolean inert) {
    }

    /** {@code g12-ref1.json} et {@code -craft} → {@code g12-ref1-craft.json}, à côté. */
    static Path sweepPath(Path out, String suffix) {
        String name = out.getFileName().toString();
        String stem = name.endsWith(".json") ? name.substring(0, name.length() - 5) : name;
        return out.resolveSibling(stem + suffix + ".json");
    }

    /** Les quatre composantes d'un chunk, dans l'ordre de {@link WorldDigest.ChunkDigest}. */
    private static final List<String> STATE_COMPONENTS =
            List.of("blocks", "biomes", "block_entities", "structures");

    /**
     * G-06, G-12 : une composante par empreinte, nommée par son instant ({@code t499} …).
     * Pas de sections : l'empreinte ne couvre que la tranche de l'ouvrage.
     */
    private Tables fixtureTables() {
        return new Tables(fixtureRun.components(), fixtureRun.chunksJson(), null, "",
                fixtureRun.activity(), fixtureRun.failures(), fixtureRun.inert());
    }

    /** Diagnostic des entités de bloc (type et champ par champ), désactivé par défaut. */
    public static final String PROPERTY_DETAILS = "rustforgex.bench.digest.details";

    private final boolean details =
            Boolean.parseBoolean(System.getProperty(PROPERTY_DETAILS, "false").trim());

    /** Une ligne par chunk dans chaque table, ou {@code null} s'il n'est pas chargé. */
    private Tables digestAll(ServerLevel level) {
        StringBuilder chunks = new StringBuilder(LoadProfile.WORLDGEN_CHUNKS * 96);
        StringBuilder sections = new StringBuilder(LoadProfile.WORLDGEN_CHUNKS * 480);
        StringBuilder detailTable = new StringBuilder();
        boolean first = true;
        for (int x = minX; x <= maxX; x++) {
            for (int z = minZ; z <= maxZ; z++) {
                String key = "    \"" + x + ',' + z + "\": ";
                chunks.append(first ? "\n" : ",\n").append(key);
                sections.append(first ? "\n" : ",\n").append(key);
                LevelChunk chunk = level.getChunkSource().getChunkNow(x, z);
                if (details && chunk != null && !chunk.getBlockEntities().isEmpty()) {
                    detailTable.append(detailTable.length() == 0 ? "\n" : ",\n").append(key)
                            .append('[');
                    List<String> entries = WorldDigest.blockEntityDetails(chunk);
                    for (int i = 0; i < entries.size(); i++) {
                        detailTable.append(i == 0 ? "\"" : ", \"")
                                .append(escape(entries.get(i))).append('"');
                    }
                    detailTable.append(']');
                }
                first = false;
                if (chunk == null) {
                    // Absent n'est pas une empreinte : une valeur inventée ici se ferait
                    // passer pour un chunk généré (R-660).
                    chunks.append("null");
                    sections.append("null");
                    continue;
                }
                WorldDigest.ChunkDigest digest = WorldDigest.digest(level, chunk);
                chunks.append(String.format(Locale.ROOT,
                        "[\"%016x\", \"%016x\", \"%016x\", \"%016x\"]",
                        digest.blocks(), digest.biomes(),
                        digest.blockEntities(), digest.structures()));
                sections.append('[');
                long[] perSection = digest.sections();
                for (int i = 0; i < perSection.length; i++) {
                    sections.append(i == 0 ? "\"" : ", \"")
                            .append(String.format(Locale.ROOT, "%016x", perSection[i]))
                            .append('"');
                }
                sections.append(']');
            }
        }
        return new Tables(STATE_COMPONENTS, chunks.toString(), sections.toString(),
                detailTable.toString(), List.of(), 0, false);
    }

    /**
     * Écrit un fichier d'empreinte.
     *
     * @param target fichier à écrire
     * @param runLabel étiquette de l'exécution dans ce fichier
     * @param expected éléments attendus — chunks, ou recettes pour le crafting de G-12
     * @param ready éléments effectivement relevés
     */
    private void write(Path target, String runLabel, ServerLevel level, int expected,
            int ready, boolean timedOut, long digestMs, Tables tables) throws IOException {
        RfxRuntime runtime = RfxRuntime.instance();
        boolean active = runtime != null && runtime.active();
        long probed = runtime == null || runtime.instrumentation() == null
                ? 0L : runtime.instrumentation().methodsProbed();

        StringBuilder json = new StringBuilder(tables.chunks().length()
                + (tables.sections() == null ? 0 : tables.sections().length()) + 4096);
        json.append("{\n");
        json.append("  \"schema\": ").append(SCHEMA).append(",\n");
        json.append("  \"test\": \"").append(scenario.id).append("\",\n");
        json.append("  \"ticks\": ").append(seen).append(",\n");
        json.append("  \"label\": \"").append(escape(runLabel)).append("\",\n");
        json.append("  \"rfx_active\": ").append(active).append(",\n");
        json.append("  \"methods_probed\": ").append(probed).append(",\n");
        json.append("  \"seed\": ").append(level.getSeed()).append(",\n");
        json.append("  \"chunks_expected\": ").append(expected).append(",\n");
        json.append("  \"chunks_ready\": ").append(ready).append(",\n");
        json.append("  \"timed_out\": ").append(timedOut).append(",\n");
        json.append("  \"ready_after_ticks\": ")
                .append(readyAt < 0 ? -1 : readyAt - appliedAt).append(",\n");
        json.append("  \"settle_ticks\": ").append(settleTicks).append(",\n");
        json.append("  \"digest_ms\": ").append(digestMs).append(",\n");
        json.append("  \"placement_failures\": ").append(tables.failures()).append(",\n");
        // Un ouvrage dont une partie n'a jamais bougé ne prouve rien : la comparaison le
        // déclare invalide plutôt que de juger trois ouvrages morts égaux.
        json.append("  \"inert\": ").append(tables.inert()).append(",\n");
        json.append("  \"components\": ").append(stringArray(tables.components()))
                .append(",\n");
        json.append("  \"activity\": ").append(stringArray(tables.activity())).append(",\n");
        json.append("  \"chunks\": {").append(tables.chunks()).append("\n  }");
        if (tables.sections() != null) {
            // Ordonnée de la première section : sans elle, l'indice d'une section dans la
            // table ne dirait pas à quelle hauteur se trouve un écart.
            json.append(",\n  \"min_section_y\": ").append(level.getMinSection());
            json.append(",\n  \"block_sections\": {").append(tables.sections())
                    .append("\n  }");
        }
        if (details) {
            // Diagnostic des entités de bloc : une liste par chunk qui en porte.
            json.append(",\n  \"block_entity_details\": {").append(tables.details())
                    .append("\n  }");
        }
        json.append('\n');
        json.append("}\n");

        Path parent = target.toAbsolutePath().getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.writeString(target, json.toString(), StandardCharsets.UTF_8);
    }

    private static String stringArray(List<String> values) {
        StringBuilder array = new StringBuilder("[");
        for (int i = 0; i < values.size(); i++) {
            array.append(i == 0 ? "\"" : ", \"").append(escape(values.get(i))).append('"');
        }
        return array.append(']').toString();
    }

    private static String escape(String text) {
        return text.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private static int intProperty(String name, int fallback) {
        try {
            String value = System.getProperty(name);
            return value == null || value.isBlank() ? fallback : Integer.parseInt(value.trim());
        } catch (NumberFormatException e) {
            return fallback;
        }
    }
}
