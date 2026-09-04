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
public final class CborLecteur {

    /** Blob CBOR illisible ou contenant un type non supporté. */
    public static final class CborInvalide extends Exception {

        private static final long serialVersionUID = 1L;

        /** @param message description de l'anomalie rencontrée */
        public CborInvalide(String message) {
            super(message);
        }
    }

    private final byte[] octets;
    private int position;

    private CborLecteur(byte[] octets) {
        this.octets = octets;
    }

    /**
     * Décode un blob CBOR complet.
     *
     * @param octets blob à décoder
     * @return la valeur décodée
     * @throws CborInvalide si le blob est tronqué, mal formé, contient un type non
     *     supporté, ou laisse des octets non consommés
     */
    public static Object decoder(byte[] octets) throws CborInvalide {
        if (octets == null || octets.length == 0) {
            throw new CborInvalide("blob CBOR vide");
        }
        CborLecteur lecteur = new CborLecteur(octets);
        Object valeur = lecteur.lireValeur();
        if (lecteur.position != octets.length) {
            throw new CborInvalide(
                    "octets excédentaires après la valeur : " + (octets.length - lecteur.position));
        }
        return valeur;
    }

    private Object lireValeur() throws CborInvalide {
        int tete = lireOctet();
        int typeMajeur = tete >>> 5;
        int information = tete & 0x1f;

        switch (typeMajeur) {
            case 0:
                return lireArgument(information);
            case 3: {
                long longueur = lireArgument(information);
                return new String(lireOctets(longueur), StandardCharsets.UTF_8);
            }
            case 4: {
                long taille = lireArgument(information);
                List<Object> liste = new ArrayList<>();
                for (long i = 0; i < taille; i++) {
                    liste.add(lireValeur());
                }
                return liste;
            }
            case 5: {
                long taille = lireArgument(information);
                Map<String, Object> table = new LinkedHashMap<>();
                for (long i = 0; i < taille; i++) {
                    Object cle = lireValeur();
                    if (!(cle instanceof String texte)) {
                        throw new CborInvalide("clé de table non textuelle : " + cle);
                    }
                    table.put(texte, lireValeur());
                }
                return table;
            }
            case 7:
                if (information == 20) {
                    return Boolean.FALSE;
                }
                if (information == 21) {
                    return Boolean.TRUE;
                }
                throw new CborInvalide("valeur simple non supportée : " + information);
            default:
                throw new CborInvalide("type majeur CBOR non supporté : " + typeMajeur);
        }
    }

    private long lireArgument(int information) throws CborInvalide {
        if (information < 24) {
            return information;
        }
        int longueur =
                switch (information) {
                    case 24 -> 1;
                    case 25 -> 2;
                    case 26 -> 4;
                    case 27 -> 8;
                    default -> throw new CborInvalide("argument CBOR non supporté : " + information);
                };
        long valeur = 0;
        for (int i = 0; i < longueur; i++) {
            valeur = (valeur << 8) | lireOctet();
        }
        if (valeur < 0) {
            throw new CborInvalide("entier non signé dépassant la capacité d'un long");
        }
        return valeur;
    }

    private int lireOctet() throws CborInvalide {
        if (position >= octets.length) {
            throw new CborInvalide("blob CBOR tronqué");
        }
        return octets[position++] & 0xff;
    }

    private byte[] lireOctets(long longueur) throws CborInvalide {
        if (longueur < 0 || longueur > octets.length - (long) position) {
            throw new CborInvalide("longueur de chaîne hors du blob : " + longueur);
        }
        byte[] extrait = new byte[(int) longueur];
        System.arraycopy(octets, position, extrait, 0, extrait.length);
        position += extrait.length;
        return extrait;
    }
}
