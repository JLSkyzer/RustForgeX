package dev.rustforgex.analysis;

import java.io.ByteArrayInputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.util.Arrays;

/**
 * Réservoir de constantes d'un fichier {@code .class} (JVMS §4.4), parcouru sans être
 * décodé : seules les positions des entrées UTF-8 et Class sont retenues.
 *
 * <p>Composant : C-07 (ADR-051). Maturité : {@code STABLE}.
 *
 * <p>Une lecture hors du tableau lève une {@link RuntimeException} : à l'appelant de
 * la traiter comme un fichier illisible.
 */
final class ConstantPool {

    private static final int MAGIC = 0xCAFEBABE;

    private final byte[] bytes;
    private final int[] utf8;
    private final int[] classes;
    private final int end;

    private ConstantPool(byte[] bytes, int[] utf8, int[] classes, int end) {
        this.bytes = bytes;
        this.utf8 = utf8;
        this.classes = classes;
        this.end = end;
    }

    /**
     * @param bytes contenu d'un fichier {@code .class}
     * @return son réservoir, ou {@code null} si ce n'en est pas un
     */
    static ConstantPool read(byte[] bytes) {
        if (bytes.length < 10 || u4(bytes, 0) != MAGIC) {
            return null;
        }
        int count = u2(bytes, 8);
        int[] utf8 = new int[count];
        int[] classes = new int[count];
        int at = 10;
        for (int index = 1; index < count; index++) {
            switch (bytes[at] & 0xff) {
                case 1 -> {
                    utf8[index] = at + 1;
                    at += 3 + u2(bytes, at + 1);
                }
                case 7 -> {
                    classes[index] = u2(bytes, at + 1);
                    at += 3;
                }
                case 8, 16, 19, 20 -> at += 3;
                case 15 -> at += 4;
                case 3, 4, 9, 10, 11, 12, 17, 18 -> at += 5;
                case 5, 6 -> {
                    at += 9;
                    index++;
                }
                default -> {
                    return null;
                }
            }
        }
        if (at > bytes.length) {
            return null;
        }
        return new ConstantPool(bytes, utf8, classes, at);
    }

    /** @return la position qui suit le réservoir : celle des drapeaux de la classe */
    int end() {
        return end;
    }

    /**
     * @param index indice dans le réservoir
     * @return la chaîne de l'entrée UTF-8, ou {@code null} si ce n'en est pas une
     */
    String utf8(int index) {
        if (index <= 0 || index >= utf8.length || utf8[index] == 0) {
            return null;
        }
        int at = utf8[index];
        // `readUTF` lit exactement ce format : longueur sur deux octets, puis UTF-8
        // modifié (JVMS §4.4.7).
        try (DataInputStream in = new DataInputStream(
                new ByteArrayInputStream(bytes, at, 2 + u2(bytes, at)))) {
            return in.readUTF();
        } catch (IOException e) {
            return null;
        }
    }

    /**
     * @param index indice dans le réservoir
     * @return le nom de l'entrée Class, ou {@code null} si ce n'en est pas une
     */
    String className(int index) {
        if (index <= 0 || index >= classes.length || classes[index] == 0) {
            return null;
        }
        return utf8(classes[index]);
    }

    /**
     * @param value octets recherchés
     * @return {@code true} si une entrée UTF-8 porte exactement ces octets
     */
    boolean containsUtf8(byte[] value) {
        for (int at : utf8) {
            if (at != 0 && u2(bytes, at) == value.length
                    && Arrays.equals(bytes, at + 2, at + 2 + value.length,
                            value, 0, value.length)) {
                return true;
            }
        }
        return false;
    }

    static int u2(byte[] bytes, int at) {
        return ((bytes[at] & 0xff) << 8) | (bytes[at + 1] & 0xff);
    }

    static int u4(byte[] bytes, int at) {
        return (u2(bytes, at) << 16) | u2(bytes, at + 2);
    }
}
