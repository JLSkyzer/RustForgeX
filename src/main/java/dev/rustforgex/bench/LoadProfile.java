package dev.rustforgex.bench;

import net.minecraft.server.MinecraftServer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
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
     * Côté du carré de régions maintenues chargées, en blocs.
     *
     * <p>{@code /forceload add} refuse au-delà de 256 régions par commande : un carré de
     * 256 blocs en fait exactement 16 sur 16. Demander plus ferait échouer la commande,
     * et la charge serait silencieusement absente.
     */
    static final int FORCELOAD_SPAN = 256;

    /** Entités invoquées par le profil {@link #MOBS}. */
    static final int MOB_COUNT = 200;

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
        if (!CHUNKS.equals(profile) && !MOBS.equals(profile)) {
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
        commands.add(String.format(Locale.ROOT, "forceload add %d %d %d %d",
                -half, -half, half - 1, half - 1));
        commands.add("gamerule randomTickSpeed " + RANDOM_TICK_SPEED);

        if (MOBS.equals(profile)) {
            // 4. Des entités qui pensent : IA, recherche de chemin, collisions. Elles
            //    sont réparties sur la zone chargée, et marquées persistantes pour ne
            //    pas disparaître faute de joueur à proximité.
            for (int i = 0; i < MOB_COUNT; i++) {
                int x = spread(i, 7) - half + 8;
                int z = spread(i, 11) - half + 8;
                commands.add(String.format(Locale.ROOT,
                        "execute positioned %d 0 %d run summon minecraft:cow ~ ~ ~ "
                                + "{PersistenceRequired:1b}", x, z));
            }
        }
        return List.copyOf(commands);
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
