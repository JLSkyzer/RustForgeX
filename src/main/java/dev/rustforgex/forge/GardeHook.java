package dev.rustforgex.forge;

import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;

/**
 * C-01, FM-02 : encapsulation d'un hook Forge.
 *
 * <p>Cahier des charges : PARTIE 5.1. Test : T-103. Maturité : {@code STABLE}.
 *
 * <p>Chaque hook de RUSTFORGE-X passe par cette garde. Une exception levée dans le
 * code du mod ne doit jamais interrompre le tick ni la séquence de chargement de
 * Forge : elle est capturée, comptée, et le hook est désactivé après
 * {@link #SEUIL_DESACTIVATION} échecs. Un hook désactivé le reste jusqu'au
 * redémarrage : un code qui échoue systématiquement coûterait plus cher qu'il ne
 * rapporte, et masquerait le défaut d'origine.
 *
 * <p>Métriques exposées : {@code rfx.hook.duration_ns}, {@code rfx.hook.errors}
 * (contrat agent 4.1).
 */
public final class GardeHook {

    /** Échecs tolérés avant désactivation définitive du hook (FM-02). */
    public static final int SEUIL_DESACTIVATION = 5;

    private final String nom;
    private final Consumer<String> journal;
    private final AtomicInteger echecs = new AtomicInteger();
    private final AtomicLong appels = new AtomicLong();
    private final AtomicLong dureeTotaleNs = new AtomicLong();
    private volatile boolean desactive;

    /**
     * @param nom nom du hook, repris dans les journaux et les métriques
     * @param journal destination des messages d'anomalie
     */
    public GardeHook(String nom, Consumer<String> journal) {
        this.nom = nom;
        this.journal = journal;
    }

    /**
     * Exécute l'action du hook sous protection.
     *
     * @param action travail à effectuer
     * @return {@code true} si l'action s'est exécutée sans erreur
     */
    public boolean executer(Runnable action) {
        if (desactive) {
            return false;
        }
        long debut = System.nanoTime();
        try {
            action.run();
            return true;
        } catch (RuntimeException | LinkageError | AssertionError e) {
            int total = echecs.incrementAndGet();
            journal.accept("Hook « " + nom + " » en échec (" + total + "/" + SEUIL_DESACTIVATION
                    + ") : " + e);
            if (total >= SEUIL_DESACTIVATION) {
                desactive = true;
                journal.accept("Hook « " + nom + " » désactivé après " + total
                        + " échecs. RUSTFORGE-X continue sans lui ; le jeu n'est pas affecté.");
            }
            return false;
        } finally {
            dureeTotaleNs.addAndGet(System.nanoTime() - debut);
            appels.incrementAndGet();
        }
    }

    /** @return {@code true} si le hook a été désactivé après trop d'échecs */
    public boolean desactive() {
        return desactive;
    }

    /** @return le nombre d'échecs observés ({@code rfx.hook.errors}) */
    public int echecs() {
        return echecs.get();
    }

    /** @return le nombre d'exécutions, réussies ou non */
    public long appels() {
        return appels.get();
    }

    /**
     * @return la durée moyenne d'une exécution en nanosecondes
     *     ({@code rfx.hook.duration_ns}), ou {@code 0} si le hook n'a jamais servi
     */
    public long dureeMoyenneNs() {
        long n = appels.get();
        return n == 0 ? 0 : dureeTotaleNs.get() / n;
    }

    /** @return le nom du hook */
    public String nom() {
        return nom;
    }
}
