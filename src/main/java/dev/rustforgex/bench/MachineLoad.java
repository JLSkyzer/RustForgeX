package dev.rustforgex.bench;

import java.lang.management.ManagementFactory;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/**
 * Charge processeur de la machine hors du serveur, relevée pendant la fenêtre de mesure
 * d'un macro-benchmark.
 *
 * <p>Composant : C-36 (ADR-042, « Bancs réalistes »). Maturité : {@code STABLE}.
 *
 * <p>Comparer deux versions demande une machine qui ne fait rien d'autre : l'écart cherché
 * (0,1 à 1 ms par tick) est plus petit que le bruit d'un programme ouvert à côté. Plutôt
 * que d'exiger le repos, l'exécution consigne ce qui tournait d'autre et l'agrégateur
 * la déclare faussée au-delà d'un seuil. Une fois par seconde, sur son propre fil :
 * charge de toute la machine moins celle du processus du jeu, en part de la capacité
 * totale (0 à 1), d'après {@code com.sun.management.OperatingSystemMXBean}.
 */
final class MachineLoad {

    /** Nom du fil : le préfixe {@code RFX-} l'exclut du relevé de charge du runtime. */
    static final String THREAD_NAME = "RFX-BenchLoad";

    /** Intervalle entre deux relevés, en millisecondes. */
    static final long PERIOD_MS = 1_000L;

    /** Résumé des relevés : charge hors serveur, en part de la capacité de la machine. */
    record Summary(int samples, double p50, double p95, double max) {
    }

    private final com.sun.management.OperatingSystemMXBean os;
    private final List<Double> samples = new ArrayList<>();
    private final Thread thread;
    private volatile boolean running = true;

    private MachineLoad(com.sun.management.OperatingSystemMXBean os) {
        this.os = os;
        this.thread = new Thread(this::loop, THREAD_NAME);
        this.thread.setDaemon(true);
    }

    /**
     * Démarre le relevé.
     *
     * @return le relevé en cours ; sans la sonde de la JVM, un relevé qui ne relèvera
     *     rien, et que l'agrégateur tiendra pour non consigné
     */
    static MachineLoad start() {
        var bean = ManagementFactory.getOperatingSystemMXBean();
        MachineLoad load = new MachineLoad(
                bean instanceof com.sun.management.OperatingSystemMXBean os ? os : null);
        if (load.os != null) {
            load.thread.start();
        }
        return load;
    }

    private void loop() {
        while (running) {
            try {
                Thread.sleep(PERIOD_MS);
            } catch (InterruptedException e) {
                return;
            }
            double machine = os.getCpuLoad();
            double process = os.getProcessCpuLoad();
            // Une valeur négative veut dire « pas encore disponible » : rien n'est
            // inventé à sa place.
            if (machine >= 0 && process >= 0) {
                synchronized (samples) {
                    samples.add(Math.max(0.0, machine - process));
                }
            }
        }
    }

    /**
     * Arrête le relevé et le résume.
     *
     * @return le résumé ; zéro relevé si la sonde n'était pas disponible
     */
    Summary stop() {
        running = false;
        thread.interrupt();
        try {
            thread.join(2 * PERIOD_MS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
        double[] values;
        synchronized (samples) {
            values = samples.stream().mapToDouble(Double::doubleValue).toArray();
        }
        return summarize(values);
    }

    /**
     * Résume des relevés : médiane, 95e centile (rang le plus proche) et maximum.
     *
     * <p>Visible dans le paquet pour les tests.
     */
    static Summary summarize(double[] values) {
        if (values.length == 0) {
            return new Summary(0, 0.0, 0.0, 0.0);
        }
        double[] sorted = values.clone();
        Arrays.sort(sorted);
        return new Summary(sorted.length, rank(sorted, 0.50), rank(sorted, 0.95),
                sorted[sorted.length - 1]);
    }

    private static double rank(double[] sorted, double quantile) {
        int index = (int) Math.ceil(quantile * sorted.length) - 1;
        return sorted[Math.max(0, Math.min(sorted.length - 1, index))];
    }
}
