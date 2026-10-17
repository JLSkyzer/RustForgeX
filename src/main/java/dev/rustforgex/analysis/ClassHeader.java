package dev.rustforgex.analysis;

/**
 * En-tête d'un fichier {@code .class} : nom, super-classe et drapeaux, sans le reste.
 *
 * <p>Composant : C-07 (ADR-051). Maturité : {@code STABLE}.
 *
 * <p>La hiérarchie de l'installation compte des dizaines de milliers de classes : en
 * lire l'en-tête suffit, sans dépendance et sans rien charger dans la JVM.
 *
 * @param name nom interne de la classe
 * @param superName nom interne de sa super-classe, vide pour {@code java/lang/Object}
 * @param access drapeaux d'accès de la classe
 */
public record ClassHeader(String name, String superName, int access) {

    /**
     * Lit l'en-tête d'un fichier {@code .class}.
     *
     * @param bytes contenu du fichier
     * @return l'en-tête, ou {@code null} si le contenu n'est pas un fichier
     *     {@code .class} lisible
     */
    public static ClassHeader parse(byte[] bytes) {
        try {
            ConstantPool pool = ConstantPool.read(bytes);
            return pool == null ? null : read(bytes, pool);
        } catch (RuntimeException e) {
            return null;
        }
    }

    /**
     * @param bytes contenu du fichier
     * @param pool son réservoir de constantes
     * @return l'en-tête, ou {@code null} s'il est illisible
     */
    static ClassHeader read(byte[] bytes, ConstantPool pool) {
        int at = pool.end();
        int access = ConstantPool.u2(bytes, at);
        String name = pool.className(ConstantPool.u2(bytes, at + 2));
        int superIndex = ConstantPool.u2(bytes, at + 4);
        String superName = superIndex == 0 ? "" : pool.className(superIndex);
        if (name == null || superName == null) {
            return null;
        }
        return new ClassHeader(name, superName, access);
    }
}
