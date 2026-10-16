package dev.rustforgex.sched;

import dev.rustforgex.bridge.NativeBridge;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.function.IntConsumer;

/**
 * C-17, côté Java : transmet au runtime natif, une fois par seconde, la charge externe
 * relevée par {@link ExternalLoadSensor}.
 *
 * <p>Cahier des charges : R-421, R-700 (un appel par seconde, hors du budget de
 * traversées par tick). Décision : ADR-046. Maturité : {@code STABLE}.
 *
 * <p>Il tourne sur son propre fil, de priorité minimale, et jamais dans le tick : le
 * relevé lit le temps CPU de chaque fil, ce qui n'a rien à faire dans le chemin du jeu.
 *
 * <p>Ne lève jamais : un code d'erreur du natif est compté et transmis au destinataire
 * d'erreurs ; une exception est comptée, et cinq échecs de suite arrêtent le relevé
 * sans toucher au jeu.
 */
public final class ExternalLoadReporter {

    /** Période des relevés, en millisecondes. */
    public static final long PERIOD_MS = 1_000L;

    /** Nom du fil de relevé : commence par {@code RFX-}, il s'exclut lui-même du compte. */
    public static final String THREAD_NAME = "RFX-LoadSensor";

    private static final int MAX_FAILURES = 5;

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex");

    private final ExternalLoadSensor sensor;
    private final NativeBridge bridge;
    private final long handle;
    private final IntConsumer nativeErrors;

    private volatile Thread worker;
    private volatile boolean running;
    private volatile long reports;
    private volatile long rejected;
    private volatile int lastSample = -1;
    private int failures;

    /**
     * @param sensor capteur de charge
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @param nativeErrors destinataire des codes d'erreur négatifs
     */
    public ExternalLoadReporter(ExternalLoadSensor sensor, NativeBridge bridge, long handle,
            IntConsumer nativeErrors) {
        this.sensor = sensor;
        this.bridge = bridge;
        this.handle = handle;
        this.nativeErrors = nativeErrors;
    }

    /**
     * Démarre le fil de relevé. Sans effet s'il tourne déjà, ou si la JVM ne mesure pas
     * le temps CPU par fil : la charge reste alors « non mesurée » (R-660).
     */
    public synchronized void start() {
        if (running) {
            return;
        }
        if (!sensor.supported()) {
            LOGGER.info("Relevé de charge externe indisponible : la JVM ne mesure pas le temps "
                    + "processeur par fil. L'ordonnanceur s'en passera ; le jeu n'est pas affecté.");
            return;
        }
        running = true;
        Thread thread = new Thread(this::loop, THREAD_NAME);
        thread.setDaemon(true);
        thread.setPriority(Thread.MIN_PRIORITY);
        worker = thread;
        thread.start();
    }

    /** Arrête le fil de relevé et attend brièvement sa fin. */
    public synchronized void stop() {
        running = false;
        Thread thread = worker;
        worker = null;
        if (thread == null) {
            return;
        }
        thread.interrupt();
        try {
            thread.join(1_000L);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    private void loop() {
        while (running) {
            try {
                Thread.sleep(PERIOD_MS);
            } catch (InterruptedException e) {
                return;
            }
            try {
                reportOnce();
                failures = 0;
            } catch (RuntimeException | LinkageError e) {
                if (++failures >= MAX_FAILURES) {
                    running = false;
                    LOGGER.warn("Relevé de charge externe arrêté après {} échecs de suite ; "
                            + "le jeu tourne normalement.", MAX_FAILURES, e);
                }
            }
        }
    }

    /**
     * Un relevé : mesure, puis transmission au natif. Le premier relevé ne sert que de
     * référence et ne transmet rien.
     *
     * <p>Visible dans le paquet pour les tests ; la boucle, elle, attend une seconde.
     */
    void reportOnce() {
        int busy = sensor.sample();
        if (busy < 0) {
            return;
        }
        lastSample = busy;
        int code = bridge.schedReportExternalLoad(handle, busy);
        if (code < 0) {
            rejected++;
            nativeErrors.accept(code);
            return;
        }
        reports++;
    }

    /** @return {@code true} si le fil de relevé tourne */
    public boolean running() {
        return running && worker != null && worker.isAlive();
    }

    /** @return le nombre de relevés acceptés par le natif */
    public long reports() {
        return reports;
    }

    /** @return le nombre de relevés refusés par le natif */
    public long rejected() {
        return rejected;
    }

    /** @return le dernier relevé transmis, ou {@code -1} avant le premier */
    public int lastSample() {
        return lastSample;
    }
}
