package dev.rustforgex;

import dev.rustforgex.bootstrap.Bootstrap;
import com.mojang.logging.LogUtils;
import dev.rustforgex.bridge.CborReader;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.command.StatusReport;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.forge.EventDispatchTable;
import dev.rustforgex.forge.EventObserver;
import dev.rustforgex.diag.ErrorCode;
import dev.rustforgex.forge.ForgeVersions;
import dev.rustforgex.forge.ModDiscovery;
import dev.rustforgex.forge.ModSource;
import dev.rustforgex.telemetry.Anonymizer;
import dev.rustforgex.telemetry.MetricSet;
import dev.rustforgex.telemetry.MetricsJson;
import dev.rustforgex.telemetry.Telemetry;
import dev.rustforgex.forge.TickCycle;
import dev.rustforgex.instrument.Instrumentation;
import dev.rustforgex.instrument.ProbeRegistry;
import dev.rustforgex.instrument.ProbeSink;
import net.minecraft.network.chat.Component;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.util.List;
import java.util.Map;

/**
 * État global de RUSTFORGE-X côté Java.
 *
 * <p>Composants : C-01 (intégration), C-02 (démarrage), C-38 (statut).
 * Maturité : {@code STABLE}.
 *
 * <p>Point unique de rassemblement entre la configuration, le rapport de démarrage et
 * le runtime natif. Tout le reste du mod passe par ici plutôt que d'appeler la
 * frontière directement.
 *
 * <p>Aucune méthode ne lève d'exception : un mod d'optimisation qui empêche le jeu de
 * démarrer est pire que pas de mod du tout.
 */
public final class RfxRuntime {

    private static final org.slf4j.Logger LOGGER = LogUtils.getLogger();

    private static volatile RfxRuntime instance;

    private final Configuration configuration;
    private final Bootstrap.Report report;
    private final boolean observeOnly;
    private final NativeBridge bridge;
    private final TickCycle tickCycle;
    private final ProbeSink probeSink;
    private final Instrumentation instrumentation;

    /**
     * Inventaire des mods, ou {@code null} tant que la découverte n'a pas eu lieu.
     *
     * <p>{@code volatile} : dressé sur le fil de chargement, lu depuis le fil serveur
     * quand une commande le demande.
     */
    private volatile ModDiscovery modDiscovery;

    /**
     * Racine de travail, {@code <gameDir>/rustforgex}.
     *
     * <p>Retenue pour deux usages : écrire les rapports sous {@code reports/}, et servir
     * de racine à l'anonymisation (R-571) — c'est le chemin qu'un rapport ne doit pas
     * révéler.
     */
    private final Path root;
    private final EventObserver eventObserver;

    private RfxRuntime(
            Path root,
            Configuration configuration,
            Bootstrap.Report report,
            boolean observeOnly,
            NativeBridge bridge,
            boolean clientSide) {
        this.root = root;
        this.configuration = configuration;
        this.report = report;
        this.observeOnly = observeOnly;
        this.bridge = bridge;
        // Le cycle de tick et le puits de sondes n'existent que si le runtime natif
        // tourne : sans lui, aucune fenêtre à ouvrir, et surtout aucun appel à émettre
        // à chaque tick.
        boolean live = !observeOnly && report.ready();
        this.probeSink = live ? new ProbeSink(bridge, report.handle()) : null;
        this.tickCycle = live
                ? new TickCycle(bridge, report.handle(),
                        clientSide ? TickCycle.SIDE_CLIENT : TickCycle.SIDE_SERVER, probeSink)
                : null;

        // C-05 avant C-04 : le profiler plafonne la profondeur des sondes, et une
        // sonde enregistrée alors qu'il est encore à l'arrêt serait armée à OFF.
        if (live) {
            bridge.profilerStart(report.handle());
        }
        this.instrumentation =
                Instrumentation.arm(live ? bridge : null, report.handle(), clientSide);

        // L'échantillonnage n'a de sens que si des sondes existent : c'est le registre
        // qui sait à quelle méthode répond chaque identifiant (PARTIE 5.5, R-322).
        if (tickCycle != null && instrumentation.registry() != null) {
            tickCycle.enableSampling(instrumentation.registry().frameIndex());
        }

        // C-06, étape 1 : observer le bus sans y toucher. Aucun auditeur tiers n'est
        // enveloppé, donc aucun risque pour R-330 ; le coût, lui, reste à mesurer.
        this.eventObserver = live ? EventObserver.onForgeBus() : null;
        if (eventObserver != null) {
            eventObserver.start();
        }
    }

    /**
     * Démarre le runtime : configuration, vérification de Forge, puis C-02.
     *
     * @param root racine de travail, {@code <gameDir>/rustforgex}
     * @param clientSide {@code true} côté client, {@code false} côté serveur dédié
     * @param forgeVersion version rapportée par Forge, pour FM-01
     * @return l'instance démarrée
     */
    public static synchronized RfxRuntime start(Path root, boolean clientSide, String forgeVersion) {
        return start(root, loadConfiguration(root), clientSide, forgeVersion);
    }

    /**
     * Charge la configuration sans rien démarrer.
     *
     * <p>Séparée du démarrage parce qu'une décision se lit dans la configuration
     * <em>avant</em> qu'il y ait un runtime : {@code instrumentation.early_arm} dit à
     * quel moment démarrer, et ne peut donc pas être lue par ce qui a déjà démarré.
     *
     * @param root racine de travail, {@code <gameDir>/rustforgex}
     * @return la configuration effective, porteuse de ses avertissements
     */
    public static Configuration loadConfiguration(Path root) {
        return Configuration.load(root.resolve(Configuration.FILE_PATH), System::getProperty);
    }

    /**
     * Démarre le runtime avec une configuration déjà lue.
     *
     * @param root racine de travail
     * @param configuration configuration effective, telle que rendue par
     *     {@link #loadConfiguration(Path)}
     * @param clientSide {@code true} côté client
     * @param forgeVersion version rapportée par Forge, pour FM-01
     * @return l'instance démarrée
     */
    public static synchronized RfxRuntime start(
            Path root, Configuration configuration, boolean clientSide, String forgeVersion) {
        // FM-01 : une version de Forge hors plage n'empêche pas le jeu de tourner ;
        // elle interdit toute transformation (E-1001, mode observation seule).
        boolean observeOnly = !ForgeVersions.isSupported(forgeVersion);

        Bootstrap.Report report;
        if (observeOnly) {
            report = new Bootstrap.Report(
                    Bootstrap.State.DEGRADED,
                    ErrorCode.FORGE_OUT_OF_RANGE,
                    "Version de Forge « " + forgeVersion + " » hors de la plage supportée "
                            + ForgeVersions.readableRange()
                            + " : RUSTFORGE-X reste en observation seule.",
                    0, null, 0, List.of());
        } else {
            report = Bootstrap.start(Bootstrap.Context.real(root, configuration, clientSide));
        }

        instance = new RfxRuntime(
                root, configuration, report, observeOnly, NativeBridge.real(), clientSide);
        return instance;
    }

    /** @return l'instance courante, ou {@code null} si le mod n'a pas encore démarré */
    public static RfxRuntime instance() {
        return instance;
    }

    /** @return la configuration effective */
    public Configuration configuration() {
        return configuration;
    }

    /** @return le rapport de démarrage produit par C-02 */
    public Bootstrap.Report report() {
        return report;
    }

    /** @return {@code true} si le runtime natif est actif */
    public boolean active() {
        return !observeOnly && report.ready();
    }

    /**
     * Cycle de tick, ou {@code null} si le runtime natif n'est pas actif.
     *
     * @return le pilote de la fenêtre de tick (IF-02)
     */
    public TickCycle tickCycle() {
        return tickCycle;
    }

    /**
     * Observateur du bus d'événements (C-06, étape 1).
     *
     * @return l'observateur, ou {@code null} si le runtime natif n'est pas actif
     */
    public EventObserver eventObserver() {
        return eventObserver;
    }

    /**
     * Dresse l'inventaire des mods (C-41), une fois pour la durée de la partie.
     *
     * <p>Appelé à {@code FMLLoadCompleteEvent} : plus tôt, la liste de Forge est
     * incomplète ; plus tard, des classes auraient déjà été sondées sans propriétaire.
     *
     * @param source d'où viennent les mods, injectable pour les tests
     * @return l'inventaire dressé, jamais {@code null}
     */
    public synchronized ModDiscovery discoverMods(ModSource source) {
        ModDiscovery found = ModDiscovery.from(source);
        this.modDiscovery = found;
        return found;
    }

    /**
     * Inventaire des mods (C-41).
     *
     * @return l'inventaire, ou {@code null} si la découverte n'a pas encore eu lieu
     */
    public ModDiscovery modDiscovery() {
        return modDiscovery;
    }

    /**
     * Relève les métriques du runtime (C-34).
     *
     * <p>Un appel natif — celui du statut — puis une mise en forme. À n'appeler ni dans
     * un tick ni dans une boucle (INV-14).
     *
     * @return le recueil, jamais {@code null}
     */
    public MetricSet metrics() {
        Instrumentation instr = instrumentation;
        ProbeRegistry registry = instr == null ? null : instr.registry();
        ModDiscovery discovery = modDiscovery;
        EventObserver observer = eventObserver;
        EventDispatchTable events = observer == null ? null : observer.table();

        return Telemetry.collect(
                nativeStatus(),
                instr == null ? null : new Telemetry.InstrumentationCounts(
                        instr.armed(), instr.classesSeen(), instr.classesMissed(),
                        instr.methodsProbed(), instr.transformFailures(),
                        registry == null ? 0L : registry.requested(),
                        registry == null ? 0L : registry.unattributed()),
                discovery == null ? null : new Telemetry.DiscoveryCounts(
                        discovery.modCount(), discovery.knownModules(),
                        discovery.knownPackages(), discovery.durationMs()),
                events == null ? null : new Telemetry.EventCounts(
                        events.dispatched(), events.knownTypes(),
                        events.timed(), events.abandoned()));
    }

    /**
     * Classement des unités les plus coûteuses (C-35, {@code /rfx top}).
     *
     * <p>Un appel natif, puis un décodage. Comme le statut, à n'appeler ni dans un tick
     * ni dans une boucle (INV-14).
     *
     * @param limit nombre maximal d'entrées
     * @return le classement décodé, ou {@code null} s'il est indisponible
     */
    public Map<String, Object> topWorkloads(int limit) {
        if (!active()) {
            return null;
        }
        byte[] blob = bridge.profilerTop(report.handle(), Math.max(limit, 0));
        if (blob == null || blob.length == 0) {
            // Distinguer les causes d'un classement absent : sans cela, « indisponible »
            // recouvre le natif muet et le blob illisible, qui n'ont ni la même cause ni
            // le même correctif. Une invocation sur deux échouait sans qu'on sache par
            // où, et deux hypothèses successives se sont révélées fausses.
            LOGGER.debug("Classement indisponible : le natif n'a rien rendu ({}).",
                    blob == null ? "null" : "0 octet");
            return null;
        }
        try {
            Object decoded = CborReader.decode(blob);
            if (decoded instanceof Map<?, ?> table) {
                return cast(table);
            }
            LOGGER.debug("Classement indisponible : {} octets décodés en {}, pas en table.",
                    blob.length, decoded == null ? "null" : decoded.getClass().getSimpleName());
            return null;
        } catch (CborReader.InvalidCbor | RuntimeException e) {
            // Un classement illisible n'est pas une panne du jeu : la commande dira
            // qu'il est indisponible, et le tick continue.
            LOGGER.debug("Classement indisponible : {} octets illisibles ({}).",
                    blob.length, e.toString());
            return null;
        }
    }

    /**
     * Écrit un rapport de métriques exploitable (C-34, C-35).
     *
     * <p>Le fichier est nommé par l'horodatage, jamais écrasé : deux rapports pris à
     * deux moments sont deux observations, et l'un ne remplace pas l'autre.
     *
     * <p>Les chemins y sont anonymisés (R-571) : un rapport est fait pour être envoyé.
     *
     * @return le chemin du rapport écrit
     * @throws IOException si le fichier ne peut être écrit
     */
    public Path writeReport() throws IOException {
        Path directory = root.resolve("reports");
        Files.createDirectories(directory);
        Path file = directory.resolve("rfx-report-" + Instant.now().getEpochSecond() + ".json");
        Files.writeString(file,
                MetricsJson.render(metrics(), Anonymizer.ofSystem(root)),
                StandardCharsets.UTF_8);
        return file;
    }

    /**
     * État de l'instrumentation (C-04).
     *
     * @return l'état, jamais {@code null} — voir {@link Instrumentation.State}
     */
    public Instrumentation instrumentation() {
        return instrumentation;
    }

    /**
     * Puits de sondes, ou {@code null} si le runtime natif n'est pas actif.
     *
     * @return le puits d'enregistrements de profilage (IF-03)
     */
    public ProbeSink probeSink() {
        return probeSink;
    }

    /** @return les avertissements de configuration, à journaliser au démarrage */
    public List<String> warnings() {
        return configuration.warnings();
    }

    /**
     * Compose le rapport de {@code /rfx status}.
     *
     * <p>Un seul appel natif est émis, borné et sans allocation notable : la commande
     * ne peut pas bloquer le thread serveur au-delà du budget de 5 ms (R-601).
     *
     * @return les lignes à afficher
     */
    public List<Component> status() {
        return StatusReport.compose(report, configuration, nativeStatus(), observeOnly,
                instrumentation == null ? null : instrumentation.view());
    }

    /**
     * Compteurs du profiler (C-05), ou {@code null} s'ils sont indisponibles.
     *
     * <p>Un appel natif, comme {@link #status()}. Destiné au harnais de benchmark
     * (C-36), qui doit pouvoir consigner ce que le profiler dit de son propre coût :
     * c'est la seule façon de confronter la mesure interne — la pause de la
     * PARTIE 12.4 — à la mesure externe que la campagne produit.
     *
     * @return les compteurs, ou {@code null}
     */
    public Map<String, Object> profilerCounters() {
        Map<String, Object> status = nativeStatus();
        if (status == null) {
            return null;
        }
        Object profiler = status.get("profiler");
        return profiler instanceof Map<?, ?> ? cast(profiler) : null;
    }

    /** Lit et décode le statut natif, ou renvoie {@code null} si indisponible. */
    private Map<String, Object> nativeStatus() {
        if (!active()) {
            return null;
        }
        try {
            byte[] blob = bridge.status(report.handle());
            if (blob == null) {
                return null;
            }
            Object value = CborReader.decode(blob);
            return value instanceof Map<?, ?> ? cast(value) : null;
        } catch (CborReader.InvalidCbor | RuntimeException | LinkageError e) {
            // Un statut illisible ne doit jamais interrompre une commande de
            // diagnostic : c'est précisément dans ce cas qu'on en a besoin.
            return null;
        }
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> cast(Object value) {
        return (Map<String, Object>) value;
    }

    /**
     * Arrête proprement le runtime natif.
     *
     * <p>Appelé à l'arrêt du serveur. Un échec est ignoré : le processus se termine de
     * toute façon, et une exception à cet instant masquerait la cause réelle d'un
     * éventuel arrêt anormal.
     */
    public synchronized void shutdown() {
        // Désarmer d'abord : le transformateur ne doit plus demander d'identifiant à
        // un runtime qu'on est en train de fermer.
        if (tickCycle != null) {
            tickCycle.stopSampling();
        }
        if (eventObserver != null) {
            eventObserver.stop();
        }
        instrumentation.disarm();
        if (active()) {
            try {
                bridge.shutdown(report.handle());
            } catch (RuntimeException | LinkageError e) {
                // Rien à faire de plus : le processus s'arrête.
            }
        }
        instance = null;
    }
}
