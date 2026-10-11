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
 * <p>Métriques exposées, par accroche : {@code rfx.hook.calls},
 * {@code rfx.hook.duration_ns} et {@code rfx.hook.errors} (contrat agent 4.1, PARTIE 4.19).
 * C'est {@code RfxRuntime.metrics()} qui les relève.
 */
public final class HookGuard {

    /** Échecs tolérés avant désactivation définitive du hook (FM-02). */
    public static final int DISABLE_THRESHOLD = 5;

    private final String name;
    private final Consumer<String> log;
    private final Consumer<Incident> incidents;
    private final AtomicInteger failures = new AtomicInteger();
    private final AtomicLong timedCalls = new AtomicLong();
    private final AtomicLong untimedCalls = new AtomicLong();
    private final AtomicLong totalDurationNs = new AtomicLong();
    private volatile boolean disabled;

    /**
     * Chronométrer chaque exécution ({@code telemetry.enabled}).
     *
     * <p>Le chronométrage est de la télémétrie pure : deux lectures d'horloge et une
     * addition par exécution, la part la plus chère de R-561 (ADR-034). Le compte des
     * échecs, lui, reste tenu dans tous les cas — c'est lui qui protège le jeu (FM-02).
     */
    private volatile boolean timed = true;

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
        // L'horloge n'est lue que si le chronométrage est actif au départ ; la mesure
        // n'est retenue que s'il l'est encore à l'arrivée. La garde du démarrage coupe
        // elle-même le chronométrage en cours d'exécution, quand la configuration lui
        // parvient : sa durée ne doit pas être publiée pour autant (constaté en jeu).
        boolean timing = timed;
        long start = timing ? System.nanoTime() : 0L;
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
            if (timing && timed) {
                totalDurationNs.addAndGet(System.nanoTime() - start);
                timedCalls.incrementAndGet();
            } else {
                untimedCalls.incrementAndGet();
            }
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

    /** @return le nombre d'exécutions, réussies ou non ({@code rfx.hook.calls}) */
    public long calls() {
        return timedCalls.get() + untimedCalls.get();
    }

    /**
     * Active ou coupe le chronométrage ({@code telemetry.enabled}).
     *
     * <p>Couper efface aussi ce qui avait été chronométré : la garde du démarrage
     * s'exécute avant que la configuration lui parvienne, et sans cela une durée serait
     * publiée alors que la télémétrie est coupée — constaté en jeu le 2026-10-11. Les
     * exécutions effacées restent comptées.
     *
     * @param enabled {@code true} pour chronométrer chaque exécution
     */
    public void timed(boolean enabled) {
        this.timed = enabled;
        if (!enabled) {
            untimedCalls.addAndGet(timedCalls.getAndSet(0));
            totalDurationNs.set(0);
        }
    }

    /** @return le nombre d'exécutions chronométrées */
    public long timedCalls() {
        return timedCalls.get();
    }

    /**
     * @return la durée moyenne d'une exécution chronométrée en nanosecondes
     *     ({@code rfx.hook.duration_ns}), ou {@code 0} si aucune ne l'a été — voir
     *     {@link #timedCalls()} pour distinguer ce zéro d'une mesure
     */
    public long averageDurationNs() {
        long n = timedCalls.get();
        return n == 0 ? 0 : totalDurationNs.get() / n;
    }

    /** @return le nom du hook */
    public String name() {
        return name;
    }
}
