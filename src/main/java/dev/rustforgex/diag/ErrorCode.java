package dev.rustforgex.diag;

/**
 * Codes d'erreur normatifs du côté Java.
 *
 * <p>Composant : C-35 (Diagnostics). Référence : cahier des charges, annexe A.2.
 * Obligation : contrat agent 4.7 — tout nouveau code d'erreur est enregistré dans
 * l'annexe A.2 avant d'être utilisé. Maturité : {@code STABLE}.
 *
 * <p>Seuls les codes que le code Java peut réellement produire sont déclarés ici. Les
 * codes produits par le runtime natif traversent la frontière sous forme d'entiers
 * négatifs et sont interprétés par {@link #fromNativeCode(int)}.
 */
public enum ErrorCode {

    /** {@code E-1001} : version de Forge hors plage supportée. Majeure, mène à OBSERVE_ONLY. */
    FORGE_OUT_OF_RANGE(1001, Severity.MAJOR, "version de Forge hors plage supportée"),

    /** {@code E-1002} : version d'ABI incompatible. Critique, mène à DISABLED. */
    ABI_INCOMPATIBLE(1002, Severity.CRITICAL, "version d'ABI incompatible"),

    /** {@code E-1003} : hash du binaire natif invalide. Critique, mène à DISABLED. */
    INVALID_NATIVE_DIGEST(1003, Severity.CRITICAL, "hash du binaire natif invalide"),

    /** {@code E-1004} : double initialisation du runtime. Majeure, la seconde est refusée. */
    DOUBLE_INIT(1004, Severity.MAJOR, "double initialisation du runtime"),

    /** {@code E-1005} : binaire natif absent pour la plateforme. Majeure, mène à DEGRADED. */
    NATIVE_MISSING(1005, Severity.MAJOR, "binaire natif absent pour la plateforme"),

    /** {@code E-1006} : échec de chargement de la bibliothèque. Majeure, mène à DEGRADED. */
    LOAD_FAILED(1006, Severity.MAJOR, "échec de chargement de la bibliothèque"),

    /** {@code E-3001} : panic Rust capturée à la frontière FFI. Majeure. */
    PANIC_CAUGHT(3001, Severity.MAJOR, "panic Rust capturée à la frontière FFI"),

    /** {@code E-3004} : invariant violé. Critique, mène à HALT avec dump. */
    INVARIANT_VIOLATED(3004, Severity.CRITICAL, "invariant violé");

    /** Sévérité d'un code, telle que définie en annexe A.2. */
    public enum Severity {
        /** Dégradation locale, le système continue. */
        MINOR,
        /** Fonctionnalité perdue ou sous-système désactivé. */
        MAJOR,
        /** Le runtime refuse de s'activer ou s'arrête. */
        CRITICAL
    }

    private final int number;
    private final Severity severity;
    private final String description;

    ErrorCode(int number, Severity severity, String description) {
        this.number = number;
        this.severity = severity;
        this.description = description;
    }

    /** @return le numéro normatif, sans le préfixe {@code E-}. */
    public int number() {
        return number;
    }

    /** @return la sévérité définie en annexe A.2. */
    public Severity severity() {
        return severity;
    }

    /** @return la description courte, identique au libellé de l'annexe A.2. */
    public String description() {
        return description;
    }

    /** @return l'identifiant normatif complet, par exemple {@code E-1003}. */
    public String id() {
        return "E-" + number;
    }

    /**
     * Interprète une valeur de retour de la frontière FFI.
     *
     * <p>Convention R-701 : {@code 0} signifie succès, une valeur négative porte le
     * code d'erreur. Un code inconnu de cette énumération renvoie {@code null} : il
     * doit alors être journalisé tel quel plutôt que d'être assimilé à un autre code.
     *
     * @param nativeCode valeur renvoyée par une fonction de l'ABI
     * @return le code correspondant, ou {@code null} si la valeur est un succès ou un
     *     code non déclaré ici
     */
    public static ErrorCode fromNativeCode(int nativeCode) {
        if (nativeCode >= 0) {
            return null;
        }
        int wanted = -nativeCode;
        for (ErrorCode c : values()) {
            if (c.number == wanted) {
                return c;
            }
        }
        return null;
    }

    @Override
    public String toString() {
        return id() + " (" + description + ")";
    }
}
