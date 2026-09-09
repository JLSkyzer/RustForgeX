package dev.rustforgex.instrument;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;

/**
 * C-05 : recensement des méthodes chaudes qu'aucune sonde ne couvre.
 *
 * <p>Cahier des charges : PARTIE 5.5 (échantillonnage périodique), PARTIE 5.39.
 * Exigences : R-311 (son exception : découvrir avant de sonder), R-322, R-660.
 * Maturité : {@code STABLE}.
 *
 * <h2>Le problème qu'il résout</h2>
 *
 * <p>Trois mesures indépendantes ont établi qu'on <strong>ne peut pas acheter de la
 * couverture en armant plus de sondes</strong> : le budget en finance quelques milliers,
 * et c'est déjà ce qu'on arme (ADR-021, ADR-026, ADR-027). Élargir le sondage multiplie
 * le coût sans faire progresser la part du tick expliquée.
 *
 * <p>La couverture ne s'obtiendra donc qu'en <strong>choisissant</strong> les sondes. Or
 * pour choisir, il faut d'abord savoir ce qui est chaud — et le savoir sans y avoir posé
 * de sonde, sous peine de tourner en rond.
 *
 * <h2>Le moyen : ce que l'échantillonneur voyait déjà passer</h2>
 *
 * <p>{@link StackSampler} prélève cent piles par seconde sur le fil autoritatif. Les
 * trames qu'il ne savait pas attribuer, il les comptait globalement et les jetait. Elles
 * portent pourtant exactement l'information manquante : le nom d'une méthode qui
 * s'exécutait pendant un tick et que rien ne mesure.
 *
 * <p>Les consigner ne coûte <strong>aucune sonde</strong>, et rien de plus qu'un
 * comptage sur un fil de service qui prélevait déjà la pile.
 *
 * <h2>Ce qui est consigné, et ce qui ne l'est pas</h2>
 *
 * <p>Seules les trames <em>candidates</em> : une méthode que le transformateur aurait pu
 * sonder. Compter {@code java.util.HashMap.get} remplirait le classement de méthodes
 * qu'on n'a pas le droit d'instrumenter (R-312), et enterrerait les seules qui appellent
 * une décision. Un classement dont on ne peut rien faire ne vaut pas mieux que pas de
 * classement.
 *
 * <p>Le critère est celui de {@code ProbeEligibility}, réécrit ici en noms pointés parce
 * qu'une pile n'en porte pas d'autres — et parce que ce paquet ne doit pas dépendre du
 * plugin de lancement, qui peut être absent (ADR-017). Un test vérifie que les deux
 * listes disent la même chose.
 *
 * <h2>Bornes</h2>
 *
 * <p>La table est plafonnée. Un serveur moddé compte des centaines de milliers de
 * méthodes, et une table sans borne finirait par peser plus que ce qu'elle mesure. Une
 * fois pleine, elle continue de compter ce qu'elle connaît et <strong>dit</strong>
 * combien de méthodes distinctes elle a refusé d'apprendre : un classement tronqué qui
 * s'annonce reste utilisable, un classement tronqué qui se tait ment (R-660).
 */
public final class UnknownFrameIndex {

    /**
     * Nombre maximal de méthodes distinctes retenues.
     *
     * <p>À cent hertz, remplir huit mille entrées demanderait au moins quatre-vingts
     * secondes de trames toutes différentes — ce qui n'arrive pas : un serveur repasse
     * sans cesse par les mêmes méthodes, et c'est précisément ce qui rend le classement
     * significatif.
     */
    public static final int CAPACITY = 8_192;

    /** Préfixes des paquets qu'on n'a pas le droit d'instrumenter (R-312). */
    private static final String[] EXCLUDED_PACKAGES = {
        "java.", "javax.", "jdk.", "sun.", "com.sun.", "org.w3c.", "org.xml.",
        "dev.rustforgex."
    };

    /**
     * Une méthode vue s'exécuter sans être sondée.
     *
     * <p>Le comptage est un {@link AtomicLong} et non un champ simple : un seul fil
     * écrit — celui de l'échantillonnage — mais le fil autoritatif et celui d'une
     * commande lisent, et une lecture déchirée donnerait un classement faux.
     */
    public static final class Sighting {

        private final String className;
        private final String methodName;
        private final AtomicLong samples = new AtomicLong();

        private Sighting(String className, String methodName) {
            this.className = className;
            this.methodName = methodName;
        }

        /** @return le nom pointé de la classe, tel que la pile le porte */
        public String className() {
            return className;
        }

        /** @return le nom de la méthode ; le descripteur n'est pas connu d'une pile */
        public String methodName() {
            return methodName;
        }

        /** @return le nombre d'échantillons où cette méthode s'exécutait */
        public long samples() {
            return samples.get();
        }

        /** @return {@code classe#methode}, forme courte pour l'affichage */
        public String label() {
            return className + '#' + methodName;
        }
    }

    private final Map<String, Sighting> byFrame = new ConcurrentHashMap<>(1_024);

    private final AtomicLong recorded = new AtomicLong();
    private final AtomicLong distinctDropped = new AtomicLong();

    /**
     * Décide si une trame désigne une méthode que le transformateur pourrait sonder.
     *
     * <p>N'alloue pas : appelée depuis le fil d'échantillonnage, jusqu'à quelques
     * dizaines de fois par prélèvement.
     *
     * @param frame trame prélevée
     * @return {@code true} si la méthode est un candidat au sondage
     */
    public static boolean isCandidate(StackTraceElement frame) {
        if (frame.isNativeMethod()) {
            // Aucun corps à instrumenter (R-312, ProbeEligibility.NO_BODY).
            return false;
        }
        String method = frame.getMethodName();
        if ("<init>".equals(method) || "<clinit>".equals(method)) {
            // R-312 : ni constructeur d'instance, ni constructeur de classe.
            return false;
        }
        return isCandidateClass(frame.getClassName());
    }

    /**
     * @param dottedClassName nom pointé d'une classe
     * @return {@code true} si le transformateur a le droit de la toucher
     */
    public static boolean isCandidateClass(String dottedClassName) {
        for (String excluded : EXCLUDED_PACKAGES) {
            if (dottedClassName.startsWith(excluded)) {
                return false;
            }
        }
        return true;
    }

    /**
     * Consigne une trame vue s'exécuter sans sonde.
     *
     * <p>Appelée depuis le fil d'échantillonnage, une fois au plus par prélèvement.
     *
     * @param frame trame candidate ; les autres n'ont rien à faire ici
     */
    public void record(StackTraceElement frame) {
        String key = frame.getClassName() + '#' + frame.getMethodName();
        Sighting sighting = byFrame.get(key);
        if (sighting == null) {
            if (byFrame.size() >= CAPACITY) {
                // Table pleine : on continue de compter ce qu'on connaît, et on dit
                // combien de méthodes on a renoncé à apprendre.
                distinctDropped.incrementAndGet();
                return;
            }
            sighting = byFrame.computeIfAbsent(key,
                    ignored -> new Sighting(frame.getClassName(), frame.getMethodName()));
        }
        sighting.samples.incrementAndGet();
        recorded.incrementAndGet();
    }

    /**
     * Classement décroissant des méthodes chaudes non sondées.
     *
     * <p>Trie et alloue : à n'appeler que depuis une commande ou un relevé, jamais dans
     * un tick (INV-14).
     *
     * @param limit nombre d'entrées voulues, au plus
     * @return les méthodes les plus vues, la plus vue en tête
     */
    public List<Sighting> top(int limit) {
        if (limit <= 0) {
            return List.of();
        }
        List<Sighting> all = new ArrayList<>(byFrame.values());
        // À égalité d'échantillons, l'ordre alphabétique : un classement qui change
        // d'ordre d'un appel à l'autre sans que rien n'ait bougé fait douter du reste.
        all.sort(Comparator.comparingLong(Sighting::samples).reversed()
                .thenComparing(Sighting::label));
        return List.copyOf(all.subList(0, Math.min(limit, all.size())));
    }

    /** @return le nombre d'échantillons consignés, toutes méthodes confondues */
    public long recorded() {
        return recorded.get();
    }

    /** @return le nombre de méthodes distinctes connues */
    public int distinct() {
        return byFrame.size();
    }

    /** @return le nombre de méthodes distinctes non apprises, la table étant pleine */
    public long distinctDropped() {
        return distinctDropped.get();
    }

    /**
     * Vide le recensement.
     *
     * <p>Un classement cumulé depuis le démarrage mélange le chargement du monde et le
     * jeu. Pouvoir repartir de zéro est ce qui permet de mesurer une situation précise
     * — un ralentissement signalé maintenant, pas la moyenne de la session.
     */
    public void reset() {
        byFrame.clear();
        recorded.set(0L);
        distinctDropped.set(0L);
    }
}
