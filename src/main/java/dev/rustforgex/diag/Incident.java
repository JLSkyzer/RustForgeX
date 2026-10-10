package dev.rustforgex.diag;

import java.time.Instant;
import java.util.Objects;

/**
 * C-35 : un incident de RUSTFORGE-X, tel qu'il sera consigné dans {@code crash/}.
 *
 * <p>Cahier des charges : PARTIE 5.33, PARTIE 19.6. Test : T-411. Maturité :
 * {@code STABLE}.
 *
 * <p>Seuls les incidents qui existent à ce jalon ont un genre : une accroche désactivée
 * après trop d'échecs (FM-02) et une panic native capturée à la frontière (E-3001).
 * Les incidents d'unité de travail — rollback, divergence — n'existent pas tant
 * qu'aucune unité n'est déportée (ADR-033).
 *
 * @param kind genre de l'incident
 * @param subject ce qui a échoué : nom de l'accroche, point d'entrée natif
 * @param occurrences nombre d'échecs observés au moment de l'incident
 * @param cause exception d'origine, ou {@code null} si l'échec vient du natif
 * @param code code d'erreur reçu du natif, ou {@code null}
 * @param at instant de l'incident
 */
public record Incident(
        Kind kind, String subject, int occurrences, Throwable cause, ErrorCode code, Instant at) {

    /** Genres d'incident consignés. */
    public enum Kind {
        /** Une accroche Forge désactivée après {@code HookGuard.DISABLE_THRESHOLD} échecs. */
        HOOK_DISABLED,
        /** Une panic Rust capturée à la frontière FFI, rapportée par son code. */
        NATIVE_PANIC
    }

    /** Valide les champs obligatoires. */
    public Incident {
        Objects.requireNonNull(kind, "kind");
        Objects.requireNonNull(subject, "subject");
        Objects.requireNonNull(at, "at");
    }

    /**
     * Accroche désactivée (FM-02).
     *
     * @param hook nom de l'accroche
     * @param failures échecs observés
     * @param last dernière exception levée
     * @return l'incident, daté de maintenant
     */
    public static Incident hookDisabled(String hook, int failures, Throwable last) {
        return new Incident(Kind.HOOK_DISABLED, hook, failures, last, null, Instant.now());
    }

    /**
     * Panic native capturée (E-3001).
     *
     * @param entry point d'entrée natif qui l'a rapportée
     * @return l'incident, daté de maintenant
     */
    public static Incident nativePanic(String entry) {
        return new Incident(
                Kind.NATIVE_PANIC, entry, 1, null, ErrorCode.PANIC_CAUGHT, Instant.now());
    }

    /** @return la clé de dédoublonnage : un même échec n'est consigné qu'une fois */
    public String key() {
        return kind + "/" + subject;
    }
}
