package dev.rustforgex.instrument;

/**
 * Cible des appels injectés dans le bytecode par C-04.
 *
 * <p>Cahier des charges : PARTIE 5.4. Exigences : R-310 (la sémantique observable ne
 * change pas), R-320 (aucune allocation dans le chemin chaud). Maturité :
 * {@code STABLE}.
 *
 * <p>Chaque méthode sondée appelle {@link #enter(int)} en entrée et {@link #exit(int,
 * long)} dans un {@code finally}. Ces deux méthodes sont donc sur le chemin le plus
 * chaud du jeu : elles n'allouent rien, ne verrouillent rien, et — surtout — <strong>ne
 * lèvent jamais d'exception</strong>. Une exception échappée d'ici remonterait dans du
 * code de mod arbitraire et casserait le jeu.
 *
 * <p>Tant qu'aucun puits n'est installé, les deux méthodes ne font rien. C'est l'état
 * normal pendant tout le chargement : le transformateur est actif bien avant que le
 * runtime natif ne soit prêt.
 *
 * <p>Le niveau de sonde est lu à l'exécution, dans un tableau indexé par identifiant
 * de sonde (ADR-016). Changer de niveau ne demande donc aucune retransformation, et le
 * bytecode injecté reste le même quel que soit le niveau.
 */
public final class RfxProbes {

    /**
     * Puits d'enregistrements, ou {@code null} tant que le runtime n'est pas prêt.
     *
     * <p>{@code volatile} : le transformateur pose des sondes depuis le fil de
     * chargement, elles s'exécutent depuis les fils du jeu, et l'installation a lieu
     * depuis un troisième. La visibilité doit être garantie sans verrou.
     */
    private static volatile ProbeSink sink;

    /**
     * Niveau de chaque sonde, indexé par identifiant.
     *
     * <p>Un tableau plutôt qu'une table de correspondance : la lecture a lieu à chaque
     * appel sondé, et un accès indexé se compile en une seule instruction.
     */
    private static volatile byte[] levels = new byte[0];

    /**
     * `true` si au moins une sonde est armée dans la table courante.
     *
     * <p>Recalculé à chaque changement de table plutôt qu'à chaque lecture :
     * l'échantillonneur pose la question cent fois par seconde, la table change
     * quelques fois par minute.
     */
    private static volatile boolean anyArmed;

    /** Enregistrements écartés faute de sonde connue, comptés sans allouer. */
    private static final java.util.concurrent.atomic.AtomicLong UNKNOWN_PROBES =
            new java.util.concurrent.atomic.AtomicLong();

    private RfxProbes() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Installe le puits et la table des niveaux.
     *
     * @param newSink puits d'enregistrements
     * @param newLevels niveau de chaque sonde, indexé par identifiant
     */
    public static void install(ProbeSink newSink, byte[] newLevels) {
        byte[] table = newLevels.clone();
        refreshAnyArmed(table);
        levels = table;
        // Le puits en dernier : tant qu'il est nul, les sondes ne font rien, et la
        // table est déjà en place quand elles commencent à s'exécuter.
        sink = newSink;
    }

    /**
     * Indique si au moins une sonde est armée.
     *
     * <p>Une table entièrement éteinte signifie soit un profiler à l'arrêt, soit une
     * pause de mesure de la PARTIE 12.4. Dans les deux cas, l'échantillonnage doit
     * cesser : prélever pendant la pause attribuerait au jeu un coût qui est le nôtre.
     *
     * @return {@code true} si au moins un niveau est non nul
     */
    public static boolean anyProbeArmed() {
        return anyArmed;
    }

    /** Recalcule le drapeau après un changement de table. */
    private static void refreshAnyArmed(byte[] table) {
        for (byte level : table) {
            if (level != 0) {
                anyArmed = true;
                return;
            }
        }
        anyArmed = false;
    }

    /**
     * Met à jour le niveau des sondes sans retransformer quoi que ce soit.
     *
     * @param newLevels niveau de chaque sonde, indexé par identifiant
     */
    public static void setLevels(byte[] newLevels) {
        byte[] table = newLevels.clone();
        refreshAnyArmed(table);
        levels = table;
    }

    /** Désinstalle le puits : les sondes redeviennent inertes. */
    public static void uninstall() {
        sink = null;
        anyArmed = false;
        levels = new byte[0];
    }

    /**
     * Entrée dans une méthode sondée.
     *
     * @param probeId identifiant de la sonde
     * @return l'horodatage d'entrée à passer à {@link #exit(int, long)}, ou {@code 0}
     *     si cette sonde ne mesure pas de durée
     */
    public static long enter(int probeId) {
        byte[] table = levels;
        if (sink == null || probeId < 0 || probeId >= table.length) {
            return 0L;
        }
        byte level = table[probeId];
        if (level == 0) {
            // OFF : la méthode est transformée mais la sonde est éteinte.
            return 0L;
        }
        if (level == 1) {
            // COUNTER : pas d'horloge, on n'enregistre que le passage.
            record(probeId, ProbeSink.KIND_ENTER, 0L);
            return 0L;
        }
        // TIMED et DEEP : l'horodatage sert de témoin d'entrée et de base de durée.
        return System.nanoTime();
    }

    /**
     * Sortie d'une méthode sondée, y compris par exception.
     *
     * @param probeId identifiant de la sonde
     * @param entryNanos valeur rendue par {@link #enter(int)}
     */
    public static void exit(int probeId, long entryNanos) {
        if (entryNanos == 0L || sink == null) {
            // Sonde éteinte, en mode compteur, ou puits absent : rien à faire.
            return;
        }
        long elapsed = System.nanoTime() - entryNanos;
        // Une horloge non monotone rendrait une durée négative : on la jette plutôt
        // que de polluer l'histogramme (FM-12).
        record(probeId, ProbeSink.KIND_EXIT, elapsed < 0 ? 0L : elapsed);
    }

    /** Écrit un enregistrement, en absorbant toute défaillance. */
    private static void record(int probeId, byte kind, long value) {
        ProbeSink current = sink;
        if (current == null) {
            return;
        }
        try {
            current.record(probeId, kind, (short) 0, System.nanoTime(), value);
        } catch (RuntimeException | LinkageError e) {
            // Le chemin le plus chaud du jeu ne remonte jamais d'exception. Un défaut
            // du profilage doit se voir dans les compteurs, pas dans une partie qui
            // s'arrête.
            UNKNOWN_PROBES.incrementAndGet();
        }
    }

    /** @return le nombre d'enregistrements qui n'ont pas pu être écrits */
    public static long failedRecords() {
        return UNKNOWN_PROBES.get();
    }

    /** @return {@code true} si un puits est installé */
    public static boolean active() {
        return sink != null;
    }

    /** @return le nombre de sondes déclarées */
    public static int probeCount() {
        return levels.length;
    }
}
