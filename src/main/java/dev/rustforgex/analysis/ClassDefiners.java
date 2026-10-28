package dev.rustforgex.analysis;

import java.nio.charset.StandardCharsets;

/**
 * Reconnaît une classe qui peut définir d'autres classes à l'exécution.
 *
 * <p>Composant : C-07 (ADR-059). Maturité : {@code STABLE}.
 *
 * <p>Une classe définie à l'exécution (générateur de bytecode, chargeur personnalisé)
 * n'est dans aucune archive : elle échappe à la hiérarchie déclarée. Si elle étend une
 * classe de l'installation et en redéfinit une méthode, un appel que l'analyse croit
 * résolu exécute en réalité un autre code. Toutes les voies de la JVM passent par un
 * des noms ci-dessous ({@code ClassLoader.defineClass}, {@code Lookup.defineClass},
 * {@code Lookup.defineHiddenClass[WithClassData]}, {@code Unsafe.defineClass},
 * {@code Unsafe.defineAnonymousClass}) ; une classe qui l'appelle, ou le nomme pour
 * l'atteindre par réflexion, porte ce nom dans son réservoir de constantes.
 *
 * <p>Reste hors de portée : un nom construit à l'exécution, et l'appel JNI
 * {@code DefineClass} depuis du code natif (ADR-059).
 */
final class ClassDefiners {

    /**
     * Noms recherchés, assemblés à l'exécution : écrits en littéraux, ils figureraient
     * dans le réservoir de cette classe même, qui est dans la couche du jeu, et
     * l'analyse se refuserait à elle-même.
     */
    private static final byte[][] NAMES = {
        utf8(String.join("", "define", "Class")),
        utf8(String.join("", "define", "HiddenClass")),
        utf8(String.join("", "define", "HiddenClassWithClassData")),
        utf8(String.join("", "define", "AnonymousClass")),
    };

    private ClassDefiners() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * @param bytes contenu d'un fichier {@code .class}
     * @return {@code true} si son réservoir nomme une méthode qui définit une classe ;
     *     {@code false} aussi pour un fichier illisible, que la JVM refuserait
     */
    static boolean mentions(byte[] bytes) {
        ConstantPool pool;
        try {
            pool = ConstantPool.read(bytes);
        } catch (RuntimeException e) {
            return false;
        }
        if (pool == null) {
            return false;
        }
        for (byte[] name : NAMES) {
            if (pool.containsUtf8(name)) {
                return true;
            }
        }
        return false;
    }

    private static byte[] utf8(String text) {
        return text.getBytes(StandardCharsets.UTF_8);
    }
}
