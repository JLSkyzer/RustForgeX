package dev.rustforgex.bridge;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Encodeur CBOR minimal, limité aux types qui traversent la frontière FFI.
 *
 * <p>Composant : C-27 (frontière). Exigence : R-704 — aucune structure {@code
 * repr(Rust)} ne traverse la frontière, seuls des tampons CBOR à schéma versionné.
 * R-705 — les chaînes sont UTF-8 avec longueur explicite. Maturité : {@code STABLE}.
 *
 * <p>Le sous-ensemble couvert est exactement celui produit côté Rust par
 * {@code ciborium} pour les structures de {@code rfx-model} : entiers non signés,
 * booléens, chaînes UTF-8 et tables de correspondance. Il est volontairement
 * restreint : écrire un encodeur complet exposerait une surface inutile, et
 * l'ajout d'une dépendance CBOR au JAR du mod serait disproportionné pour quelques
 * dizaines d'octets par démarrage.
 *
 * <p>Cette classe n'encode que le sens Java vers natif. Le sens inverse (blob de
 * statut) est décodé par {@link CborReader}.
 */
public final class Cbor {

    private Cbor() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /** Construit une table de correspondance dont l'ordre d'insertion est conservé. */
    public static Map<String, Object> map() {
        return new LinkedHashMap<>();
    }

    /**
     * Encode une valeur.
     *
     * <p>Types acceptés : {@link Map} à clés {@link String}, {@link String},
     * {@link Boolean}, et tout entier non signé tenant sur 64 bits ({@link Integer},
     * {@link Long}).
     *
     * @param value valeur à encoder
     * @return la représentation CBOR
     * @throws IllegalArgumentException si un type non supporté est rencontré, ou si un
     *     entier est négatif
     */
    public static byte[] encode(Object value) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        write(out, value);
        return out.toByteArray();
    }

    private static void write(ByteArrayOutputStream out, Object value) {
        if (value instanceof Boolean b) {
            out.write(b ? 0xf5 : 0xf4);
        } else if (value instanceof String s) {
            byte[] utf8 = s.getBytes(StandardCharsets.UTF_8);
            header(out, 3, utf8.length);
            out.writeBytes(utf8);
        } else if (value instanceof Integer || value instanceof Long) {
            long n = ((Number) value).longValue();
            if (n < 0) {
                throw new IllegalArgumentException(
                        "seuls les entiers non signés traversent la frontière, reçu " + n);
            }
            header(out, 0, n);
        } else if (value instanceof Map<?, ?> table) {
            header(out, 5, table.size());
            for (Map.Entry<?, ?> e : table.entrySet()) {
                if (!(e.getKey() instanceof String key)) {
                    throw new IllegalArgumentException(
                            "clé de table non textuelle : " + e.getKey());
                }
                write(out, key);
                write(out, e.getValue());
            }
        } else {
            throw new IllegalArgumentException(
                    "type non encodable en CBOR : "
                            + (value == null ? "null" : value.getClass().getName()));
        }
    }

    /** Écrit l'en-tête d'un élément : type majeur sur 3 bits, puis l'argument. */
    private static void header(ByteArrayOutputStream out, int majorType, long argument) {
        int prefix = majorType << 5;
        if (argument < 24) {
            out.write(prefix | (int) argument);
        } else if (argument <= 0xff) {
            out.write(prefix | 24);
            out.write((int) argument);
        } else if (argument <= 0xffff) {
            out.write(prefix | 25);
            out.write((int) (argument >>> 8) & 0xff);
            out.write((int) argument & 0xff);
        } else if (argument <= 0xffff_ffffL) {
            out.write(prefix | 26);
            for (int shift = 24; shift >= 0; shift -= 8) {
                out.write((int) (argument >>> shift) & 0xff);
            }
        } else {
            out.write(prefix | 27);
            for (int shift = 56; shift >= 0; shift -= 8) {
                out.write((int) (argument >>> shift) & 0xff);
            }
        }
    }
}
