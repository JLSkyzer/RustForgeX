package dev.rustforgex.bootstrap;

import dev.rustforgex.bridge.NativeBridge;

import java.nio.ByteBuffer;

/**
 * C-45, partie Java : mesure du coût de franchissement de la frontière.
 *
 * <p>Cahier des charges : PARTIE 5.43. Exigence : R-660 — ces mesures alimentent le
 * modèle de coût de C-15, et aucune constante de coût codée en dur ne doit être
 * utilisée en production. Tests : T-480, T-482. Maturité : {@code STABLE}.
 *
 * <p>Ces deux coûts ne peuvent pas être mesurés depuis le natif : ils incluent le
 * trajet aller-retour complet depuis la JVM, avec la transition JNI. Ils sont donc
 * mesurés ici, puis publiés dans la classe matérielle par
 * {@link NativeBridge#hwSetFfiCosts(long, int, int)}.
 *
 * <p>Une mesure qui n'aboutit pas rend {@code 0}, ce qui signifie « non mesuré » et
 * laisse la couverture de sonde à {@code false} côté natif. Aucune valeur n'est
 * substituée à une mesure manquante (contrat agent 6.1).
 */
public final class FfiCalibration {

    /** Appels mesurés pour le coût d'un aller-retour, conformément à la PARTIE 5.43. */
    public static final int MEASURED_CALLS = 10_000;

    /** Appels de chauffe, pour que le JIT ait compilé le chemin avant la mesure. */
    private static final int WARMUP_CALLS = 2_000;

    /** Tailles de transfert mesurées, en octets (PARTIE 5.43). */
    private static final int[] TRANSFER_SIZES = {1024, 64 * 1024, 1024 * 1024};

    /**
     * Résultat de la calibration.
     *
     * @param jniCallNs coût moyen d'un aller-retour FFI minimal, en nanosecondes,
     *     ou {@code 0} si la mesure a échoué
     * @param ffiBatchNsPerKb coût de transfert Java vers natif, en nanosecondes par
     *     kibioctet, ou {@code 0} si la mesure a échoué
     * @param durationMs durée totale de la calibration, en millisecondes
     */
    public record Costs(int jniCallNs, int ffiBatchNsPerKb, long durationMs) {

        /** @return {@code true} si les deux coûts ont pu être mesurés */
        public boolean complete() {
            return jniCallNs > 0 && ffiBatchNsPerKb > 0;
        }
    }

    private FfiCalibration() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Mesure les coûts de franchissement de la frontière.
     *
     * @param bridge pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @return les coûts mesurés
     */
    public static Costs measure(NativeBridge bridge, long handle) {
        long start = System.nanoTime();
        int callCost = measureCallCost(bridge, handle);
        int transferCost = measureTransferCost(bridge, handle);
        long durationMs = (System.nanoTime() - start) / 1_000_000L;
        return new Costs(callCost, transferCost, durationMs);
    }

    /** Coût moyen d'un aller-retour minimal, en nanosecondes. */
    private static int measureCallCost(NativeBridge bridge, long handle) {
        for (int i = 0; i < WARMUP_CALLS; i++) {
            if (bridge.noop(handle) != 0) {
                return 0;
            }
        }

        long start = System.nanoTime();
        for (int i = 0; i < MEASURED_CALLS; i++) {
            if (bridge.noop(handle) != 0) {
                return 0;
            }
        }
        long elapsed = System.nanoTime() - start;

        long average = elapsed / MEASURED_CALLS;
        // Une moyenne nulle signifierait que l'horloge n'a pas la résolution
        // nécessaire : mieux vaut déclarer la mesure absente que publier un coût nul,
        // qui serait interprété comme un appel gratuit par le modèle de coût.
        return average <= 0 ? 0 : (int) Math.min(average, Integer.MAX_VALUE);
    }

    /** Coût de transfert Java vers natif, en nanosecondes par kibioctet. */
    private static int measureTransferCost(NativeBridge bridge, long handle) {
        long totalNs = 0;
        long totalKib = 0;

        for (int size : TRANSFER_SIZES) {
            ByteBuffer buffer = ByteBuffer.allocateDirect(size);
            for (int i = 0; i < size; i++) {
                buffer.put(i, (byte) i);
            }

            // Une passe de chauffe, puis la mesure.
            if (bridge.transferProbe(handle, buffer, size) < 0) {
                return 0;
            }
            long start = System.nanoTime();
            long witness = bridge.transferProbe(handle, buffer, size);
            long elapsed = System.nanoTime() - start;
            if (witness < 0) {
                return 0;
            }

            totalNs += elapsed;
            totalKib += size / 1024L;
        }

        if (totalKib <= 0) {
            return 0;
        }
        long perKib = totalNs / totalKib;
        return perKib <= 0 ? 0 : (int) Math.min(perKib, Integer.MAX_VALUE);
    }
}
