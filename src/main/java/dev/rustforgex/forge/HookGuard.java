package dev.rustforgex.forge;

import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;

/**
 * C-01, FM-02 : encapsulation d'un hook Forge.
 *
 * <p>Cahier des charges : PARTIE 5.1. Test : T-103. Maturité : {@code STABLE}.
 *
 * <p>Chaque hook de RUSTFORGE-X passe par cette garde. Une exception levée dans le
 * code du mod ne doit jamais interrompre le tick ni la séquence de chargement de
 * Forge : elle est capturée, comptée, et le hook est désactivé après
 * {@link #DISABLE_THRESHOLD} échecs. Un hook désactivé le reste jusqu'au redémarrage :
 * un code qui échoue systématiquement coûterait plus cher qu'il ne rapporte, et
 * masquerait le défaut d'origine.
 *
 * <p>Métriques exposées : {@code rfx.hook.duration_ns}, {@code rfx.hook.errors}
 * (contrat agent 4.1).
 */
public final class HookGuard {

    /** Échecs tolérés avant désactivation définitive du hook (FM-02). */
    public static final int DISABLE_THRESHOLD = 5;

    private final String name;
    private final Consumer<String> log;
    private final AtomicInteger failures = new AtomicInteger();
    private final AtomicLong calls = new AtomicLong();
    private final AtomicLong totalDurationNs = new AtomicLong();
    private volatile boolean disabled;

    /**
     * @param name nom du hook, repris dans les journaux et les métriques
     * @param log destination des messages d'anomalie
     */
    public HookGuard(String name, Consumer<String> log) {
        this.name = name;
        this.log = log;
    }

    /**
     * Exécute l'action du hook sous protection.
     *
     * @param action travail à effectuer
     * @return {@code true} si l'action s'est exécutée sans erreur
     */
    public boolean run(Runnable action) {
        if (disabled) {
            return false;
        }
        long start = System.nanoTime();
        try {
            action.run();
            return true;
        } catch (RuntimeException | LinkageError | AssertionError e) {
            int total = failures.incrementAndGet();
            log.accept("Hook « " + name + " » en échec (" + total + "/" + DISABLE_THRESHOLD
                    + ") : " + e);
            if (total >= DISABLE_THRESHOLD) {
                disabled = true;
                log.accept("Hook « " + name + " » désactivé après " + total
                        + " échecs. RUSTFORGE-X continue sans lui ; le jeu n'est pas affecté.");
            }
            return false;
        } finally {
            totalDurationNs.addAndGet(System.nanoTime() - start);
            calls.incrementAndGet();
        }
    }

    /** @return {@code true} si le hook a été désactivé après trop d'échecs */
    public boolean disabled() {
        return disabled;
    }

    /** @return le nombre d'échecs observés ({@code rfx.hook.errors}) */
    public int failures() {
        return failures.get();
    }

    /** @return le nombre d'exécutions, réussies ou non */
    public long calls() {
        return calls.get();
    }

    /**
     * @return la durée moyenne d'une exécution en nanosecondes
     *     ({@code rfx.hook.duration_ns}), ou {@code 0} si le hook n'a jamais servi
     */
    public long averageDurationNs() {
        long n = calls.get();
        return n == 0 ? 0 : totalDurationNs.get() / n;
    }

    /** @return le nom du hook */
    public String name() {
        return name;
    }
}
