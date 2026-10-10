package rfxtest.discovery;

/**
 * Sentinelle du test T-441 : son initialisation statique déclenche {@link Tripwire}.
 * Aucun test ne l'utilise directement ; seule une découverte fautive la chargerait.
 */
public final class Sentinel {

    static {
        Tripwire.trip();
    }

    private Sentinel() {
    }
}
