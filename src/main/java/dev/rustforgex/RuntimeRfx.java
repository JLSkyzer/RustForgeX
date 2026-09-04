package dev.rustforgex;

import dev.rustforgex.bootstrap.Bootstrap;
import dev.rustforgex.bridge.CborLecteur;
import dev.rustforgex.bridge.PontNatif;
import dev.rustforgex.command.RapportStatut;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.forge.VersionForge;

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
public final class RuntimeRfx {

    private static volatile RuntimeRfx instance;

    private final Configuration configuration;
    private final Bootstrap.Rapport rapport;
    private final boolean observationSeule;
    private final PontNatif pont;

    private RuntimeRfx(
            Configuration configuration,
            Bootstrap.Rapport rapport,
            boolean observationSeule,
            PontNatif pont) {
        this.configuration = configuration;
        this.rapport = rapport;
        this.observationSeule = observationSeule;
        this.pont = pont;
    }

    /**
     * Démarre le runtime : configuration, vérification de Forge, puis C-02.
     *
     * @param racine racine de travail, {@code <gameDir>/rustforgex}
     * @param coteClient {@code true} côté client, {@code false} côté serveur dédié
     * @param versionForge version rapportée par Forge, pour FM-01
     * @return l'instance démarrée
     */
    public static synchronized RuntimeRfx demarrer(Path racine, boolean coteClient, String versionForge) {
        Configuration configuration =
                Configuration.charger(racine.resolve(Configuration.CHEMIN_FICHIER), System::getProperty);

        // FM-01 : une version de Forge hors plage n'empêche pas le jeu de tourner ;
        // elle interdit toute transformation (E-1001, mode observation seule).
        boolean observationSeule = !VersionForge.estSupportee(versionForge);

        Bootstrap.Rapport rapport;
        if (observationSeule) {
            rapport = new Bootstrap.Rapport(
                    Bootstrap.Etat.DEGRADED,
                    dev.rustforgex.diag.CodeErreur.FORGE_HORS_PLAGE,
                    "Version de Forge « " + versionForge + " » hors de la plage supportée "
                            + VersionForge.plageLisible()
                            + " : RUSTFORGE-X reste en observation seule.",
                    0, null, 0, List.of());
        } else {
            rapport = Bootstrap.demarrer(Bootstrap.Contexte.reel(racine, configuration, coteClient));
        }

        instance = new RuntimeRfx(configuration, rapport, observationSeule, PontNatif.reel());
        return instance;
    }

    /** @return l'instance courante, ou {@code null} si le mod n'a pas encore démarré */
    public static RuntimeRfx instance() {
        return instance;
    }

    /** @return la configuration effective */
    public Configuration configuration() {
        return configuration;
    }

    /** @return le rapport de démarrage produit par C-02 */
    public Bootstrap.Rapport rapport() {
        return rapport;
    }

    /** @return {@code true} si le runtime natif est actif */
    public boolean actif() {
        return !observationSeule && rapport.pret();
    }

    /** @return les avertissements de configuration, à journaliser au démarrage */
    public List<String> avertissements() {
        return configuration.avertissements();
    }

    /**
     * Compose le rapport de {@code /rfx status}.
     *
     * <p>Un seul appel natif est émis, borné et sans allocation notable : la commande
     * ne peut pas bloquer le thread serveur au-delà du budget de 5 ms (R-601).
     *
     * @return les lignes à afficher
     */
    public List<String> statut() {
        return RapportStatut.composer(rapport, configuration, statutNatif(), observationSeule);
    }

    /** Lit et décode le statut natif, ou renvoie {@code null} si indisponible. */
    private Map<String, Object> statutNatif() {
        if (!actif()) {
            return null;
        }
        try {
            byte[] blob = pont.status(rapport.handle());
            if (blob == null) {
                return null;
            }
            Object valeur = CborLecteur.decoder(blob);
            return valeur instanceof Map<?, ?> ? cast(valeur) : null;
        } catch (CborLecteur.CborInvalide | RuntimeException | LinkageError e) {
            // Un statut illisible ne doit jamais interrompre une commande de
            // diagnostic : c'est précisément dans ce cas qu'on en a besoin.
            return null;
        }
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> cast(Object valeur) {
        return (Map<String, Object>) valeur;
    }

    /**
     * Arrête proprement le runtime natif.
     *
     * <p>Appelé à l'arrêt du serveur. Un échec est ignoré : le processus se termine de
     * toute façon, et une exception à cet instant masquerait la cause réelle d'un
     * éventuel arrêt anormal.
     */
    public synchronized void arreter() {
        if (actif()) {
            try {
                pont.shutdown(rapport.handle());
            } catch (RuntimeException | LinkageError e) {
                // Rien à faire de plus : le processus s'arrête.
            }
        }
        instance = null;
    }
}
