package dev.rustforgex.command;

import dev.rustforgex.bridge.NativeBridge;
import dev.rustforgex.diag.ErrorCode;
import dev.rustforgex.diag.Incident;
import net.minecraft.network.chat.Component;

import java.util.List;
import java.util.Map;
import java.util.function.Consumer;
import java.util.function.Supplier;

/**
 * C-38 : {@code /rfx panic-test}, vérification en jeu du confinement des panics.
 *
 * <p>Cahier des charges : PARTIE 5.36 (« DEBUG uniquement, vérifie le confinement »),
 * PARTIE 5.27, R-522, R-523. Point d'entrée natif : {@code rfx_panic_test} (ADR-015).
 * Tests : T-420, T-360 côté natif. Maturité : {@code STABLE}.
 *
 * <p>Le natif panique délibérément ; la panic doit être capturée à la frontière et
 * revenir en {@code E-3001}, sans que la JVM tombe. Le test va jusqu'au bout de la
 * chaîne : la panic est comptée par le runtime, et elle est consignée comme incident
 * dans {@code crash/} (T-411) — c'est le seul moyen de voir un dump sans attendre une
 * vraie panne.
 *
 * <p>Hors du mode {@code debug}, rien n'est appelé : une panic, même confinée, est
 * comptée, et dix en soixante secondes font passer le runtime en {@code HALTED}.
 */
public final class ContainmentCheck {

    /** Préfixe des clés de traduction. */
    static final String KEY_PREFIX = "rustforgex.panic_test.";

    /** Mode de configuration seul autorisé à déclencher la panic. */
    static final String DEBUG_MODE = "debug";

    private ContainmentCheck() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /** Ce que le test a constaté. */
    public enum Verdict {
        /** Runtime natif absent : rien à vérifier. */
        UNAVAILABLE,
        /** Mode autre que {@code debug} : rien n'a été appelé. */
        REFUSED,
        /** La panic est revenue en {@code E-3001} : le confinement tient. */
        CONFINED,
        /** Le natif a rendu autre chose que {@code E-3001}. */
        NOT_CONFINED
    }

    /**
     * Résultat d'un test.
     *
     * @param verdict ce qui a été constaté
     * @param mode mode de configuration au moment du test
     * @param code valeur rendue par le natif, ou {@code 0} s'il n'a pas été appelé
     * @param state état du runtime après le test, ou {@code null} s'il est inconnu
     * @param panics panics comptées par le runtime après le test, ou {@code -1}
     */
    public record Outcome(Verdict verdict, String mode, int code, String state, long panics) {
    }

    /**
     * Exécute le test.
     *
     * @param bridge pont natif, ou {@code null} si le runtime n'est pas actif
     * @param handle handle du runtime
     * @param mode mode de configuration ({@code general.mode})
     * @param status relève du statut natif décodé, après le test ; peut rendre {@code null}
     * @param incidents destinataire de l'incident, si la panic est confinée
     * @return le résultat, jamais {@code null}
     */
    public static Outcome run(NativeBridge bridge, long handle, String mode,
            Supplier<Map<String, Object>> status, Consumer<Incident> incidents) {
        if (bridge == null) {
            return new Outcome(Verdict.UNAVAILABLE, mode, 0, null, -1);
        }
        if (!DEBUG_MODE.equals(mode)) {
            return new Outcome(Verdict.REFUSED, mode, 0, null, -1);
        }
        int code = bridge.panicTest(handle);
        Map<String, Object> after = status.get();
        String state = after == null ? null : String.valueOf(after.get("state"));
        long panics = after != null && after.get("panics") instanceof Long n ? n : -1;
        if (ErrorCode.fromNativeCode(code) != ErrorCode.PANIC_CAUGHT) {
            return new Outcome(Verdict.NOT_CONFINED, mode, code, state, panics);
        }
        incidents.accept(Incident.nativePanic("panic-test"));
        return new Outcome(Verdict.CONFINED, mode, code, state, panics);
    }

    /**
     * Met le résultat en forme.
     *
     * @param outcome le résultat
     * @return les lignes à afficher
     */
    public static List<Component> lines(Outcome outcome) {
        return switch (outcome.verdict()) {
            case UNAVAILABLE -> List.of(Component.translatable(KEY_PREFIX + "unavailable"));
            case REFUSED -> List.of(Component.translatable(KEY_PREFIX + "refused",
                    Component.literal(String.valueOf(outcome.mode()))));
            case CONFINED -> List.of(Component.translatable(KEY_PREFIX + "confined",
                    Component.literal(String.valueOf(outcome.state())),
                    Component.literal(Long.toString(outcome.panics()))));
            case NOT_CONFINED -> List.of(Component.translatable(KEY_PREFIX + "not_confined",
                    Component.literal(Integer.toString(outcome.code()))));
        };
    }
}
