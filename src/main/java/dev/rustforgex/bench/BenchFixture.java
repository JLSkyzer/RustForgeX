package dev.rustforgex.bench;

import net.minecraft.server.level.ServerLevel;

/**
 * C-36 : ouvrage de test posé dans le monde puis regardé tourner — circuit de G-06,
 * ligne de conteneurs de G-12.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Un ouvrage occupe un carré de chunks et une tranche de hauteurs, au-dessus du bruit
 * de la génération (ADR-032). {@link FixtureRun} le pose, le lance et en prend
 * l'empreinte ; l'ouvrage, lui, dit ce qu'il pose et comment prouver qu'il a tourné.
 */
public interface BenchFixture {

    /** Premier chunk du carré, en X comme en Z. */
    int minChunk();

    /** Dernier chunk du carré, borne incluse. */
    int maxChunk();

    /** Bas de la tranche vidée et hachée. */
    int minY();

    /** Haut de la tranche vidée et hachée, borne incluse. */
    int maxY();

    /**
     * Pose l'ouvrage, dans un seul tick et toujours dans le même ordre.
     *
     * @return le nombre de poses qui n'ont pas pris ; zéro attendu
     */
    int build(ServerLevel level);

    /** Nombre de ticks de lancement après la pose ; zéro si l'ouvrage démarre seul. */
    int launchSteps();

    /**
     * Une étape de lancement, au tick {@code step} après la pose (de 1 à
     * {@link #launchSteps()}).
     *
     * @return le nombre de poses qui n'ont pas pris
     */
    int launch(ServerLevel level, int step);

    /** Relève l'état des parties de l'ouvrage, une fois par tick après le lancement. */
    void observe(ServerLevel level);

    /** Changements d'état comptés par partie, pour le journal et le fichier. */
    String activity();

    /**
     * {@code true} si chaque partie a changé au moins une fois. Une égalité entre trois
     * ouvrages restés immobiles ne prouverait rien.
     */
    boolean everyPartMoved();
}
