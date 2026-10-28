package fixtures;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;

/**
 * Classes de test des types d'annotations (ADR-059) : retentions CLASS et RUNTIME, et
 * toutes les formes de valeurs d'element (constante, chaine, classe, enumeration,
 * annotation imbriquee, tableau).
 */
@Annotated.Visible(name = "classe", kind = ElementType.TYPE, nested = @Annotated.Leaf,
        values = {1, 2, 3}, type = String.class)
public class Annotated {

    /** Retenue a l'execution : `RuntimeVisibleAnnotations`. */
    @Retention(RetentionPolicy.RUNTIME)
    public @interface Visible {
        String name() default "";
        ElementType kind() default ElementType.FIELD;
        Leaf nested() default @Leaf;
        int[] values() default {};
        Class<?> type() default Object.class;
    }

    /** Annotation imbriquee, sans element. */
    @Retention(RetentionPolicy.RUNTIME)
    public @interface Leaf {
    }

    /** Retenue dans le fichier seulement : `RuntimeInvisibleAnnotations`. */
    @Retention(RetentionPolicy.CLASS)
    public @interface Invisible {
        Visible[] inner() default {};
    }

    @Invisible
    static int marked;

    static int plain;

    @Visible(kind = ElementType.METHOD)
    @Invisible(inner = {@Visible(name = "a"), @Visible(name = "b")})
    static void both() {
    }

    static void none() {
    }
}
