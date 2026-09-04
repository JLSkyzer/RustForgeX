package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.NativeLoader.EchecChargement;
import dev.rustforgex.bootstrap.NativeLoader.Plateforme;
import dev.rustforgex.bridge.PontNatif;
import dev.rustforgex.bridge.RfxNative;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.CodeErreur;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * C-02 : initialisation du runtime, ou refus propre de s'activer.
 *
 * <p>Cahier des charges : PARTIE 5.2. Machine à états :
 *
 * <pre>{@code
 * INIT -> PROBE -> LOAD_NATIVE -> HANDSHAKE -> CONFIGURE -> READY | DEGRADED | DISABLED
 * }</pre>
 *
 * <p>Tests : T-110 à T-114. Maturité : {@code STABLE}.
 *
 * <p><strong>Garantie centrale</strong> : aucune exception ne remonte de
 * {@link #demarrer}. Tout échec d'une étape est journalisé et conduit à
 * {@code DEGRADED} ou {@code DISABLED}, états dans lesquels le jeu tourne exactement
 * comme sans le mod : aucune instrumentation, aucun thread supplémentaire.
 */
public final class Bootstrap {

    /** États de la séquence d'initialisation (SM du composant C-02). */
    public enum Etat {
        /** Point de départ. */
        INIT,
        /** Configuration lue, plateforme en cours de détection. */
        PROBE,
        /** Extraction et chargement de la bibliothèque native. */
        LOAD_NATIVE,
        /** Vérification de la version d'ABI (R-702). */
        HANDSHAKE,
        /** Initialisation du runtime natif et sonde matérielle. */
        CONFIGURE,
        /** Runtime opérationnel. */
        READY,
        /** Java pur : le jeu tourne, le runtime n'entreprend rien. */
        DEGRADED,
        /** Désactivé explicitement, ou refus d'activation pour cause d'incompatibilité. */
        DISABLED
    }

    /**
     * Résultat d'une séquence de démarrage.
     *
     * @param etat état final atteint
     * @param code code de l'annexe A.2 expliquant un état non nominal, ou {@code null}
     * @param message message destiné aux journaux, jamais {@code null}
     * @param handle handle du runtime natif si {@code etat == READY}, sinon {@code 0}
     * @param bibliotheque chemin du binaire chargé, ou {@code null}
     * @param dureeMs durée totale du démarrage, en millisecondes
     * @param journal trace des étapes, dans l'ordre
     */
    public record Rapport(
            Etat etat,
            CodeErreur code,
            String message,
            long handle,
            Path bibliotheque,
            long dureeMs,
            List<String> journal) {

        /** @return {@code true} si le runtime natif est utilisable */
        public boolean pret() {
            return etat == Etat.READY;
        }
    }

    /**
     * Dépendances du démarrage, injectables pour les tests.
     *
     * @param racine racine de travail, typiquement {@code <gameDir>/rustforgex}
     * @param configuration configuration déjà chargée et validée (C-37)
     * @param coteClient {@code true} côté client, {@code false} côté serveur dédié
     * @param plateforme plateforme cible, vide si l'hôte n'est pas supporté
     * @param loader chargeur de bibliothèque native (C-03)
     * @param chargeur action de chargement effectif, isolée pour les tests
     * @param pont pont vers le runtime natif (IF-01)
     */
    public record Contexte(
            Path racine,
            Configuration configuration,
            boolean coteClient,
            Optional<Plateforme> plateforme,
            NativeLoader loader,
            Chargeur chargeur,
            PontNatif pont) {

        /** Action de chargement d'une bibliothèque déjà extraite et vérifiée. */
        @FunctionalInterface
        public interface Chargeur {

            /**
             * @param bibliotheque chemin du binaire vérifié
             * @throws EchecChargement si le système refuse le chargement
             */
            void charger(Path bibliotheque) throws EchecChargement;
        }

        /**
         * Contexte de production : ressources du JAR, chargement réel, pont JNI.
         *
         * @param racine racine de travail
         * @param configuration configuration chargée
         * @param coteClient côté d'exécution
         * @return le contexte à passer à {@link Bootstrap#demarrer(Contexte)}
         */
        public static Contexte reel(Path racine, Configuration configuration, boolean coteClient) {
            return new Contexte(
                    racine,
                    configuration,
                    coteClient,
                    NativeLoader.plateformeHote(),
                    new NativeLoader(),
                    NativeLoader::charger,
                    PontNatif.reel());
        }
    }

    /**
     * Empêche une seconde initialisation dans le même processus (T-114).
     *
     * <p>Le runtime natif refuse déjà la double initialisation avec {@code E-1004}
     * (R-520). Ce garde-fou côté Java évite en plus de recharger la bibliothèque et de
     * relancer une sonde, opérations inutiles et coûteuses.
     */
    private static final AtomicBoolean DEMARRE = new AtomicBoolean(false);

    private Bootstrap() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Exécute la séquence d'initialisation.
     *
     * @param contexte dépendances du démarrage
     * @return le rapport final, jamais {@code null}
     */
    public static Rapport demarrer(Contexte contexte) {
        long debut = System.nanoTime();
        List<String> journal = new ArrayList<>();

        if (!DEMARRE.compareAndSet(false, true)) {
            return rapport(Etat.DISABLED, CodeErreur.DOUBLE_INIT,
                    "Le runtime a déjà été initialisé dans ce processus.",
                    0, null, debut, journal);
        }

        try {
            return sequence(contexte, journal, debut);
        } catch (RuntimeException | LinkageError e) {
            // Filet de sécurité : aucune exception ne doit remonter jusqu'à Forge, sous
            // peine d'empêcher le jeu de démarrer (PARTIE 5.2, dernière ligne de
            // l'algorithme normatif).
            return rapport(Etat.DEGRADED, CodeErreur.CHARGEMENT_ECHOUE,
                    "Erreur inattendue au démarrage : " + e,
                    0, null, debut, journal);
        }
    }

    /** Séquence normative en dix étapes (PARTIE 5.2). */
    private static Rapport sequence(Contexte contexte, List<String> journal, long debut) {

        // 1 et 2. La configuration est déjà lue (C-37) ; reste à l'honorer.
        journal.add("INIT : configuration chargée, mode " + contexte.configuration().mode());
        if (!contexte.configuration().activeSur(contexte.coteClient())) {
            return rapport(Etat.DISABLED, null,
                    "Désactivé par configuration sur ce côté : le jeu tourne sans RUSTFORGE-X.",
                    0, null, debut, journal);
        }

        // 3. Détecter la plateforme et sélectionner le natif.
        journal.add("PROBE : détection de la plateforme");
        Optional<Plateforme> plateforme = contexte.plateforme();
        if (plateforme.isEmpty()) {
            return rapport(Etat.DEGRADED, CodeErreur.NATIF_ABSENT,
                    "Plateforme non supportée (" + System.getProperty("os.name") + " / "
                            + System.getProperty("os.arch")
                            + ") : aucun binaire natif ne correspond. Le jeu tourne en Java pur.",
                    0, null, debut, journal);
        }

        // 4 et 5. Extraire puis charger la bibliothèque (C-03).
        journal.add("LOAD_NATIVE : extraction pour " + plateforme.get().repertoire());
        Path bibliotheque;
        try {
            bibliotheque = contexte.loader().preparer(plateforme.get(), contexte.racine());
            contexte.chargeur().charger(bibliotheque);
        } catch (EchecChargement e) {
            // FM-04, FM-05, FM-08 : un hash invalide est une altération du binaire et
            // interdit toute activation ; une absence ou un refus système dégradent.
            Etat etat = e.code() == CodeErreur.HASH_NATIF_INVALIDE ? Etat.DISABLED : Etat.DEGRADED;
            return rapport(etat, e.code(), e.getMessage(), 0, null, debut, journal);
        }

        // 6. Handshake ABI : aucune autre fonction n'est appelée avant (R-702).
        journal.add("HANDSHAKE : vérification de l'ABI");
        int abi = contexte.pont().abiVersion();
        if (abi != RfxNative.ABI_ATTENDUE) {
            return rapport(Etat.DISABLED, CodeErreur.ABI_INCOMPATIBLE,
                    "Le binaire natif annonce l'ABI " + abi + ", cette version attend l'ABI "
                            + RfxNative.ABI_ATTENDUE + ". Aucun appel natif ne sera émis.",
                    0, bibliotheque, debut, journal);
        }

        // 7. Initialiser le runtime natif.
        journal.add("CONFIGURE : initialisation du runtime natif");
        long handle = contexte.pont().init(contexte.configuration().versCborNatif());
        if (handle <= 0) {
            CodeErreur code = CodeErreur.depuisCodeNatif((int) handle);
            return rapport(Etat.DEGRADED,
                    code != null ? code : CodeErreur.CHARGEMENT_ECHOUE,
                    "Initialisation du runtime natif refusée (code " + handle + ").",
                    0, bibliotheque, debut, journal);
        }

        // 8. Sonder le matériel puis calibrer le coût de la frontière (C-45).
        int codeSonde = contexte.pont().hwProbe(handle);
        if (codeSonde != 0) {
            journal.add("CONFIGURE : sonde matérielle indisponible (code " + codeSonde + ")");
        }
        CalibrationFfi.Resultat couts = CalibrationFfi.mesurer(contexte.pont(), handle);
        contexte.pont().hwSetFfiCosts(handle, couts.jniCallNs(), couts.ffiBatchNsPerKb());
        journal.add("CONFIGURE : coût FFI " + couts.jniCallNs() + " ns/appel, "
                + couts.ffiBatchNsPerKb() + " ns/Kio, mesuré en " + couts.dureeMs() + " ms");

        // 9. Vérifier l'intégrité des caches. Aucun cache n'existe à ce jalon : C-09,
        // C-32 et C-48 arrivent à M2. L'étape est donc sans objet, et le signaler vaut
        // mieux que de laisser croire à une vérification qui n'a pas lieu.
        journal.add("CONFIGURE : aucun cache à vérifier à ce stade");

        // 10. Prêt.
        return rapport(Etat.READY, null,
                "Runtime natif prêt (" + bibliotheque.getFileName() + ").",
                handle, bibliotheque, debut, journal);
    }

    /**
     * Réinitialise le garde-fou de double initialisation.
     *
     * <p>Réservé aux tests, qui exécutent plusieurs séquences dans une même JVM. Le
     * code de production ne démarre qu'une fois par processus.
     */
    static void reinitialiserPourTests() {
        DEMARRE.set(false);
    }

    private static Rapport rapport(
            Etat etat,
            CodeErreur code,
            String message,
            long handle,
            Path bibliotheque,
            long debut,
            List<String> journal) {

        long dureeMs = (System.nanoTime() - debut) / 1_000_000L;
        List<String> trace = new ArrayList<>(journal);
        trace.add(etat + " : " + message);
        return new Rapport(etat, code, message, handle, bibliotheque, dureeMs, List.copyOf(trace));
    }
}
