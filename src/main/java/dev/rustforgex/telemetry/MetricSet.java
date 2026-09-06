package dev.rustforgex.telemetry;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Recueil de métriques à cardinalité bornée (C-34).
 *
 * <p>Cahier des charges : PARTIE 5.32. Tests : T-402. Maturité : {@code STABLE}.
 *
 * <h2>Pourquoi une borne dure</h2>
 *
 * <p>La PARTIE 5.32 exige une cardinalité bornée, et donne la raison en creux : « jamais
 * un {@code WorkId} brut en label de métrique agrégée ». Un recueil dont le nombre de
 * séries dépend d'une donnée d'exécution grossit avec la partie, et finit par coûter
 * plus que ce qu'il mesure.
 *
 * <p>La borne est donc <strong>vérifiée à l'exécution</strong>, pas seulement promise en
 * commentaire. Un dépassement lève : c'est un défaut de programmation — un nom de série
 * construit à partir d'une donnée — et il doit se voir au test, pas fuir en production.
 *
 * <p>Un nom en double est refusé pour la même raison : deux valeurs sous un même nom
 * rendent le relevé indéterminé selon l'ordre d'insertion.
 */
public final class MetricSet {

    /**
     * Nombre maximal de séries.
     *
     * <p>Deux cent cinquante-six, soit largement plus que ce que le jalon produit, et
     * assez peu pour qu'un nom construit dynamiquement fasse échouer un test avant
     * d'atteindre la production.
     */
    public static final int MAX_SERIES = 256;

    private final Map<String, Metric> metrics = new LinkedHashMap<>();

    /**
     * Ajoute une métrique.
     *
     * @param metric la mesure, jamais {@code null}
     * @return ce recueil, pour enchaîner
     * @throws IllegalStateException si la borne est atteinte ou le nom déjà présent
     */
    public MetricSet add(Metric metric) {
        if (metrics.containsKey(metric.name())) {
            throw new IllegalStateException(
                    "métrique en double : " + metric.name() + " — deux valeurs sous un "
                            + "même nom rendent le relevé indéterminé");
        }
        if (metrics.size() >= MAX_SERIES) {
            throw new IllegalStateException(
                    "plus de " + MAX_SERIES + " séries : un nom de métrique est "
                            + "probablement construit à partir d'une donnée d'exécution "
                            + "(PARTIE 5.32, cardinalité bornée)");
        }
        metrics.put(metric.name(), metric);
        return this;
    }

    /**
     * Ajoute une métrique seulement si sa source l'a mesurée.
     *
     * <p>R-660 et le contrat agent 3.2 : une valeur non mesurée ne s'écrit pas. Un zéro
     * à la place d'une mesure absente est indiscernable d'un zéro mesuré, et c'est
     * exactement ce qui rend un tableau de bord trompeur.
     *
     * @param present {@code true} si la source a effectivement mesuré
     * @param metric la mesure, évaluée seulement si {@code present}
     * @return ce recueil, pour enchaîner
     */
    public MetricSet addIfMeasured(boolean present, java.util.function.Supplier<Metric> metric) {
        return present ? add(metric.get()) : this;
    }

    /**
     * @param name nom complet
     * @return la métrique, ou {@code null} si elle n'a pas été relevée
     */
    public Metric get(String name) {
        return metrics.get(name);
    }

    /** @return les métriques, dans leur ordre d'insertion */
    public List<Metric> all() {
        return List.copyOf(new ArrayList<>(metrics.values()));
    }

    /** @return le nombre de séries relevées */
    public int size() {
        return metrics.size();
    }
}
