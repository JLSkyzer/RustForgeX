package corpus;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Vérité terrain du corpus annoté de C-11 (T-204, ADR-071) : les domaines qu'une
 * méthode écrit réellement à l'exécution, hors des objets qu'elle a elle-même créés et
 * qui ne lui ont pas échappé avant l'écriture.
 *
 * <p>Chaque méthode du corpus porte au moins une de ces annotations. Le domaine se
 * déduit de la mémoire écrite, selon la PARTIE 5.11 :
 * <ul>
 *   <li>champ d'un objet : le domaine de sa classe (ancres de la table) ; une classe
 *       sans ancre donne {@link Unknown} ;</li>
 *   <li>champ statique : {@link ModStatics}, ou {@link JavaStatics} pour une classe du
 *       JDK ;</li>
 *   <li>élément de tableau : le domaine de l'objet ou du statique d'où vient le
 *       tableau ; un tableau reçu en paramètre donne {@link Unknown}.</li>
 * </ul>
 */
public final class W {

    private W() {
    }

    /** Aucune écriture hors de ses propres objets locaux. */
    @Retention(RetentionPolicy.CLASS)
    @Target(ElementType.METHOD)
    public @interface Nothing {
    }

    /** Écrit l'état d'une entité. */
    @Retention(RetentionPolicy.CLASS)
    @Target(ElementType.METHOD)
    public @interface EntityState {
    }

    /** Écrit un inventaire d'entité. */
    @Retention(RetentionPolicy.CLASS)
    @Target(ElementType.METHOD)
    public @interface EntityInventory {
    }

    /** Écrit l'état d'une block entity. */
    @Retention(RetentionPolicy.CLASS)
    @Target(ElementType.METHOD)
    public @interface BlockEntityState {
    }

    /** Écrit les globaux du niveau. */
    @Retention(RetentionPolicy.CLASS)
    @Target(ElementType.METHOD)
    public @interface LevelGlobals {
    }

    /** Écrit un statique d'une classe hors du JDK. */
    @Retention(RetentionPolicy.CLASS)
    @Target(ElementType.METHOD)
    public @interface ModStatics {
    }

    /** Écrit un objet partagé d'une classe sans domaine. */
    @Retention(RetentionPolicy.CLASS)
    @Target(ElementType.METHOD)
    public @interface Unknown {
    }
}
