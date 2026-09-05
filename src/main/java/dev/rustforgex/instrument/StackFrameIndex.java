package dev.rustforgex.instrument;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * C-05 : correspondance entre une trame de pile et un identifiant de sonde.
 *
 * <p>Cahier des charges : PARTIE 5.5, échantillonnage périodique.
 * Maturité : {@code STABLE}.
 *
 * <h2>Pourquoi cette classe existe</h2>
 *
 * <p>Un identifiant de sonde est attribué à un triplet — classe, méthode, descripteur.
 * Une {@link StackTraceElement}, elle, ne porte que la classe et le nom de la méthode :
 * <strong>le descripteur est perdu</strong>. Deux surcharges d'une même méthode sont
 * donc indiscernables dans une pile.
 *
 * <p>Cet index note cette ambiguïté au lieu de la trancher. Quand deux sondes partagent
 * une classe et un nom de méthode, la trame correspondante n'est attribuée à
 * <strong>aucune</strong> des deux : attribuer au hasard fausserait durablement le
 * classement des unités de travail, alors que ne pas attribuer ne fait que perdre un
 * échantillon. C'est le principe UNKNOWN = CONSERVATIVE appliqué à la mesure.
 */
public final class StackFrameIndex {

    /** Rendu quand aucune sonde ne correspond à la trame. */
    public static final int NO_PROBE = -1;

    /** Rendu quand plusieurs sondes correspondent : la trame n'est pas attribuable. */
    public static final int AMBIGUOUS = -2;

    /**
     * Clé « nom interne de classe # nom de méthode » vers identifiant de sonde.
     *
     * <p>Alimenté depuis les fils de chargement de classes, lu depuis le fil
     * d'échantillonnage : la table doit être concurrente.
     */
    private final Map<String, Integer> byFrame = new ConcurrentHashMap<>(16_384);

    /** Trames dont plusieurs surcharges portent une sonde. */
    private volatile long ambiguous;

    /**
     * Déclare qu'une sonde a été posée sur une méthode.
     *
     * @param classInternalName nom interne, par exemple {@code net/minecraft/…/Mob}
     * @param methodName nom de la méthode
     * @param probeId identifiant attribué
     */
    public void declare(String classInternalName, String methodName, int probeId) {
        if (probeId < 0) {
            return;
        }
        String key = key(classInternalName, methodName);
        Integer previous = byFrame.putIfAbsent(key, probeId);
        if (previous != null && previous != probeId && previous != AMBIGUOUS) {
            // Une seconde surcharge : la trame cesse d'être attribuable, et le reste.
            byFrame.put(key, AMBIGUOUS);
            ambiguous++;
        }
    }

    /**
     * Cherche la sonde correspondant à une trame de pile.
     *
     * <p>N'alloue pas au-delà de la clé de recherche : appelée jusqu'à quelques
     * milliers de fois par seconde depuis le fil d'échantillonnage.
     *
     * @param frame trame de pile
     * @return l'identifiant, {@link #NO_PROBE} ou {@link #AMBIGUOUS}
     */
    public int probeFor(StackTraceElement frame) {
        Integer found = byFrame.get(
                key(frame.getClassName().replace('.', '/'), frame.getMethodName()));
        return found == null ? NO_PROBE : found;
    }

    /** @return le nombre de trames rendues inattribuables par une surcharge */
    public long ambiguousFrames() {
        return ambiguous;
    }

    /** @return le nombre de trames connues, ambiguës comprises */
    public int size() {
        return byFrame.size();
    }

    /** Clé de recherche : le descripteur n'y figure pas, une pile ne le porte pas. */
    private static String key(String classInternalName, String methodName) {
        return classInternalName + '#' + methodName;
    }
}
