package dev.rustforgex.bench;

import net.minecraft.server.MinecraftServer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.Deque;
import java.util.List;
import java.util.Locale;

/**
 * C-36, PARTIE 22 : charge de jeu scriptée et reproductible.
 *
 * <p>Cahier des charges : PARTIE 21.3 point 2 (monde et charge figés), PARTIE 22
 * (profils de modpack). Maturité : {@code STABLE}.
 *
 * <h2>Pourquoi un script et non un joueur</h2>
 *
 * <p>La mesure de C-36 est une comparaison <strong>appariée</strong> : la même charge
 * doit s'exécuter dans les deux configurations, cinq fois chacune. Un humain ne rejoue
 * pas dix fois la même session, et l'écart cherché — de l'ordre du dixième de
 * milliseconde par tick — serait noyé sous la différence entre deux sessions.
 *
 * <p>Un joueur reste irremplaçable pour l'autre question, « est-ce que ça se sent ? ».
 * Elle ne se mesure pas en appariant, elle se constate.
 *
 * <h2>Pourquoi les quatre premières campagnes ne valaient qu'à moitié</h2>
 *
 * <p>Elles ont mesuré un serveur <strong>au repos</strong> : douze types d'événements
 * distribués là où un serveur joué en poste des centaines, un tick de 1,6 à 2,5 ms là
 * où un serveur joué en dure vingt à quarante. Le pourcentage relatif y est surestimé —
 * le dénominateur est minuscule — et le coût absolu sous-estimé, puisque presque aucune
 * méthode sondée n'est réellement atteinte.
 *
 * <h2>Idempotence</h2>
 *
 * <p>Le monde est réutilisé d'une exécution à l'autre. Une charge qui s'ajouterait à
 * elle-même ferait dériver la mesure d'exécution en exécution, et la comparaison
 * n'aurait plus de sens. Chaque profil commence donc par remettre le monde dans le même
 * état — entités supprimées, chargements forcés retirés — avant de reconstruire sa
 * charge à l'identique.
 *
 * <h2>Inerte par défaut</h2>
 *
 * <p>Sans {@link #PROPERTY}, cette classe ne fait rien et n'est jamais appelée. Comme
 * {@link MacroRecorder}, elle n'existe que pendant un benchmark : le code du jeu n'est
 * jamais muté en partie normale.
 */
public final class LoadProfile {

    /** Profil demandé. Sans cette propriété, aucune charge n'est appliquée. */
    public static final String PROPERTY = "rustforgex.bench.load";

    /** Aucun profil : le serveur reste au repos, comme les quatre premières campagnes. */
    public static final String NONE = "none";

    /** Régions maintenues chargées et ticks de blocs accélérés. */
    public static final String CHUNKS = "chunks";

    /** Le profil {@link #CHUNKS}, plus des entités qui pensent et se déplacent. */
    public static final String MOBS = "mobs";

    /**
     * Le profil {@code heavy} de la PARTIE 22 : 8 000 entités, 2 500 chunks.
     *
     * <p>Le cahier des charges l'appelle « profil principal de benchmark ». Les profils
     * précédents lui sont très inférieurs : {@link #MOBS} charge 256 chunks et 200
     * entités, soit <strong>moins que la ligne {@code vanilla}</strong> de la table, qui
     * en demande 400 et 200. Toutes les mesures faites jusqu'ici décrivent donc un
     * serveur au repos — MSPT de l'ordre de 4 ms, là où un serveur qui souffre est
     * entre 20 et 40 ms.
     *
     * <p>Cela compte directement pour le budget : la PARTIE 5.5 exprime le coût toléré
     * en pourcentage du MSPT, donc un serveur rapide rend le budget dur à tenir alors
     * que le coût absolu y importe le moins.
     */
    public static final String HEAVY = "heavy";

    /**
     * Côté du carré de régions maintenues chargées, en blocs.
     *
     * <p>{@code /forceload add} refuse au-delà de 256 régions par commande : un carré de
     * 256 blocs en fait exactement 16 sur 16. Demander plus ferait échouer la commande,
     * et la charge serait silencieusement absente.
     */
    static final int FORCELOAD_SPAN = 256;

    /** Entités invoquées par le profil {@link #MOBS}. */
    static final int MOB_COUNT = 200;

    /** Entités invoquées par le profil {@link #HEAVY} (PARTIE 22). */
    static final int HEAVY_MOB_COUNT = 8_000;

    /** Chunks maintenus chargés par le profil {@link #HEAVY} (PARTIE 22). */
    static final int HEAVY_CHUNKS = 2_500;

    /**
     * Chunks par commande {@code /forceload add}.
     *
     * <p>La commande refuse au-delà de 256 régions. Atteindre 2 500 chunks demande donc
     * plusieurs commandes, disposées en bandes adjacentes — et non une seule commande
     * plus large, qui échouerait en silence et laisserait la charge absente.
     */
    static final int FORCELOAD_PER_COMMAND = 256;

    /**
     * Vitesse des ticks aléatoires.
     *
     * <p>Le défaut vaut 3. Vingt reste loin des valeurs qui font fondre un serveur, et
     * suffit à donner du travail réel aux 256 régions chargées.
     */
    static final int RANDOM_TICK_SPEED = 20;

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    private LoadProfile() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /** @return le profil demandé, en minuscules, ou {@link #NONE} */
    public static String requested() {
        String value = System.getProperty(PROPERTY, NONE);
        return value.isBlank() ? NONE : value.trim().toLowerCase(Locale.ROOT);
    }

    /**
     * Commandes d'un profil, dans l'ordre où elles doivent s'exécuter.
     *
     * <p>Fonction pure : c'est ce qui permet de vérifier l'idempotence et l'ordre sans
     * démarrer un serveur.
     *
     * @param profile nom du profil
     * @return la suite de commandes, vide si le profil est inconnu ou {@link #NONE}
     */
    public static List<String> commandsFor(String profile) {
        List<String> commands = new ArrayList<>();
        if (profile == null || NONE.equals(profile)) {
            return List.of();
        }
        if (!CHUNKS.equals(profile) && !MOBS.equals(profile) && !HEAVY.equals(profile)) {
            return List.of();
        }

        // 1. Figer ce qui varierait tout seul. Un cycle jour/nuit, une météo ou des
        //    apparitions naturelles rendraient deux exécutions incomparables.
        commands.add("gamerule doDaylightCycle false");
        commands.add("gamerule doWeatherCycle false");
        commands.add("gamerule doMobSpawning false");
        commands.add("gamerule doFireTick false");
        commands.add("time set noon");
        commands.add("weather clear");

        // 2. Remettre le monde dans son état de départ. C'est ce qui rend le profil
        //    idempotent : sans cela, la charge de l'exécution précédente s'ajouterait.
        commands.add("kill @e[type=!minecraft:player]");
        commands.add("forceload remove all");

        // 3. Charger les régions AVANT d'y placer quoi que ce soit : une entité dans une
        //    région non chargée ne tick pas du tout, elle reste figée. L'ordre n'est pas
        //    un détail de présentation.
        int half = FORCELOAD_SPAN / 2;
        if (HEAVY.equals(profile)) {
            appendHeavyForceload(commands);
        } else {
            commands.add(String.format(Locale.ROOT, "forceload add %d %d %d %d",
                    -half, -half, half - 1, half - 1));
        }
        commands.add("gamerule randomTickSpeed " + RANDOM_TICK_SPEED);

        if (HEAVY.equals(profile)) {
            // Le cramming tuerait les entités entassées avant qu'elles ne ticks : à
            // huit mille sur la zone, l'entassement est inévitable et il ne doit pas
            // faire disparaître la charge qu'on vient d'installer.
            commands.add("gamerule maxEntityCramming 0");
            appendMobs(commands, HEAVY_MOB_COUNT, heavySpan());
        } else if (MOBS.equals(profile)) {
            // 4. Des entités qui pensent : IA, recherche de chemin, collisions. Elles
            //    sont réparties sur la zone chargée, et marquées persistantes pour ne
            //    pas disparaître faute de joueur à proximité.
            appendMobs(commands, MOB_COUNT, FORCELOAD_SPAN);
        }
        return List.copyOf(commands);
    }

    /**
     * Côté du carré chargé par {@link #HEAVY}, en blocs.
     *
     * <p>2 500 chunks font un carré de 50 sur 50, soit 800 blocs de côté.
     */
    static int heavySpan() {
        int side = (int) Math.round(Math.sqrt(HEAVY_CHUNKS));
        return side * 16;
    }

    /**
     * Charge les 2 500 chunks par bandes de 256 au plus.
     *
     * <p>Une seule commande couvrant tout le carré serait refusée sans que rien ne le
     * signale au benchmark : la charge manquerait, et la mesure porterait sur un serveur
     * vide en croyant mesurer un serveur chargé.
     */
    private static void appendHeavyForceload(List<String> commands) {
        int side = (int) Math.round(Math.sqrt(HEAVY_CHUNKS));
        int halfChunks = side / 2;
        // Une bande de `rows` rangées de `side` chunks tient sous la limite par commande.
        int rows = Math.max(1, FORCELOAD_PER_COMMAND / side);
        for (int row = -halfChunks; row < side - halfChunks; row += rows) {
            int last = Math.min(row + rows - 1, side - halfChunks - 1);
            commands.add(String.format(Locale.ROOT, "forceload add %d %d %d %d",
                    (-halfChunks) * 16, row * 16, (side - halfChunks - 1) * 16 + 15,
                    last * 16 + 15));
        }
    }

    /** Invoque `count` entités persistantes, réparties sur un carré de `span` blocs. */
    private static void appendMobs(List<String> commands, int count, int span) {
        int half = span / 2;
        for (int i = 0; i < count; i++) {
            int x = Math.floorMod(i * 7 * 13, span) - half + 8;
            int z = Math.floorMod(i * 11 * 13, span) - half + 8;
            commands.add(String.format(Locale.ROOT,
                    "execute positioned %d 0 %d run summon minecraft:cow ~ ~ ~ "
                            + "{PersistenceRequired:1b}", x, z));
        }
    }

    /**
     * Répartit déterministiquement une entité sur la zone chargée.
     *
     * <p>Un simple produit modulo : reproductible d'une exécution à l'autre, ce qu'un
     * générateur aléatoire non ensemencé ne serait pas.
     */
    private static int spread(int index, int stride) {
        return Math.floorMod(index * stride * 13, FORCELOAD_SPAN);
    }

    /**
     * Applique le profil demandé sur le serveur.
     *
     * <p>À appeler depuis le fil autoritatif, pendant l'échauffement : la charge doit
     * être en place et stabilisée bien avant que la fenêtre de mesure s'ouvre.
     *
     * <p>N'échoue jamais. Une commande refusée est journalisée et la suivante est tentée :
     * un profil partiellement appliqué reste préférable à un benchmark interrompu, à
     * condition que le journal le dise.
     *
     * @param server serveur sur lequel exécuter les commandes
     * @param profile profil à appliquer
     * @return le nombre de commandes exécutées sans erreur
     */
    /**
     * Commandes exécutées au plus par tick lors d'une application étalée.
     *
     * <p>Le profil {@code heavy} en compte plus de huit mille. Les exécuter d'un bloc
     * les ferait tenir dans un seul tick, qui durerait des dizaines de secondes : le
     * chien de garde du serveur y verrait un blocage et pourrait l'arrêter. Deux cents
     * par tick étalent l'installation sur une quarantaine de ticks, soit deux secondes,
     * bien à l'intérieur de l'échauffement.
     */
    public static final int COMMANDS_PER_TICK = 200;

    /**
     * Exécute au plus {@link #COMMANDS_PER_TICK} commandes en attente.
     *
     * <p>À appeler à chaque tick tant que la file n'est pas vide. Étaler n'est pas un
     * confort : c'est ce qui empêche l'installation de la charge de ressembler à un
     * blocage du serveur.
     *
     * @param server serveur sur lequel exécuter
     * @param pending file des commandes restantes, consommée au fur et à mesure
     * @return le nombre de commandes exécutées sans erreur pendant ce tick
     */
    public static int pump(MinecraftServer server, Deque<String> pending) {
        int applied = 0;
        for (int i = 0; i < COMMANDS_PER_TICK && !pending.isEmpty(); i++) {
            String command = pending.poll();
            try {
                server.getCommands().performPrefixedCommand(
                        server.createCommandSourceStack(), command);
                applied++;
            } catch (RuntimeException | LinkageError e) {
                LOGGER.warn("Commande de charge refusée : « {} »", command, e);
            }
        }
        return applied;
    }

    public static int apply(MinecraftServer server, String profile) {
        List<String> commands = commandsFor(profile);
        if (commands.isEmpty()) {
            return 0;
        }
        LOGGER.info("Profil de charge « {} » : {} commandes.", profile, commands.size());

        int applied = 0;
        int failed = 0;
        for (String command : commands) {
            try {
                server.getCommands().performPrefixedCommand(
                        server.createCommandSourceStack(), command);
                applied++;
            } catch (RuntimeException | LinkageError e) {
                failed++;
                if (failed <= 3) {
                    LOGGER.warn("Commande de charge refusée : « {} »", command, e);
                }
            }
        }
        LOGGER.info("Profil de charge « {} » appliqué : {} commandes exécutées, {} refusées.",
                profile, applied, failed);
        return applied;
    }
}
