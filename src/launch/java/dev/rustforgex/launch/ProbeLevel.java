package dev.rustforgex.launch;

/**
 * Niveau de sondage d'une méthode (C-04).
 *
 * <p>Cahier des charges : PARTIE 5.4. Maturité : {@code STABLE}.
 *
 * <p>Le niveau ne change <strong>pas</strong> le bytecode injecté : il est consulté à
 * l'exécution par {@code RfxProbes}. Voir ADR-016 — un seul bytecode signifie une
 * seule sémantique à préserver (R-310), et un changement de niveau n'exige alors
 * aucune retransformation.
 */
public enum ProbeLevel {

    /** Méthode non sondée. */
    OFF,

    /** Compteur d'appels seul. Coût cible : moins de 5 ns par appel. */
    COUNTER,

    /** Compteur et durée. Coût cible : moins de 40 ns par appel. */
    TIMED,

    /** Durée, contexte et allocation approchée. Coût cible : moins de 150 ns. */
    DEEP;

    /** @return {@code true} si ce niveau demande de mesurer une durée */
    public boolean measuresDuration() {
        return this == TIMED || this == DEEP;
    }

    /** @return le code transmis au runtime natif */
    public byte code() {
        return (byte) ordinal();
    }
}
