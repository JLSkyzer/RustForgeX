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
 * négatifs et sont interprétés par {@link #depuisCodeNatif(int)}.
 */
public enum CodeErreur {

    /** {@code E-1001} : version de Forge hors plage supportée. Majeure, mène à OBSERVE_ONLY. */
    FORGE_HORS_PLAGE(1001, Severite.MAJEURE, "version de Forge hors plage supportée"),

    /** {@code E-1002} : version d'ABI incompatible. Critique, mène à DISABLED. */
    ABI_INCOMPATIBLE(1002, Severite.CRITIQUE, "version d'ABI incompatible"),

    /** {@code E-1003} : hash du binaire natif invalide. Critique, mène à DISABLED. */
    HASH_NATIF_INVALIDE(1003, Severite.CRITIQUE, "hash du binaire natif invalide"),

    /** {@code E-1004} : double initialisation du runtime. Majeure, la seconde est refusée. */
    DOUBLE_INIT(1004, Severite.MAJEURE, "double initialisation du runtime"),

    /** {@code E-1005} : binaire natif absent pour la plateforme. Majeure, mène à DEGRADED. */
    NATIF_ABSENT(1005, Severite.MAJEURE, "binaire natif absent pour la plateforme"),

    /** {@code E-1006} : échec de chargement de la bibliothèque. Majeure, mène à DEGRADED. */
    CHARGEMENT_ECHOUE(1006, Severite.MAJEURE, "échec de chargement de la bibliothèque"),

    /** {@code E-3001} : panic Rust capturée à la frontière FFI. Majeure. */
    PANIC_CAPTUREE(3001, Severite.MAJEURE, "panic Rust capturée à la frontière FFI"),

    /** {@code E-3004} : invariant violé. Critique, mène à HALT avec dump. */
    INVARIANT_VIOLE(3004, Severite.CRITIQUE, "invariant violé");

    /** Sévérité d'un code, telle que définie en annexe A.2. */
    public enum Severite {
        /** Dégradation locale, le système continue. */
        MINEURE,
        /** Fonctionnalité perdue ou sous-système désactivé. */
        MAJEURE,
        /** Le runtime refuse de s'activer ou s'arrête. */
        CRITIQUE
    }

    private final int numero;
    private final Severite severite;
    private final String description;

    CodeErreur(int numero, Severite severite, String description) {
        this.numero = numero;
        this.severite = severite;
        this.description = description;
    }

    /** @return le numéro normatif, sans le préfixe {@code E-}. */
    public int numero() {
        return numero;
    }

    /** @return la sévérité définie en annexe A.2. */
    public Severite severite() {
        return severite;
    }

    /** @return la description courte, identique au libellé de l'annexe A.2. */
    public String description() {
        return description;
    }

    /** @return l'identifiant normatif complet, par exemple {@code E-1003}. */
    public String identifiant() {
        return "E-" + numero;
    }

    /**
     * Interprète une valeur de retour de la frontière FFI.
     *
     * <p>Convention R-701 : {@code 0} signifie succès, une valeur négative porte le
     * code d'erreur. Un code inconnu de cette énumération renvoie {@code null} : il
     * doit alors être journalisé tel quel plutôt que d'être assimilé à un autre code.
     *
     * @param codeNatif valeur renvoyée par une fonction de l'ABI
     * @return le code correspondant, ou {@code null} si la valeur est un succès ou un
     *     code non déclaré ici
     */
    public static CodeErreur depuisCodeNatif(int codeNatif) {
        if (codeNatif >= 0) {
            return null;
        }
        int numero = -codeNatif;
        for (CodeErreur c : values()) {
            if (c.numero == numero) {
                return c;
            }
        }
        return null;
    }

    @Override
    public String toString() {
        return identifiant() + " (" + description + ")";
    }
}
