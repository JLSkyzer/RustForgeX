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
 * statut) est décodé par {@link CborLecteur}.
 */
public final class Cbor {

    private Cbor() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /** Construit une table de correspondance dont l'ordre d'insertion est conservé. */
    public static Map<String, Object> table() {
        return new LinkedHashMap<>();
    }

    /**
     * Encode une valeur.
     *
     * <p>Types acceptés : {@link Map} à clés {@link String}, {@link String},
     * {@link Boolean}, et tout entier non signé tenant sur 64 bits ({@link Integer},
     * {@link Long}).
     *
     * @param valeur valeur à encoder
     * @return la représentation CBOR
     * @throws IllegalArgumentException si un type non supporté est rencontré, ou si un
     *     entier est négatif
     */
    public static byte[] encoder(Object valeur) {
        ByteArrayOutputStream sortie = new ByteArrayOutputStream();
        ecrire(sortie, valeur);
        return sortie.toByteArray();
    }

    private static void ecrire(ByteArrayOutputStream sortie, Object valeur) {
        if (valeur instanceof Boolean b) {
            sortie.write(b ? 0xf5 : 0xf4);
        } else if (valeur instanceof String s) {
            byte[] octets = s.getBytes(StandardCharsets.UTF_8);
            enTete(sortie, 3, octets.length);
            sortie.writeBytes(octets);
        } else if (valeur instanceof Integer || valeur instanceof Long) {
            long n = ((Number) valeur).longValue();
            if (n < 0) {
                throw new IllegalArgumentException(
                        "seuls les entiers non signés traversent la frontière, reçu " + n);
            }
            enTete(sortie, 0, n);
        } else if (valeur instanceof Map<?, ?> map) {
            enTete(sortie, 5, map.size());
            for (Map.Entry<?, ?> e : map.entrySet()) {
                if (!(e.getKey() instanceof String cle)) {
                    throw new IllegalArgumentException(
                            "clé de table non textuelle : " + e.getKey());
                }
                ecrire(sortie, cle);
                ecrire(sortie, e.getValue());
            }
        } else {
            throw new IllegalArgumentException(
                    "type non encodable en CBOR : "
                            + (valeur == null ? "null" : valeur.getClass().getName()));
        }
    }

    /** Écrit l'en-tête d'un élément : type majeur sur 3 bits, puis l'argument. */
    private static void enTete(ByteArrayOutputStream sortie, int typeMajeur, long argument) {
        int prefixe = typeMajeur << 5;
        if (argument < 24) {
            sortie.write(prefixe | (int) argument);
        } else if (argument <= 0xff) {
            sortie.write(prefixe | 24);
            sortie.write((int) argument);
        } else if (argument <= 0xffff) {
            sortie.write(prefixe | 25);
            sortie.write((int) (argument >>> 8) & 0xff);
            sortie.write((int) argument & 0xff);
        } else if (argument <= 0xffff_ffffL) {
            sortie.write(prefixe | 26);
            for (int decalage = 24; decalage >= 0; decalage -= 8) {
                sortie.write((int) (argument >>> decalage) & 0xff);
            }
        } else {
            sortie.write(prefixe | 27);
            for (int decalage = 56; decalage >= 0; decalage -= 8) {
                sortie.write((int) (argument >>> decalage) & 0xff);
            }
        }
    }
}
