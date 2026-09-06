package dev.rustforgex.telemetry;

/**
 * Une mesure nommée, typée et documentée (C-34).
 *
 * <p>Cahier des charges : PARTIE 5.32. Exigence : R-562. Maturité : {@code STABLE}.
 *
 * <p>Le nom suit {@code rfx.<domaine>.<mesure>}. La contrainte de cardinalité bornée de
 * la PARTIE 5.32 est tenue par construction : une métrique n'a pas d'étiquette libre,
 * et l'ensemble des noms est fixé par le code, jamais par une donnée d'exécution. En
 * particulier, aucun {@code WorkId} ne peut devenir un nom de série.
 *
 * <p>{@link #description} n'est pas décorative : R-562 exige que toute métrique soit
 * documentée avec son nom, son unité et sa sémantique. La porter ici plutôt que dans un
 * fichier séparé est la seule façon qu'elle ne diverge pas.
 *
 * @param name nom complet, {@code rfx.<domaine>.<mesure>}
 * @param type nature de la mesure
 * @param unit unité, ou {@link #NO_UNIT} pour un décompte sans dimension
 * @param value valeur relevée
 * @param description sémantique en une phrase (R-562)
 */
public record Metric(String name, Metric.Type type, String unit, double value,
        String description) {

    /** Unité des décomptes sans dimension. */
    public static final String NO_UNIT = "";

    /** Préfixe imposé à tout nom de métrique. */
    public static final String PREFIX = "rfx.";

    /** Nature d'une mesure (PARTIE 5.32). */
    public enum Type {
        /** Décompte qui ne fait que croître. */
        COUNTER,
        /** Valeur instantanée, qui monte et descend. */
        GAUGE,
        /** Rapport entre deux mesures, en pourcent. */
        RATIO
    }

    /** Valide le nom et la valeur. */
    public Metric {
        if (name == null || !name.startsWith(PREFIX)) {
            throw new IllegalArgumentException(
                    "un nom de métrique commence par « " + PREFIX + " » : " + name);
        }
        if (name.chars().filter(c -> c == '.').count() < 2) {
            throw new IllegalArgumentException(
                    "un nom de métrique s'écrit rfx.<domaine>.<mesure> : " + name);
        }
        if (description == null || description.isBlank()) {
            // R-562 : une métrique sans sémantique écrite n'est pas exploitable, et
            // personne ne reviendra l'écrire plus tard.
            throw new IllegalArgumentException("métrique sans description : " + name);
        }
        if (!Double.isFinite(value)) {
            throw new IllegalArgumentException(
                    "valeur non finie pour " + name + " : une mesure impossible ne "
                            + "s'écrit pas, elle ne se relève pas");
        }
        unit = unit == null ? NO_UNIT : unit;
    }

    /**
     * Crée un décompte.
     *
     * @param name nom complet
     * @param value valeur relevée
     * @param description sémantique
     * @return la métrique
     */
    public static Metric counter(String name, long value, String description) {
        return new Metric(name, Type.COUNTER, NO_UNIT, value, description);
    }

    /**
     * Crée une valeur instantanée.
     *
     * @param name nom complet
     * @param value valeur relevée
     * @param unit unité
     * @param description sémantique
     * @return la métrique
     */
    public static Metric gauge(String name, double value, String unit, String description) {
        return new Metric(name, Type.GAUGE, unit, value, description);
    }

    /**
     * Crée un rapport, exprimé en pourcent.
     *
     * @param name nom complet
     * @param percent valeur en pourcent
     * @param description sémantique
     * @return la métrique
     */
    public static Metric ratio(String name, double percent, String description) {
        return new Metric(name, Type.RATIO, "%", percent, description);
    }
}
