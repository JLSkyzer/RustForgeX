package rfxtest.workload;

import java.util.ArrayList;
import java.util.List;

import dev.rustforgex.launch.ProbeEligibility;

/**
 * Corpus de méthodes de référence pour T-130.
 *
 * <p>Volontairement hors du paquet {@code dev.rustforgex} : C-04 refuse de sonder ses
 * propres classes, et un corpus placé sous ce paquet ne serait jamais instrumenté —
 * le test ne prouverait rien.
 *
 * <p>Chaque méthode exerce une forme de flot de contrôle que l'injection doit
 * traverser sans rien changer : retour de valeur, retours multiples, boucle,
 * exception propagée, bloc de rattrapage existant, {@code finally} existant, méthode
 * sans valeur de retour.
 *
 * <p>Les corps sont volontairement assez longs pour dépasser le seuil de
 * {@link ProbeEligibility#MIN_INSTRUCTIONS} : une méthode triviale serait refusée au
 * sondage, et ne prouverait donc rien de la transformation.
 */
public class SampleWorkload {

    /** Somme des entiers de zéro à {@code n}, avec accumulation dans une liste. */
    public int sum(int n) {
        List<Integer> seen = new ArrayList<>();
        int total = 0;
        for (int i = 0; i < n; i++) {
            total += i;
            seen.add(i);
            if (total < 0) {
                total = 0;
            }
        }
        return total + seen.size() - seen.size();
    }

    /** Deux points de retour distincts. */
    public String classify(int value) {
        List<String> labels = new ArrayList<>();
        labels.add("negatif");
        labels.add("positif");
        int index = value < 0 ? 0 : 1;
        String label = labels.get(index);
        if (label.isEmpty()) {
            return "vide";
        }
        return label + ":" + value;
    }

    /** Lève systématiquement, pour vérifier la sortie par exception. */
    public int alwaysThrows(int n) {
        List<Integer> values = new ArrayList<>();
        for (int i = 0; i < 3; i++) {
            values.add(i + n);
        }
        if (!values.isEmpty()) {
            throw new IllegalStateException("échec attendu : " + values.size());
        }
        return values.size();
    }

    /** Bloc de rattrapage existant : il doit rester prioritaire. */
    public String catchesItsOwn(int n) {
        List<Integer> values = new ArrayList<>();
        try {
            values.add(n);
            if (n > 0) {
                throw new IllegalArgumentException("interne");
            }
            return "sans-exception:" + values.size();
        } catch (IllegalArgumentException e) {
            return "rattrape:" + e.getMessage() + ":" + values.size();
        }
    }

    /** {@code finally} existant : son effet doit être conservé. */
    public String withFinally(int n, List<String> trace) {
        List<Integer> values = new ArrayList<>();
        try {
            values.add(n);
            trace.add("corps");
            if (n < 0) {
                throw new IllegalStateException("negatif");
            }
            return "ok:" + values.size();
        } finally {
            trace.add("finally");
        }
    }

    /** Méthode sans valeur de retour. */
    public void appendAll(List<String> target, int count) {
        List<Integer> values = new ArrayList<>();
        for (int i = 0; i < count; i++) {
            values.add(i);
            target.add("v" + i);
        }
        if (values.size() != count) {
            target.add("incoherent");
        }
    }

    /** Méthode trop courte : elle ne doit jamais être sondée (R-311). */
    public int tooShort() {
        return 42;
    }

    /** Méthode statique, pour vérifier que le sondage n'exige pas d'instance. */
    public static long staticWork(long seed) {
        List<Long> values = new ArrayList<>();
        long acc = seed;
        for (int i = 0; i < 5; i++) {
            acc = acc * 31 + i;
            values.add(acc);
        }
        return acc + values.size();
    }
}
