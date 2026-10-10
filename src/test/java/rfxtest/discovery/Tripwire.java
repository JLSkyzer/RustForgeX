package rfxtest.discovery;

/**
 * Fil-piège du test T-441 (R-620) : lève un drapeau quand une sentinelle de ce paquet
 * est initialisée.
 *
 * <p>Un mod synthétique déclare ce paquet. Si la découverte des mods chargeait et
 * initialisait les classes des paquets qu'elle inventorie, l'initialisation statique
 * d'une sentinelle passerait par ici.
 */
public final class Tripwire {

    private static volatile boolean tripped;

    private Tripwire() {
    }

    /** Appelé par l'initialisation statique d'une sentinelle. */
    static void trip() {
        tripped = true;
    }

    /** @return {@code true} si une sentinelle a été initialisée */
    public static boolean tripped() {
        return tripped;
    }
}
