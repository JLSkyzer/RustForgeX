package dev.rustforgex.analysis;

import java.io.IOException;
import java.util.Set;

/**
 * Les classes que cette installation peut charger, lues dans leurs archives.
 *
 * <p>Composant : C-07 (ADR-051). Maturité : {@code STABLE}.
 *
 * <p>Utilisée par un seul fil, celui de l'analyse ; jamais depuis le tick.
 */
public interface ClassLibrary extends AutoCloseable {

    /** Reçoit le contenu de chaque classe, une à une. */
    @FunctionalInterface
    interface ClassSink {
        /**
         * @param bytes contenu d'un fichier {@code .class}
         * @return {@code false} pour arrêter le parcours
         */
        boolean accept(byte[] bytes);
    }

    /**
     * Parcourt chaque classe une fois. Une classe masquée par une autre du même nom,
     * que la JVM ne chargerait pas, est sautée.
     *
     * @param sink destinataire
     * @return {@code true} si le parcours est allé à son terme
     * @throws IOException si une archive n'a pu être lue : le parcours est alors
     *     incomplet
     */
    boolean forEachClass(ClassSink sink) throws IOException;

    /**
     * @param internalName nom interne de la classe
     * @return son contenu, ou {@code null} si elle est introuvable ou illisible
     */
    byte[] read(String internalName);

    /**
     * @return les classes qu'un access transformer ou un coremod modifie, en noms
     *     internes ({@link TransformedClasses})
     * @throws IOException si un fichier n'a pu être lu
     */
    Set<String> transformedClasses() throws IOException;

    /** Ferme les archives. Ne lève pas. */
    @Override
    void close();
}
