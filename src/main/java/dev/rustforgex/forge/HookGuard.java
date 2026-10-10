package dev.rustforgex.forge;

import dev.rustforgex.diag.Incident;

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
 * <p>La désactivation est un incident : elle est remise, avec la dernière exception,
 * à qui consigne les incidents (C-35, T-411). Les échecs qui la précèdent ne le sont
 * pas — ils sont journalisés, et un échec isolé n'appelle pas de dump.
 *
 * <p>Métriques exposées : {@code rfx.hook.duration_ns}, {@code rfx.hook.errors}
 * (contrat agent 4.1).
 */
public final class HookGuard {

    /** Échecs tolérés avant désactivation définitive du hook (FM-02). */
    public static final int DISABLE_THRESHOLD = 5;

    private final String name;
    private final Consumer<String> log;
    private final Consumer<Incident> incidents;
    private final AtomicInteger failures = new AtomicInteger();
    private final AtomicLong calls = new AtomicLong();
    private final AtomicLong totalDurationNs = new AtomicLong();
    private volatile boolean disabled;

    /**
     * @param name nom du hook, repris dans les journaux et les métriques
     * @param log destination des messages d'anomalie
     */
    public HookGuard(String name, Consumer<String> log) {
        this(name, log, incident -> { });
    }

    /**
     * @param name nom du hook, repris dans les journaux et les métriques
     * @param log destination des messages d'anomalie
     * @param incidents destinataire de l'incident de désactivation (C-35)
     */
    public HookGuard(String name, Consumer<String> log, Consumer<Incident> incidents) {
        this.name = name;
        this.log = log;
        this.incidents = incidents;
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
                report(Incident.hookDisabled(name, total, e));
            }
            return false;
        } finally {
            totalDurationNs.addAndGet(System.nanoTime() - start);
            calls.incrementAndGet();
        }
    }

    /**
     * Remet l'incident sans jamais lever : la garde est la dernière ligne de défense du
     * jeu, et sa propre consignation ne doit pas devenir un nouvel échec.
     */
    private void report(Incident incident) {
        try {
            incidents.accept(incident);
        } catch (RuntimeException | LinkageError e) {
            log.accept("Hook « " + name + " » : incident non consigné (" + e
                    + "). Le jeu n'est pas affecté.");
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
