package dev.rustforgex.instrument;

/**
 * C-05 : cadence de l'échantillonnage de piles, réglée sur ce qu'il coûte (ADR-037).
 *
 * <p>Cahier des charges : PARTIE 5.5, H-07. Test : T-140. Maturité : {@code STABLE}.
 *
 * <p>Lire la pile d'un autre fil suspend ce fil le temps de la lecture. À cent hertz
 * fixes, ce prix est négligeable sur un serveur ordinaire et ne l'est plus sous forte
 * charge : sous {@code heavy} synthétique, l'échantillonnage était le coût dominant du
 * profilage, au-delà de son budget (ADR-036). Le gouverneur ne l'arrêtait qu'en coupant
 * la dernière sonde, c'est-à-dire en cessant d'observer.
 *
 * <p>D'où cette règle : la durée de chaque prélèvement est mesurée, lissée, et la
 * période est choisie pour que la part du temps passée à suspendre le fil observé reste
 * sous {@code budgetFraction}. Bornée entre {@link #MIN_PERIOD_NS} (la cadence d'origine)
 * et {@link #MAX_PERIOD_NS} : l'échantillonnage ralentit, il ne s'arrête jamais de lui-
 * même.
 *
 * <p>La durée de l'appel {@code getStackTrace} est prise comme estimation de la
 * suspension : c'est une estimation, que la ligne de base de la PARTIE 12.4 contrôle de
 * l'extérieur.
 */
public final class SamplingPacer {

    /** Période minimale : cent prélèvements par seconde, la cadence de la PARTIE 5.5. */
    public static final long MIN_PERIOD_NS = 10_000_000L;

    /** Période maximale : un prélèvement par seconde. */
    public static final long MAX_PERIOD_NS = 1_000_000_000L;

    /**
     * Part par défaut du temps que l'échantillonnage peut prendre au fil observé : le
     * quart du budget de profilage par défaut (2 % d'un cœur, H-07).
     */
    public static final double DEFAULT_BUDGET_FRACTION = 0.005;

    /** Poids d'une nouvelle mesure dans la moyenne lissée du coût d'un prélèvement. */
    static final double ALPHA = 0.1;

    private final double budgetFraction;
    private double costNs = -1.0;
    private volatile long periodNs = MIN_PERIOD_NS;

    /**
     * @param budgetFraction part du temps, entre 0 exclu et 1, que l'échantillonnage peut
     *     prendre au fil observé
     */
    public SamplingPacer(double budgetFraction) {
        if (!(budgetFraction > 0.0 && budgetFraction <= 1.0)) {
            throw new IllegalArgumentException("part de budget hors de ]0, 1] : " + budgetFraction);
        }
        this.budgetFraction = budgetFraction;
    }

    /**
     * Intègre la durée d'un prélèvement et recalcule la période.
     *
     * <p>Appelée par le seul fil d'échantillonnage.
     *
     * @param sampleNs durée de la lecture de pile, en nanosecondes
     */
    public void record(long sampleNs) {
        double sample = Math.max(0L, sampleNs);
        costNs = costNs < 0 ? sample : costNs + ALPHA * (sample - costNs);
        long wanted = (long) (costNs / budgetFraction);
        periodNs = Math.max(MIN_PERIOD_NS, Math.min(MAX_PERIOD_NS, wanted));
    }

    /**
     * @return la période courante, en nanosecondes ; c'est aussi la durée que chaque
     *     prélèvement représente quand il est attribué à une sonde
     */
    public long periodNs() {
        return periodNs;
    }
}
