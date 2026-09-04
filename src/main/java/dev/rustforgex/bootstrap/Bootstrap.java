package dev.rustforgex.bootstrap;

import dev.rustforgex.bootstrap.NativeLoader.LoadFailure;
import dev.rustforgex.bootstrap.NativeLoader.Platform;
import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.bridge.RfxNative;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.ErrorCode;

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
 * {@link #start}. Tout échec d'une étape est journalisé et conduit à
 * {@code DEGRADED} ou {@code DISABLED}, états dans lesquels le jeu tourne exactement
 * comme sans le mod : aucune instrumentation, aucun thread supplémentaire.
 */
public final class Bootstrap {

    /** États de la séquence d'initialisation (SM du composant C-02). */
    public enum State {
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
     * @param state état final atteint
     * @param code code de l'annexe A.2 expliquant un état non nominal, ou {@code null}
     * @param message message destiné aux journaux, jamais {@code null}
     * @param handle handle du runtime natif si {@code state == READY}, sinon {@code 0}
     * @param library chemin du binaire chargé, ou {@code null}
     * @param durationMs durée totale du démarrage, en millisecondes
     * @param journal trace des étapes, dans l'ordre
     */
    public record Report(
            State state,
            ErrorCode code,
            String message,
            long handle,
            Path library,
            long durationMs,
            List<String> journal) {

        /** @return {@code true} si le runtime natif est utilisable */
        public boolean ready() {
            return state == State.READY;
        }
    }

    /**
     * Dépendances du démarrage, injectables pour les tests.
     *
     * @param root racine de travail, typiquement {@code <gameDir>/rustforgex}
     * @param configuration configuration déjà chargée et validée (C-37)
     * @param clientSide {@code true} côté client, {@code false} côté serveur dédié
     * @param platform plateforme cible, vide si l'hôte n'est pas supporté
     * @param nativeLoader chargeur de bibliothèque native (C-03)
     * @param libraryLoader action de chargement effectif, isolée pour les tests
     * @param bridge pont vers le runtime natif (IF-01)
     */
    public record Context(
            Path root,
            Configuration configuration,
            boolean clientSide,
            Optional<Platform> platform,
            NativeLoader nativeLoader,
            LibraryLoader libraryLoader,
            NativeBridge bridge) {

        /** Action de chargement d'une bibliothèque déjà extraite et vérifiée. */
        @FunctionalInterface
        public interface LibraryLoader {

            /**
             * @param library chemin du binaire vérifié
             * @throws LoadFailure si le système refuse le chargement
             */
            void load(Path library) throws LoadFailure;
        }

        /**
         * Contexte de production : ressources du JAR, chargement réel, pont JNI.
         *
         * @param root racine de travail
         * @param configuration configuration chargée
         * @param clientSide côté d'exécution
         * @return le contexte à passer à {@link Bootstrap#start(Context)}
         */
        public static Context real(Path root, Configuration configuration, boolean clientSide) {
            return new Context(
                    root,
                    configuration,
                    clientSide,
                    NativeLoader.hostPlatform(),
                    new NativeLoader(),
                    NativeLoader::load,
                    NativeBridge.real());
        }
    }

    /**
     * Empêche une seconde initialisation dans le même processus (T-114).
     *
     * <p>Le runtime natif refuse déjà la double initialisation avec {@code E-1004}
     * (R-520). Ce garde-fou côté Java évite en plus de recharger la bibliothèque et de
     * relancer une sonde, opérations inutiles et coûteuses.
     */
    private static final AtomicBoolean STARTED = new AtomicBoolean(false);

    private Bootstrap() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Exécute la séquence d'initialisation.
     *
     * @param context dépendances du démarrage
     * @return le rapport final, jamais {@code null}
     */
    public static Report start(Context context) {
        long begin = System.nanoTime();
        List<String> journal = new ArrayList<>();

        if (!STARTED.compareAndSet(false, true)) {
            return report(State.DISABLED, ErrorCode.DOUBLE_INIT,
                    "Le runtime a déjà été initialisé dans ce processus.",
                    0, null, begin, journal);
        }

        try {
            return sequence(context, journal, begin);
        } catch (RuntimeException | LinkageError e) {
            // Filet de sécurité : aucune exception ne doit remonter jusqu'à Forge, sous
            // peine d'empêcher le jeu de démarrer (PARTIE 5.2, dernière ligne de
            // l'algorithme normatif).
            return report(State.DEGRADED, ErrorCode.LOAD_FAILED,
                    "Erreur inattendue au démarrage : " + e,
                    0, null, begin, journal);
        }
    }

    /** Séquence normative en dix étapes (PARTIE 5.2). */
    private static Report sequence(Context context, List<String> journal, long begin) {

        // 1 et 2. La configuration est déjà lue (C-37) ; reste à l'honorer.
        journal.add("INIT : configuration chargée, mode " + context.configuration().mode());
        if (!context.configuration().enabledOn(context.clientSide())) {
            return report(State.DISABLED, null,
                    "Désactivé par configuration sur ce côté : le jeu tourne sans RUSTFORGE-X.",
                    0, null, begin, journal);
        }

        // 3. Détecter la plateforme et sélectionner le natif.
        journal.add("PROBE : détection de la plateforme");
        Optional<Platform> platform = context.platform();
        if (platform.isEmpty()) {
            return report(State.DEGRADED, ErrorCode.NATIVE_MISSING,
                    "Plateforme non supportée (" + System.getProperty("os.name") + " / "
                            + System.getProperty("os.arch")
                            + ") : aucun binaire natif ne correspond. Le jeu tourne en Java pur.",
                    0, null, begin, journal);
        }

        // 4 et 5. Extraire puis charger la bibliothèque (C-03).
        journal.add("LOAD_NATIVE : extraction pour " + platform.get().directory());
        Path library;
        try {
            library = context.nativeLoader().prepare(platform.get(), context.root());
            context.libraryLoader().load(library);
        } catch (LoadFailure e) {
            // FM-04, FM-05, FM-08 : un hash invalide est une altération du binaire et
            // interdit toute activation ; une absence ou un refus système dégradent.
            State state =
                    e.code() == ErrorCode.INVALID_NATIVE_DIGEST ? State.DISABLED : State.DEGRADED;
            return report(state, e.code(), e.getMessage(), 0, null, begin, journal);
        }

        // 6. Handshake ABI : aucune autre fonction n'est appelée avant (R-702).
        journal.add("HANDSHAKE : vérification de l'ABI");
        int abi = context.bridge().abiVersion();
        if (abi != RfxNative.EXPECTED_ABI) {
            return report(State.DISABLED, ErrorCode.ABI_INCOMPATIBLE,
                    "Le binaire natif annonce l'ABI " + abi + ", cette version attend l'ABI "
                            + RfxNative.EXPECTED_ABI + ". Aucun appel natif ne sera émis.",
                    0, library, begin, journal);
        }

        // 7. Initialiser le runtime natif.
        journal.add("CONFIGURE : initialisation du runtime natif");
        long handle = context.bridge().init(context.configuration().toNativeCbor());
        if (handle <= 0) {
            ErrorCode code = ErrorCode.fromNativeCode((int) handle);
            return report(State.DEGRADED,
                    code != null ? code : ErrorCode.LOAD_FAILED,
                    "Initialisation du runtime natif refusée (code " + handle + ").",
                    0, library, begin, journal);
        }

        // 8. Sonder le matériel puis calibrer le coût de la frontière (C-45).
        int probeCode = context.bridge().hwProbe(handle);
        if (probeCode != 0) {
            journal.add("CONFIGURE : sonde matérielle indisponible (code " + probeCode + ")");
        }
        FfiCalibration.Costs costs = FfiCalibration.measure(context.bridge(), handle);
        context.bridge().hwSetFfiCosts(handle, costs.jniCallNs(), costs.ffiBatchNsPerKb());
        journal.add("CONFIGURE : coût FFI " + costs.jniCallNs() + " ns/appel, "
                + costs.ffiBatchNsPerKb() + " ns/Kio, mesuré en " + costs.durationMs() + " ms");

        // 9. Vérifier l'intégrité des caches. Aucun cache n'existe à ce jalon : C-09,
        // C-32 et C-48 arrivent à M2. L'étape est donc sans objet, et le signaler vaut
        // mieux que de laisser croire à une vérification qui n'a pas lieu.
        journal.add("CONFIGURE : aucun cache à vérifier à ce stade");

        // 10. Prêt.
        return report(State.READY, null,
                "Runtime natif prêt (" + library.getFileName() + ").",
                handle, library, begin, journal);
    }

    /**
     * Réinitialise le garde-fou de double initialisation.
     *
     * <p>Réservé aux tests, qui exécutent plusieurs séquences dans une même JVM. Le
     * code de production ne démarre qu'une fois par processus.
     */
    static void resetForTests() {
        STARTED.set(false);
    }

    private static Report report(
            State state,
            ErrorCode code,
            String message,
            long handle,
            Path library,
            long begin,
            List<String> journal) {

        long durationMs = (System.nanoTime() - begin) / 1_000_000L;
        List<String> trace = new ArrayList<>(journal);
        trace.add(state + " : " + message);
        return new Report(state, code, message, handle, library, durationMs, List.copyOf(trace));
    }
}
