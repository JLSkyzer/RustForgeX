package dev.rustforgex.bridge;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Décodeur CBOR minimal, limité aux types que le runtime natif publie.
 *
 * <p>Composant : C-27 (frontière). Exigence : R-704. Maturité : {@code STABLE}.
 *
 * <p>Types reconnus : entiers non signés, booléens, chaînes UTF-8, tableaux et tables
 * de correspondance. C'est exactement ce que {@code ciborium} produit pour le blob de
 * statut de {@code rfx-model}. Tout autre type majeur est refusé plutôt qu'ignoré :
 * un blob inattendu doit être signalé, jamais interprété au jugé (PARTIE 19.2 — les
 * données franchissant la frontière sont validées).
 *
 * <p>Valeurs Java produites : {@link Long}, {@link Boolean}, {@link String},
 * {@link List} et {@link Map} à clés {@link String}.
 */
public final class CborReader {

    /** Blob CBOR illisible ou contenant un type non supporté. */
    public static final class InvalidCbor extends Exception {

        private static final long serialVersionUID = 1L;

        /** @param message description de l'anomalie rencontrée */
        public InvalidCbor(String message) {
            super(message);
        }
    }

    private final byte[] bytes;
    private int position;

    private CborReader(byte[] bytes) {
        this.bytes = bytes;
    }

    /**
     * Décode un blob CBOR complet.
     *
     * @param bytes blob à décoder
     * @return la valeur décodée
     * @throws InvalidCbor si le blob est tronqué, mal formé, contient un type non
     *     supporté, ou laisse des octets non consommés
     */
    public static Object decode(byte[] bytes) throws InvalidCbor {
        if (bytes == null || bytes.length == 0) {
            throw new InvalidCbor("blob CBOR vide");
        }
        CborReader reader = new CborReader(bytes);
        Object value = reader.readValue();
        if (reader.position != bytes.length) {
            throw new InvalidCbor(
                    "octets excédentaires après la valeur : " + (bytes.length - reader.position));
        }
        return value;
    }

    private Object readValue() throws InvalidCbor {
        int head = readByte();
        int majorType = head >>> 5;
        int info = head & 0x1f;

        switch (majorType) {
            case 0:
                return readArgument(info);
            case 1: {
                // Entier negatif : CBOR encode -1 - n. Une difference signee en est un,
                // et c'est une valeur legitime — contrairement aux reels, qui restent
                // refuses. Java a des entiers signes : la correspondance est exacte.
                // `readArgument` refuse deja ce qui ne tient pas dans un long signe,
                // donc la magnitude est dans [0, Long.MAX_VALUE] et la negation ne
                // deborde pas.
                return -1L - readArgument(info);
            }
            case 3: {
                long length = readArgument(info);
                return new String(readBytes(length), StandardCharsets.UTF_8);
            }
            case 4: {
                long size = readArgument(info);
                List<Object> list = new ArrayList<>();
                for (long i = 0; i < size; i++) {
                    list.add(readValue());
                }
                return list;
            }
            case 5: {
                long size = readArgument(info);
                Map<String, Object> table = new LinkedHashMap<>();
                for (long i = 0; i < size; i++) {
                    Object key = readValue();
                    if (!(key instanceof String text)) {
                        throw new InvalidCbor("clé de table non textuelle : " + key);
                    }
                    table.put(text, readValue());
                }
                return table;
            }
            case 7:
                if (info == 20) {
                    return Boolean.FALSE;
                }
                if (info == 21) {
                    return Boolean.TRUE;
                }
                throw new InvalidCbor("valeur simple non supportée : " + info);
            default:
                throw new InvalidCbor("type majeur CBOR non supporté : " + majorType);
        }
    }

    private long readArgument(int info) throws InvalidCbor {
        if (info < 24) {
            return info;
        }
        int width =
                switch (info) {
                    case 24 -> 1;
                    case 25 -> 2;
                    case 26 -> 4;
                    case 27 -> 8;
                    default -> throw new InvalidCbor("argument CBOR non supporté : " + info);
                };
        long value = 0;
        for (int i = 0; i < width; i++) {
            value = (value << 8) | readByte();
        }
        if (value < 0) {
            throw new InvalidCbor("entier non signé dépassant la capacité d'un long");
        }
        return value;
    }

    private int readByte() throws InvalidCbor {
        if (position >= bytes.length) {
            throw new InvalidCbor("blob CBOR tronqué");
        }
        return bytes[position++] & 0xff;
    }

    private byte[] readBytes(long length) throws InvalidCbor {
        if (length < 0 || length > bytes.length - (long) position) {
            throw new InvalidCbor("longueur de chaîne hors du blob : " + length);
        }
        byte[] extracted = new byte[(int) length];
        System.arraycopy(bytes, position, extracted, 0, extracted.length);
        position += extracted.length;
        return extracted;
    }
}
