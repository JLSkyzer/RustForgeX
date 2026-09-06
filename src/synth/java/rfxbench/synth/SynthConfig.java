package rfxbench.synth;

import java.util.Locale;

/**
 * Réglages du mod synthétique de charge (PARTIE 22).
 *
 * <p>Tout est piloté par propriétés système, et tout vaut zéro par défaut : sans elles,
 * le mod se charge, ne s'abonne à rien et ne consomme rien. C'est la même règle que pour
 * l'enregistreur de benchmark — on n'installe une accroche que si elle fait quelque chose.
 *
 * <p>Classe sans dépendance à Minecraft ni à Forge : elle se teste seule.
 */
public final class SynthConfig {

    /** Préfixe des propriétés de réglage. */
    public static final String PREFIX = "rfxbench.synth.";

    /** Nombre d'unités de travail tickées à chaque tick serveur. */
    public static final String WORKLOADS = PREFIX + "workloads";

    /** Itérations de calcul par unité et par tick. */
    public static final String ITERATIONS = PREFIX + "iterations";

    /** Octets alloués par unité et par tick, arrondis au tableau le plus proche. */
    public static final String ALLOCATION = PREFIX + "allocation";

    /** Nombre de gestionnaires d'événements enregistrés, priorités mêlées. */
    public static final String HANDLERS = PREFIX + "handlers";

    /**
     * Unités engendrées à instancier, donc à charger, donc à instrumenter.
     *
     * <p>C'est le réglage qui produit de la <strong>largeur</strong> : chaque unité
     * chargée apporte une vingtaine de méthodes assez longues pour être sondées. Sans
     * lui, le mod synthétique fait du travail sans charger l'instrumentation, ce qui
     * était le défaut de sa première version.
     */
    public static final String UNITS = PREFIX + "units";

    /** Fils de travail propres au mod synthétique (R-870). */
    public static final String THREADS = PREFIX + "threads";

    /** {@code true} pour rendre une part du travail non déterministe (R-870). */
    public static final String NONDETERMINISTIC = PREFIX + "nondeterministic";

    private final int workloads;
    private final int units;
    private final int iterations;
    private final int allocationBytes;
    private final int handlers;
    private final int threads;
    private final boolean nondeterministic;

    private SynthConfig(int workloads, int units, int iterations, int allocationBytes,
            int handlers, int threads, boolean nondeterministic) {
        this.workloads = workloads;
        this.units = units;
        this.iterations = iterations;
        this.allocationBytes = allocationBytes;
        this.handlers = handlers;
        this.threads = threads;
        this.nondeterministic = nondeterministic;
    }

    /**
     * Lit les réglages depuis les propriétés système.
     *
     * @param properties accès aux propriétés, injectable pour les tests
     * @return les réglages, jamais {@code null}
     */
    public static SynthConfig from(java.util.function.UnaryOperator<String> properties) {
        return new SynthConfig(
                positive(properties, WORKLOADS),
                positive(properties, UNITS),
                positive(properties, ITERATIONS),
                positive(properties, ALLOCATION),
                positive(properties, HANDLERS),
                positive(properties, THREADS),
                Boolean.parseBoolean(orEmpty(properties, NONDETERMINISTIC)));
    }

    /** @return les réglages lus des propriétés système du processus */
    public static SynthConfig fromSystem() {
        return from(System::getProperty);
    }

    /**
     * Indique si le mod a quoi que ce soit à faire.
     *
     * <p>Un mod synthétique qui ne fait rien ne doit rien coûter : c'est ce qui permet
     * de le laisser dans {@code mods} entre deux campagnes sans fausser une mesure.
     *
     * @return {@code true} si au moins un réglage demande du travail
     */
    public boolean idle() {
        return workloads == 0 && units == 0 && handlers == 0 && threads == 0;
    }

    /** @return le nombre d'unités engendrées à charger */
    public int units() {
        return units;
    }

    /** @return le nombre d'unités de travail tickées */
    public int workloads() {
        return workloads;
    }

    /** @return les itérations de calcul par unité et par tick */
    public int iterations() {
        return iterations;
    }

    /** @return les octets alloués par unité et par tick */
    public int allocationBytes() {
        return allocationBytes;
    }

    /** @return le nombre de gestionnaires d'événements enregistrés */
    public int handlers() {
        return handlers;
    }

    /** @return le nombre de fils propres au mod synthétique */
    public int threads() {
        return threads;
    }

    /** @return {@code true} si une part du travail doit être non déterministe */
    public boolean nondeterministic() {
        return nondeterministic;
    }

    @Override
    public String toString() {
        return String.format(Locale.ROOT,
                "workloads=%d units=%d iterations=%d allocation=%do handlers=%d "
                        + "threads=%d nondeterministic=%b",
                workloads, units, iterations, allocationBytes, handlers, threads,
                nondeterministic);
    }

    /**
     * Lit un entier positif, ou zéro.
     *
     * <p>Une valeur illisible ou négative vaut zéro : un profil mal écrit doit produire
     * un mod inerte, jamais un comportement indéfini au milieu d'une campagne.
     */
    private static int positive(java.util.function.UnaryOperator<String> properties, String key) {
        try {
            int value = Integer.parseInt(orEmpty(properties, key).trim());
            return Math.max(value, 0);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    private static String orEmpty(
            java.util.function.UnaryOperator<String> properties, String key) {
        String value = properties.apply(key);
        return value == null ? "" : value;
    }
}
