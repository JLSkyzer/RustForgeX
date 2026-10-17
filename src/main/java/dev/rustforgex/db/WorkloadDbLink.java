package dev.rustforgex.db;

import dev.rustforgex.bridge.Cbor;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.forge.ModEntry;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.function.IntConsumer;

/**
 * C-09, côté Java : ouvre la base des unités de travail, puis la fait synchroniser
 * périodiquement par le natif, hors du tick.
 *
 * <p>Cahier des charges : PARTIE 5.9 (R-360, R-361), PARTIE 4.18, DM-05. Décision :
 * ADR-049. Maturité : {@code STABLE}.
 *
 * <p>Tout se passe sur le fil {@code rustforgex-db}, de priorité minimale :
 * <ol>
 *   <li>calculer l'environnement — versions, et pour chaque mod
 *       {@code modid@version@début du sha256 du jar}. Hacher les jars prend du temps ;
 *       rien de cela ne touche au démarrage du jeu ;</li>
 *   <li>ouvrir la base : le natif la relit en arrière-plan ;</li>
 *   <li>toutes les cinq minutes, faire avancer la synchronisation tranche par tranche.
 *       Entre deux tranches, le verrou du runtime est rendu et le fil s'efface une
 *       milliseconde : le tick n'attend jamais plus d'une tranche.</li>
 * </ol>
 *
 * <p>La fermeture n'est pas ici : l'arrêt du runtime natif confie le dernier lot et
 * attend son écriture, hors de son verrou.
 *
 * <p>Ne lève jamais : un code d'erreur est compté et transmis au destinataire d'erreurs ;
 * cinq échecs de suite arrêtent le fil sans toucher au jeu.
 */
public final class WorkloadDbLink {

    /** Période des synchronisations, en millisecondes. */
    public static final long PERIOD_MS = 5 * 60 * 1_000L;

    /** Nom du fil : le préfixe {@code rustforgex-} l'exclut du relevé de charge. */
    public static final String THREAD_NAME = "rustforgex-db";

    /** Version du blob {@code DbOpen}, égale à {@code MODEL_SCHEMA_VERSION}. */
    static final int SCHEMA = 1;

    /** Tranches au plus par cycle : borne une boucle qui ne finirait pas. */
    static final int MAX_STEPS_PER_CYCLE = 100_000;

    private static final int MAX_FAILURES = 5;

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex");

    private final NativeBridge bridge;
    private final long handle;
    private final Path directory;
    private final Map<String, String> versions;
    private final IntConsumer nativeErrors;
    private final long periodMs;

    private volatile Thread worker;
    private volatile boolean running;
    private volatile boolean opened;
    private volatile long cycles;
    private volatile long rejected;
    private int failures;

    /**
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @param directory répertoire de la base, {@code <gameDir>/rustforgex/cache}
     * @param versions versions de l'installation ({@code minecraft}, {@code forge},
     *     {@code java})
     * @param nativeErrors destinataire des codes d'erreur négatifs
     */
    public WorkloadDbLink(NativeBridge bridge, long handle, Path directory,
            Map<String, String> versions, IntConsumer nativeErrors) {
        this(bridge, handle, directory, versions, nativeErrors, PERIOD_MS);
    }

    WorkloadDbLink(NativeBridge bridge, long handle, Path directory,
            Map<String, String> versions, IntConsumer nativeErrors, long periodMs) {
        this.bridge = bridge;
        this.handle = handle;
        this.directory = directory;
        this.versions = Map.copyOf(versions);
        this.nativeErrors = nativeErrors;
        this.periodMs = periodMs;
    }

    /**
     * Démarre le fil. Sans effet s'il tourne déjà.
     *
     * @param mods inventaire des mods (C-41), dressé au chargement complet
     */
    public synchronized void start(List<ModEntry> mods) {
        if (running) {
            return;
        }
        running = true;
        List<ModEntry> snapshot = List.copyOf(mods);
        Thread thread = new Thread(() -> loop(snapshot), THREAD_NAME);
        thread.setDaemon(true);
        thread.setPriority(Thread.MIN_PRIORITY);
        worker = thread;
        thread.start();
    }

    /**
     * Arrête le fil et attend brièvement sa fin. À appeler avant l'arrêt du natif : le
     * fil ne doit plus l'appeler.
     */
    public synchronized void stop() {
        running = false;
        Thread thread = worker;
        worker = null;
        if (thread == null) {
            return;
        }
        thread.interrupt();
        try {
            thread.join(2_000L);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    private void loop(List<ModEntry> mods) {
        try {
            if (!open(mods)) {
                running = false;
                return;
            }
        } catch (RuntimeException | LinkageError e) {
            running = false;
            LOGGER.warn("Base des unités de travail non ouverte ({}). Rien ne sera relu ni "
                    + "écrit ; le jeu tourne normalement.", e.toString());
            return;
        }
        while (running) {
            try {
                Thread.sleep(periodMs);
                syncCycle();
                failures = 0;
            } catch (InterruptedException e) {
                return;
            } catch (RuntimeException | LinkageError e) {
                if (++failures >= MAX_FAILURES) {
                    running = false;
                    LOGGER.warn("Synchronisation de la base des unités de travail arrêtée "
                            + "après {} échecs de suite ; le jeu tourne normalement.",
                            MAX_FAILURES, e);
                }
            }
        }
    }

    /**
     * Calcule l'environnement et ouvre la base.
     *
     * <p>Visible dans le paquet pour les tests.
     *
     * @param mods inventaire des mods
     * @return {@code true} si le natif a accepté l'ouverture
     */
    boolean open(List<ModEntry> mods) {
        byte[] blob = openBlob(directory, versions, modSet(mods));
        int code = bridge.dbOpen(handle, blob);
        if (code < 0) {
            rejected++;
            nativeErrors.accept(code);
            LOGGER.warn("Base des unités de travail refusée par le natif (code {}). Rien ne "
                    + "sera relu ni écrit ; le jeu tourne normalement.", code);
            return false;
        }
        opened = true;
        LOGGER.info("Base des unités de travail ouverte ({} mods) : relecture de {} en "
                + "arrière-plan, écriture toutes les {} s et à l'arrêt.",
                mods.size(), directory, periodMs / 1_000L);
        return true;
    }

    /**
     * Un cycle : fait avancer la synchronisation jusqu'à ce que le natif l'annonce
     * terminée.
     *
     * <p>Visible dans le paquet pour les tests.
     *
     * @throws InterruptedException si le fil est interrompu entre deux tranches
     */
    void syncCycle() throws InterruptedException {
        for (int step = 0; step < MAX_STEPS_PER_CYCLE; step++) {
            int result = bridge.dbSync(handle);
            if (result < 0) {
                rejected++;
                nativeErrors.accept(result);
                return;
            }
            if (result == 1) {
                cycles++;
                return;
            }
            // Le verrou du runtime est rendu : laisser le tick le prendre.
            Thread.sleep(1L);
        }
    }

    /**
     * Ensemble des mods, un élément par mod : {@code modid@version@empreinte}.
     *
     * @param mods inventaire des mods
     * @return les éléments, dans l'ordre de l'inventaire ; le natif trie avant de hacher
     */
    static List<String> modSet(List<ModEntry> mods) {
        List<String> set = new ArrayList<>(mods.size());
        for (ModEntry mod : mods) {
            set.add(mod.modId() + "@" + mod.version() + "@" + mod.ownerModHash());
        }
        return set;
    }

    /**
     * Blob {@code DbOpen}, à l'identique de {@code rfx_model::DbOpen}.
     *
     * @param directory répertoire de la base
     * @param versions versions de l'installation
     * @param mods ensemble des mods
     * @return le blob CBOR
     */
    static byte[] openBlob(Path directory, Map<String, String> versions, List<String> mods) {
        Map<String, Object> table = Cbor.map();
        table.put("schema", SCHEMA);
        table.put("dir", directory.toAbsolutePath().toString());
        table.put("minecraft", versions.getOrDefault("minecraft", ""));
        table.put("forge", versions.getOrDefault("forge", ""));
        table.put("java", versions.getOrDefault("java", ""));
        table.put("mods", List.copyOf(mods));
        return Cbor.encode(table);
    }

    /** @return {@code true} si le fil tourne */
    public boolean running() {
        return running && worker != null && worker.isAlive();
    }

    /** @return {@code true} une fois la base ouverte */
    public boolean opened() {
        return opened;
    }

    /** @return le nombre de cycles de synchronisation terminés */
    public long cycles() {
        return cycles;
    }

    /** @return le nombre d'appels refusés par le natif */
    public long rejected() {
        return rejected;
    }
}
