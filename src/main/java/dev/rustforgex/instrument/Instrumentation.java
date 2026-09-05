package dev.rustforgex.instrument;

import dev.rustforgex.bridge.NativeBridge;

/**
 * C-04, côté jeu : armement du transformateur et suivi de son activité (ADR-017).
 *
 * <p>Cahier des charges : PARTIE 5.4. Métriques : {@code rfx.instr.classes_seen},
 * {@code rfx.instr.methods_probed}, {@code rfx.instr.transform_failures}.
 * Maturité : {@code STABLE}.
 *
 * <p>Le plugin de lancement vit dans un second JAR. S'il manque, le mod démarre,
 * mesure la machine, répond à {@code /rfx status} — et ne sonde rien. C'est le pire
 * des symptômes, parce qu'il ressemble à un fonctionnement normal : cette classe
 * <strong>détecte l'absence et la dit</strong>, au lieu de la subir en silence.
 */
public final class Instrumentation {

    /** État de l'instrumentation, tel que rapporté à l'utilisateur. */
    public enum State {
        /** Le transformateur est armé : les classes chargées ensuite sont sondées. */
        ARMED,
        /** Le JAR du transformateur est absent : aucune sonde ne sera posée. */
        PLUGIN_MISSING,
        /** Le JAR est présent mais ModLauncher n'a jamais enregistré le
         * transformateur : aucune classe ne lui est soumise. */
        PLUGIN_NOT_INSTALLED,
        /** Le runtime natif n'est pas actif : il n'y a personne pour attribuer les
         * identifiants de sonde. */
        RUNTIME_INACTIVE
    }

    private final State state;
    private final ProbeRegistry registry;

    private Instrumentation(State state, ProbeRegistry registry) {
        this.state = state;
        this.registry = registry;
    }

    /**
     * Arme le transformateur si tout est en place.
     *
     * @param bridge pont vers le runtime natif, ou {@code null} s'il n'est pas actif
     * @param handle handle du runtime
     * @param clientSide {@code true} côté client
     * @return l'état obtenu, jamais {@code null}
     */
    public static Instrumentation arm(NativeBridge bridge, long handle, boolean clientSide) {
        if (bridge == null) {
            return new Instrumentation(State.RUNTIME_INACTIVE, null);
        }
        try {
            ProbeRegistry registry = InstrumentationLink.arm(bridge, handle, clientSide);
            if (registry == null) {
                return new Instrumentation(State.PLUGIN_NOT_INSTALLED, null);
            }
            return new Instrumentation(State.ARMED, registry);
        } catch (NoClassDefFoundError e) {
            // Le JAR de lancement n'est pas installé. Ce n'est pas une panne : le mod
            // fonctionne, sans instrumentation. Le dire est tout ce qu'il y a à faire.
            return new Instrumentation(State.PLUGIN_MISSING, null);
        }
    }

    /** Désarme le transformateur, sans échouer s'il n'a jamais été armé. */
    public void disarm() {
        if (state != State.ARMED) {
            return;
        }
        try {
            InstrumentationLink.disarm();
        } catch (NoClassDefFoundError e) {
            // Impossible en pratique — l'armement a réussi, donc la classe est là —
            // mais l'arrêt du mod ne doit jamais échouer sur ce chemin.
        }
    }

    /** @return l'état de l'instrumentation */
    public State state() {
        return state;
    }

    /** @return {@code true} si le transformateur est armé */
    public boolean armed() {
        return state == State.ARMED;
    }

    /** @return le nombre de classes vues passer, ou {@code 0} si non armé */
    public long classesSeen() {
        return armed() ? InstrumentationLink.classesSeen() : 0L;
    }

    /** @return le nombre de méthodes sondées, ou {@code 0} si non armé */
    public long methodsProbed() {
        return armed() ? InstrumentationLink.methodsProbed() : 0L;
    }

    /** @return le nombre d'échecs de transformation, ou {@code 0} si non armé */
    public long transformFailures() {
        return armed() ? InstrumentationLink.transformFailures() : 0L;
    }

    /** @return le registre d'identifiants, ou {@code null} si non armé */
    public ProbeRegistry registry() {
        return registry;
    }

    /**
     * Photographie des compteurs, pour l'affichage.
     *
     * <p>Les compteurs vivent dans des variables statiques du transformateur, qui n'est
     * pas joignable depuis un test unitaire. Les figer dans une valeur immuable permet
     * de vérifier ce que {@code /rfx status} affiche pour <strong>chaque</strong> état,
     * sans jamais avoir à manipuler cet état statique.
     *
     * @return l'état et les compteurs à cet instant
     */
    public View view() {
        return new View(state, classesSeen(), methodsProbed(), transformFailures());
    }

    /**
     * État de l'instrumentation à un instant donné.
     *
     * @param state état du transformateur
     * @param classesSeen classes passées par le transformateur
     * @param methodsProbed méthodes effectivement sondées
     * @param transformFailures transformations abandonnées (FM-09)
     */
    public record View(State state, long classesSeen, long methodsProbed,
            long transformFailures) {

        /** @return {@code true} si le transformateur est armé */
        public boolean armed() {
            return state == State.ARMED;
        }
    }
}
