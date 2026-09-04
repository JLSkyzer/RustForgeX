package dev.rustforgex;

import dev.rustforgex.bootstrap.Bootstrap;
import dev.rustforgex.bridge.CborReader;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.command.StatusReport;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.ErrorCode;
import dev.rustforgex.forge.ForgeVersions;
import net.minecraft.network.chat.Component;

import java.nio.file.Path;
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

    private static volatile RfxRuntime instance;

    private final Configuration configuration;
    private final Bootstrap.Report report;
    private final boolean observeOnly;
    private final NativeBridge bridge;

    private RfxRuntime(
            Configuration configuration,
            Bootstrap.Report report,
            boolean observeOnly,
            NativeBridge bridge) {
        this.configuration = configuration;
        this.report = report;
        this.observeOnly = observeOnly;
        this.bridge = bridge;
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
        Configuration configuration =
                Configuration.load(root.resolve(Configuration.FILE_PATH), System::getProperty);

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

        instance = new RfxRuntime(configuration, report, observeOnly, NativeBridge.real());
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
        return StatusReport.compose(report, configuration, nativeStatus(), observeOnly);
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
