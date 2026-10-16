package dev.rustforgex.sched;

import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.lang.management.ManagementFactory;
import java.lang.management.ThreadMXBean;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicBoolean;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests du capteur de charge externe (C-17, R-421, ADR-046).
 *
 * <p>Chaque test compte les fils d'un groupe isolé qu'il peuple lui-même : le compte est
 * exact, quels que soient les autres fils de la JVM de test. Les fils lancés sont
 * arrêtés après chaque test, y compris quand une assertion échoue.
 */
class ExternalLoadSensorTest {

    private final List<Thread> started = new ArrayList<>();
    private final AtomicBoolean stop = new AtomicBoolean();
    private final CountDownLatch release = new CountDownLatch(1);

    @AfterEach
    void stopEveryThread() throws InterruptedException {
        stop.set(true);
        release.countDown();
        for (Thread thread : started) {
            thread.join(5_000L);
        }
    }

    private void spinning(ThreadGroup group, String name) {
        start(group, name, () -> {
            while (!stop.get()) {
                Thread.onSpinWait();
            }
        }, Thread.State.RUNNABLE);
    }

    private void waiting(ThreadGroup group, String name) {
        start(group, name, () -> {
            try {
                release.await();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }, Thread.State.WAITING);
    }

    /** Lance un fil et attend qu'il soit dans l'état voulu. */
    private void start(ThreadGroup group, String name, Runnable body, Thread.State state) {
        Thread thread = new Thread(group, body, name);
        thread.setDaemon(true);
        thread.start();
        started.add(thread);
        while (thread.getState() != state) {
            Thread.onSpinWait();
        }
    }

    private static ExternalLoadSensor sensorOn(ThreadGroup group, int capacity) {
        return new ExternalLoadSensor(group, capacity, ManagementFactory.getThreadMXBean());
    }

    @Test
    @DisplayName("R-421 : seuls les fils tiers qui consomment du processeur sont comptés")
    void onlyThirdPartyThreadsThatUseTheCpuAreCounted() throws InterruptedException {
        ThreadGroup group = new ThreadGroup("rfx-test-charge");
        spinning(group, "tiers-1");
        spinning(group, "tiers-2");
        waiting(group, "tiers-en-attente");
        spinning(group, "RFX-propre");
        spinning(group, "rustforgex-propre");
        ExternalLoadSensor sensor = sensorOn(group, 256);

        assertEquals(-1, sensor.sample(), "le premier relevé ne sert que de référence");
        Thread.sleep(300L);
        assertEquals(2, sensor.sample(),
                "deux tiers occupés ; ni le fil en attente, ni les fils de RUSTFORGE-X");
    }

    /**
     * ADR-046 : un fil {@code RUNNABLE} pour la JVM mais bloqué dans un appel natif ne
     * consomme pas de processeur. Ici, un fil qui dort : il ne compte pas.
     */
    @Test
    @DisplayName("ADR-046 : un fil qui attend ne compte pas, même longtemps")
    void aWaitingThreadNeverCounts() throws InterruptedException {
        ThreadGroup group = new ThreadGroup("rfx-test-attente");
        for (int i = 0; i < 20; i++) {
            waiting(group, "attente-" + i);
        }
        ExternalLoadSensor sensor = sensorOn(group, 4);

        sensor.sample();
        Thread.sleep(300L);
        assertEquals(0, sensor.sample());
    }

    @Test
    @DisplayName("Un tableau trop petit est agrandi, sans omettre de fil")
    void aTooSmallArrayGrowsWithoutMissingThreads() throws InterruptedException {
        ThreadGroup group = new ThreadGroup("rfx-test-debordement");
        for (int i = 0; i < 40; i++) {
            waiting(group, "attente-" + i);
        }
        spinning(group, "tiers-1");
        spinning(group, "tiers-2");
        spinning(group, "tiers-3");

        ExternalLoadSensor sensor = sensorOn(group, 4);
        sensor.sample();
        Thread.sleep(300L);
        assertEquals(3, sensor.sample());
        Thread.sleep(300L);
        assertEquals(3, sensor.sample(), "le relevé suivant repart du tableau agrandi");
    }

    @Test
    @DisplayName("R-660 : sans mesure du temps processeur par fil, le capteur se déclare absent")
    void withoutCpuTimeTheSensorDeclaresItselfUnsupported() {
        ThreadMXBean real = ManagementFactory.getThreadMXBean();
        ThreadMXBean blind = (ThreadMXBean) Proxy.newProxyInstance(
                ThreadMXBean.class.getClassLoader(), new Class<?>[] {ThreadMXBean.class},
                (proxy, method, args) -> method.getName().equals("isThreadCpuTimeSupported")
                        ? Boolean.FALSE
                        : method.invoke(real, args));

        assertFalse(new ExternalLoadSensor(new ThreadGroup("rfx-test-aveugle"), 8, blind)
                .supported());
        assertTrue(new ExternalLoadSensor().supported(), "la JVM de test mesure le temps CPU");
    }

    @Test
    @DisplayName("Les fils de RUSTFORGE-X sont reconnus à leur nom, et eux seuls")
    void ownThreadsAreRecognisedByName() {
        assertTrue(ExternalLoadSensor.isOwn(new Thread("RFX-StackSampler")));
        assertTrue(ExternalLoadSensor.isOwn(new Thread(ExternalLoadReporter.THREAD_NAME)));
        assertTrue(ExternalLoadSensor.isOwn(new Thread("rustforgex-report")));
        assertFalse(ExternalLoadSensor.isOwn(new Thread("Server thread")));
        assertFalse(ExternalLoadSensor.isOwn(new Thread("dimthread_server_overworld")));
    }
}
