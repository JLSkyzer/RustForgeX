package dev.rustforgex.bootstrap;

import dev.rustforgex.bridge.PontNatif;

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
 * {@link PontNatif#hwSetFfiCosts(long, int, int)}.
 *
 * <p>Une mesure qui n'aboutit pas rend {@code 0}, ce qui signifie « non mesuré » et
 * laisse la couverture de sonde à {@code false} côté natif. Aucune valeur n'est
 * substituée à une mesure manquante (contrat agent 6.1).
 */
public final class CalibrationFfi {

    /** Appels mesurés pour le coût d'un aller-retour, conformément à la PARTIE 5.43. */
    public static final int APPELS_MESURES = 10_000;

    /** Appels de chauffe, pour que le JIT ait compilé le chemin avant la mesure. */
    private static final int APPELS_CHAUFFE = 2_000;

    /** Tailles de transfert mesurées, en octets (PARTIE 5.43). */
    private static final int[] TAILLES_TRANSFERT = {1024, 64 * 1024, 1024 * 1024};

    /**
     * Résultat de la calibration.
     *
     * @param jniCallNs coût moyen d'un aller-retour FFI minimal, en nanosecondes,
     *     ou {@code 0} si la mesure a échoué
     * @param ffiBatchNsPerKb coût de transfert Java vers natif, en nanosecondes par
     *     kibioctet, ou {@code 0} si la mesure a échoué
     * @param dureeMs durée totale de la calibration, en millisecondes
     */
    public record Resultat(int jniCallNs, int ffiBatchNsPerKb, long dureeMs) {

        /** @return {@code true} si les deux coûts ont pu être mesurés */
        public boolean complet() {
            return jniCallNs > 0 && ffiBatchNsPerKb > 0;
        }
    }

    private CalibrationFfi() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Mesure les coûts de franchissement de la frontière.
     *
     * @param pont pont vers le runtime natif
     * @param handle handle du runtime, déjà initialisé
     * @return les coûts mesurés
     */
    public static Resultat mesurer(PontNatif pont, long handle) {
        long debut = System.nanoTime();
        int coutAppel = mesurerCoutAppel(pont, handle);
        int coutTransfert = mesurerCoutTransfert(pont, handle);
        long dureeMs = (System.nanoTime() - debut) / 1_000_000L;
        return new Resultat(coutAppel, coutTransfert, dureeMs);
    }

    /** Coût moyen d'un aller-retour minimal, en nanosecondes. */
    private static int mesurerCoutAppel(PontNatif pont, long handle) {
        for (int i = 0; i < APPELS_CHAUFFE; i++) {
            if (pont.noop(handle) != 0) {
                return 0;
            }
        }

        long debut = System.nanoTime();
        for (int i = 0; i < APPELS_MESURES; i++) {
            if (pont.noop(handle) != 0) {
                return 0;
            }
        }
        long ecoule = System.nanoTime() - debut;

        long moyenne = ecoule / APPELS_MESURES;
        // Une moyenne nulle signifierait que l'horloge n'a pas la résolution
        // nécessaire : mieux vaut déclarer la mesure absente que publier un coût nul,
        // qui serait interprété comme un appel gratuit par le modèle de coût.
        return moyenne <= 0 ? 0 : (int) Math.min(moyenne, Integer.MAX_VALUE);
    }

    /** Coût de transfert Java vers natif, en nanosecondes par kibioctet. */
    private static int mesurerCoutTransfert(PontNatif pont, long handle) {
        long totalNs = 0;
        long totalKio = 0;

        for (int taille : TAILLES_TRANSFERT) {
            ByteBuffer tampon = ByteBuffer.allocateDirect(taille);
            for (int i = 0; i < taille; i++) {
                tampon.put(i, (byte) i);
            }

            // Une passe de chauffe, puis la mesure.
            if (pont.transferProbe(handle, tampon, taille) < 0) {
                return 0;
            }
            long debut = System.nanoTime();
            long temoin = pont.transferProbe(handle, tampon, taille);
            long ecoule = System.nanoTime() - debut;
            if (temoin < 0) {
                return 0;
            }

            totalNs += ecoule;
            totalKio += taille / 1024L;
        }

        if (totalKio <= 0) {
            return 0;
        }
        long parKio = totalNs / totalKio;
        return parKio <= 0 ? 0 : (int) Math.min(parKio, Integer.MAX_VALUE);
    }
}
