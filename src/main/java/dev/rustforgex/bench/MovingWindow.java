package dev.rustforgex.bench;

import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.ChunkPos;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.level.ChunkEvent;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.Set;
import java.util.TreeSet;

/**
 * C-36 : fenêtre de chunks forcés qui se déplace vite, aller et retour — test G-09,
 * « chargement/déchargement massif de chunks (joueur en mouvement rapide) ».
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Sans joueur, ce qui compte est la mécanique des chunks : un carré de
 * {@code (2 × }{@value #RADIUS}{@code  + 1)²} chunks forcés avance d'un chunk tous les
 * {@value #STEP_TICKS} ticks — quatre chunks par seconde, l'allure d'un vol en élytres —
 * sur {@value #LENGTH} chunks vers l'est, puis revient. Un chunk qui sort de la fenêtre
 * perd son ticket : il est sauvegardé et déchargé. Au retour, il est relu depuis le
 * disque, parfois avant la fin de sa génération.
 *
 * <p>Les tickets sont posés et retirés dans l'ordre des positions, jamais dans celui
 * d'une table de hachage : l'ordre des demandes de génération en dépend.
 *
 * <p>Chargements et déchargements sont comptés par les événements de Forge, pendant la
 * course seulement : une campagne sans aucun déchargement n'aurait rien éprouvé.
 */
final class MovingWindow {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Demi-côté de la fenêtre, en chunks : 9 × 9 chunks. */
    static final int RADIUS = 4;

    /** Longueur de l'aller, en chunks. */
    static final int LENGTH = 80;

    /** Ticks entre deux pas de la fenêtre. */
    static final int STEP_TICKS = 5;

    private Set<Long> forced = new TreeSet<>();
    private int ticks;
    private int step = -1;
    private boolean done;
    private long loads;
    private long unloads;
    private int maxLoaded;

    /** Bornes de la bande parcourue, en chunks : {minX, maxX, minZ, maxZ}. */
    static int[] strip() {
        return new int[] {-RADIUS, LENGTH + RADIUS, -RADIUS, RADIUS};
    }

    /** Commence à compter les chargements et déchargements. */
    void start() {
        MinecraftForge.EVENT_BUS.register(this);
    }

    /**
     * Un tick de la course.
     *
     * @return {@code true} une fois le retour achevé et la fenêtre libérée
     */
    boolean tick(ServerLevel level) {
        if (done) {
            return true;
        }
        maxLoaded = Math.max(maxLoaded, level.getChunkSource().getLoadedChunksCount());
        if (ticks++ % STEP_TICKS != 0) {
            return false;
        }
        step++;
        if (step > 2 * LENGTH) {
            move(level, new TreeSet<>());
            MinecraftForge.EVENT_BUS.unregister(this);
            done = true;
            LOGGER.info("G-09 : course finie, {}.", activity());
            return true;
        }
        int centre = step <= LENGTH ? step : 2 * LENGTH - step;
        Set<Long> wanted = new TreeSet<>();
        for (int x = centre - RADIUS; x <= centre + RADIUS; x++) {
            for (int z = -RADIUS; z <= RADIUS; z++) {
                wanted.add(ChunkPos.asLong(x, z));
            }
        }
        move(level, wanted);
        if (step % 20 == 0) {
            LOGGER.info("G-09 pas {} : centre x = {}, {}.", step, centre, activity());
        }
        return false;
    }

    /** Retire les tickets qui sortent de la fenêtre, puis pose ceux qui y entrent. */
    private void move(ServerLevel level, Set<Long> wanted) {
        for (long position : forced) {
            if (!wanted.contains(position)) {
                level.setChunkForced(ChunkPos.getX(position), ChunkPos.getZ(position), false);
            }
        }
        for (long position : wanted) {
            if (!forced.contains(position)) {
                level.setChunkForced(ChunkPos.getX(position), ChunkPos.getZ(position), true);
            }
        }
        forced = wanted;
    }

    @SubscribeEvent
    public void onLoad(ChunkEvent.Load event) {
        if (event.getLevel() instanceof ServerLevel) {
            loads++;
        }
    }

    @SubscribeEvent
    public void onUnload(ChunkEvent.Unload event) {
        if (event.getLevel() instanceof ServerLevel) {
            unloads++;
        }
    }

    /** {@code true} si la course n'a déchargé aucun chunk : rien n'a été éprouvé. */
    boolean inert() {
        return unloads == 0;
    }

    /** Compteurs de la course. */
    String activity() {
        return "chargements " + loads + ", déchargements " + unloads
                + ", chunks chargés au plus " + maxLoaded;
    }
}
