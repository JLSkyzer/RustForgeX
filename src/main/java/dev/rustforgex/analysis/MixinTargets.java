package dev.rustforgex.analysis;

import java.nio.charset.StandardCharsets;
import java.util.Set;

/**
 * Classes visées par un mixin, lues dans l'annotation {@code @Mixin} de la classe mixin.
 *
 * <p>Composant : C-07 (ADR-051). Maturité : {@code STABLE}.
 *
 * <p>Mixin réécrit ses cibles au chargement : leur bytecode d'archive n'est pas celui
 * qui s'exécute, et l'analyse les déclare absentes. Le relevé du lancement
 * ({@code TargetScanner}) ne suffit pas ici : il ne voit que les archives de premier
 * niveau de {@code mods}. Celui-ci voit toutes les classes de l'installation.
 *
 * <p>L'annotation est de rétention {@code CLASS} : elle figure dans l'attribut
 * {@code RuntimeInvisibleAnnotations} (JVMS §4.7.17). Ses cibles sont données par
 * {@code value} (classes) et {@code targets} (noms, pour les classes inaccessibles).
 */
public final class MixinTargets {

    /** Issue de la lecture d'une classe. */
    public enum Outcome {
        /** Pas une classe mixin. */
        NOT_MIXIN,
        /** Une classe mixin, dont les cibles ont été relevées. */
        MIXIN,
        /** Une classe mixin dont les cibles n'ont pu être lues. */
        UNREADABLE
    }

    static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    private static final byte[] MIXIN_BYTES = MIXIN.getBytes(StandardCharsets.UTF_8);

    private final byte[] bytes;
    private final ConstantPool pool;
    private final Set<String> targets;

    private MixinTargets(byte[] bytes, ConstantPool pool, Set<String> targets) {
        this.bytes = bytes;
        this.pool = pool;
        this.targets = targets;
    }

    /**
     * Relève les cibles d'une classe, si c'est une classe mixin.
     *
     * @param bytes contenu du fichier {@code .class}
     * @param targets ensemble à compléter, en noms internes
     * @return l'issue ; {@link Outcome#UNREADABLE} si le fichier est illisible alors
     *     qu'il nomme l'annotation
     */
    public static Outcome collect(byte[] bytes, Set<String> targets) {
        ConstantPool pool;
        try {
            pool = ConstantPool.read(bytes);
        } catch (RuntimeException e) {
            pool = null;
        }
        if (pool == null) {
            return contains(bytes, MIXIN_BYTES) ? Outcome.UNREADABLE : Outcome.NOT_MIXIN;
        }
        return collect(bytes, pool, targets);
    }

    /**
     * @param bytes contenu du fichier {@code .class}
     * @param pool son réservoir de constantes
     * @param targets ensemble à compléter, en noms internes
     * @return l'issue
     */
    static Outcome collect(byte[] bytes, ConstantPool pool, Set<String> targets) {
        if (!pool.containsUtf8(MIXIN_BYTES)) {
            return Outcome.NOT_MIXIN;
        }
        try {
            // Nommer l'annotation n'est pas la porter : la bibliothèque Mixin elle-même
            // cite ce descripteur.
            return new MixinTargets(bytes, pool, targets).classAttributes()
                    ? Outcome.MIXIN
                    : Outcome.NOT_MIXIN;
        } catch (RuntimeException e) {
            return Outcome.UNREADABLE;
        }
    }

    /** Parcourt la classe jusqu'à ses attributs ; {@code true} si {@code @Mixin} y est. */
    private boolean classAttributes() {
        int at = pool.end() + 6;
        at += 2 + 2 * u2(at);
        at = skipMembers(at);
        at = skipMembers(at);
        int count = u2(at);
        at += 2;
        boolean found = false;
        for (int attribute = 0; attribute < count; attribute++) {
            String name = pool.utf8(u2(at));
            int length = ConstantPool.u4(bytes, at + 2);
            int body = at + 6;
            if ("RuntimeInvisibleAnnotations".equals(name)
                    || "RuntimeVisibleAnnotations".equals(name)) {
                int annotations = u2(body);
                int position = body + 2;
                for (int index = 0; index < annotations; index++) {
                    if (MIXIN.equals(pool.utf8(u2(position)))) {
                        found = true;
                    }
                    position = annotation(position, true);
                }
            }
            at = body + length;
        }
        return found;
    }

    /** Saute les champs ou les méthodes, et leurs attributs. */
    private int skipMembers(int at) {
        int count = u2(at);
        at += 2;
        for (int member = 0; member < count; member++) {
            int attributes = u2(at + 6);
            at += 8;
            for (int attribute = 0; attribute < attributes; attribute++) {
                at += 6 + ConstantPool.u4(bytes, at + 2);
            }
        }
        return at;
    }

    /**
     * Lit une annotation ; relève ses cibles si c'est {@code @Mixin}.
     *
     * @param at position de l'annotation
     * @param topLevel {@code false} pour une annotation imbriquée, jamais relevée
     * @return la position qui la suit
     */
    private int annotation(int at, boolean topLevel) {
        boolean mixin = topLevel && MIXIN.equals(pool.utf8(u2(at)));
        int pairs = u2(at + 2);
        at += 4;
        for (int pair = 0; pair < pairs; pair++) {
            String name = pool.utf8(u2(at));
            at = elementValue(at + 2,
                    mixin && ("value".equals(name) || "targets".equals(name)));
        }
        return at;
    }

    /**
     * Lit une valeur d'élément (JVMS §4.7.16.1).
     *
     * @param at position de la valeur
     * @param collect {@code true} pour relever les classes et les noms qu'elle porte
     * @return la position qui la suit
     */
    private int elementValue(int at, boolean collect) {
        int tag = bytes[at] & 0xff;
        at++;
        switch (tag) {
            case 'B', 'C', 'D', 'F', 'I', 'J', 'S', 'Z' -> {
                return at + 2;
            }
            case 's' -> {
                if (collect) {
                    String name = pool.utf8(u2(at));
                    if (name == null) {
                        throw new IllegalArgumentException("cible illisible");
                    }
                    targets.add(name.replace('.', '/'));
                }
                return at + 2;
            }
            case 'c' -> {
                if (collect) {
                    String descriptor = pool.utf8(u2(at));
                    if (descriptor == null || !descriptor.startsWith("L")
                            || !descriptor.endsWith(";")) {
                        throw new IllegalArgumentException("cible illisible");
                    }
                    targets.add(descriptor.substring(1, descriptor.length() - 1));
                }
                return at + 2;
            }
            case 'e' -> {
                return at + 4;
            }
            case '@' -> {
                return annotation(at, false);
            }
            case '[' -> {
                int values = u2(at);
                at += 2;
                for (int value = 0; value < values; value++) {
                    at = elementValue(at, collect);
                }
                return at;
            }
            default -> throw new IllegalArgumentException("étiquette " + tag);
        }
    }

    private int u2(int at) {
        return ConstantPool.u2(bytes, at);
    }

    /** Recherche brute, pour un fichier dont le réservoir est illisible. */
    private static boolean contains(byte[] haystack, byte[] needle) {
        outer:
        for (int start = 0; start + needle.length <= haystack.length; start++) {
            for (int offset = 0; offset < needle.length; offset++) {
                if (haystack[start + offset] != needle[offset]) {
                    continue outer;
                }
            }
            return true;
        }
        return false;
    }
}
