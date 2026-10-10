package dev.rustforgex.diag;

import dev.rustforgex.telemetry.Anonymizer;
import dev.rustforgex.telemetry.MetricSet;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executor;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Supplier;

/**
 * C-35 : consignation des incidents dans {@code <gameDir>/rustforgex/crash/}.
 *
 * <p>Cahier des charges : PARTIE 4.17, PARTIE 5.33, PARTIE 19.6, R-224. Test : T-411.
 * Maturité : {@code STABLE}.
 *
 * <p>Trois règles :
 * <ul>
 *   <li><strong>jamais d'exception</strong> : la consignation arrive au moment où
 *       quelque chose vient déjà d'échouer, elle ne doit pas ajouter un second échec ;
 *   <li><strong>un incident, un dump</strong> : une accroche ne se désactive qu'une
 *       fois, mais une panic native peut se répéter à chaque tick. Le même échec n'est
 *       consigné qu'une fois par partie, et le total est borné ;
 *   <li><strong>écriture hors du fil appelant, atomique</strong> : fichier temporaire
 *       puis renommage (R-224), sur l'exécuteur des rapports. Seul le relevé des
 *       métriques a lieu sur le fil appelant — il lit l'état du runtime.
 * </ul>
 */
public final class IncidentRecorder {

    /**
     * Dumps écrits au plus par partie.
     *
     * <p>Sept accroches et un point d'entrée natif rapporteur : seize laisse de la marge
     * sans laisser une boucle d'échecs remplir le disque.
     */
    public static final int MAX_DUMPS = 16;

    private final Path directory;
    private final Map<String, String> versions;
    private final Supplier<MetricSet> metrics;
    private final Anonymizer anonymizer;
    private final Executor writer;
    private final boolean enabled;
    private final Set<String> recorded = ConcurrentHashMap.newKeySet();
    private final AtomicInteger written = new AtomicInteger();

    /**
     * @param directory dossier des dumps, {@code <gameDir>/rustforgex/crash}
     * @param versions versions de l'installation, voir {@link IncidentDump#versions}
     * @param metrics relevé des métriques ; peut lever ou rendre {@code null}
     * @param anonymizer assainisseur des textes libres (R-571)
     * @param writer exécuteur de l'écriture, hors du fil du serveur
     * @param enabled {@code diagnostics.report_on_incident}
     */
    public IncidentRecorder(
            Path directory, Map<String, String> versions, Supplier<MetricSet> metrics,
            Anonymizer anonymizer, Executor writer, boolean enabled) {
        this.directory = directory;
        this.versions = Map.copyOf(versions);
        this.metrics = metrics;
        this.anonymizer = anonymizer;
        this.writer = writer;
        this.enabled = enabled;
    }

    /**
     * Consigne un incident.
     *
     * @param incident l'incident
     * @return le chemin du dump une fois écrit ; {@code null} si l'incident n'est pas
     *     consigné (désactivé, déjà consigné, plafond atteint) ; une
     *     {@link UncheckedIOException} si l'écriture échoue
     */
    public CompletableFuture<Path> record(Incident incident) {
        if (!enabled || !recorded.add(incident.key())) {
            return CompletableFuture.completedFuture(null);
        }
        if (written.incrementAndGet() > MAX_DUMPS) {
            return CompletableFuture.completedFuture(null);
        }
        MetricSet snapshot = snapshot();
        try {
            return CompletableFuture.supplyAsync(
                    () -> write(incident, IncidentDump.render(incident, versions, snapshot, anonymizer)),
                    writer);
        } catch (RuntimeException e) {
            // Exécuteur refusé (arrêt de la JVM en cours) : rien à écrire, rien à lever.
            return CompletableFuture.failedFuture(e);
        }
    }

    /** @return le nombre de dumps demandés, plafond compris ({@code rfx.diag.incidents}) */
    public int incidents() {
        return written.get();
    }

    /** Relève les métriques sans jamais lever : un dump sans métriques vaut mieux que rien. */
    private MetricSet snapshot() {
        try {
            return metrics.get();
        } catch (RuntimeException | LinkageError e) {
            return null;
        }
    }

    /**
     * Écrit le dump de façon atomique (R-224).
     *
     * <p>Nommé à la milliseconde, comme les rapports ; deux incidents dans la même
     * milliseconde reçoivent un suffixe plutôt que de s'écraser.
     */
    private Path write(Incident incident, String json) {
        try {
            Files.createDirectories(directory);
            String stem = "rfx-crash-" + incident.at().toEpochMilli();
            Path target = directory.resolve(stem + ".json");
            for (int n = 2; Files.exists(target); n++) {
                target = directory.resolve(stem + "-" + n + ".json");
            }
            Path temporary = directory.resolve(target.getFileName() + ".tmp");
            Files.writeString(temporary, json, StandardCharsets.UTF_8);
            try {
                Files.move(temporary, target, StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException e) {
                Files.move(temporary, target, StandardCopyOption.REPLACE_EXISTING);
            }
            return target;
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }
}
