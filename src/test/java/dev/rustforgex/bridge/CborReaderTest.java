package dev.rustforgex.bridge;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

/**
 * Tests du décodeur CBOR de la frontière (IF-01).
 *
 * <p>Le décodeur est délibérément minimal : un décodeur qui traverse une frontière de
 * confiance a d'autant moins de défauts possibles qu'il accepte moins de choses. Encore
 * faut-il qu'il accepte ce que le natif produit réellement — sans quoi le blob
 * <strong>entier</strong> devient illisible, pas seulement le champ fautif.
 *
 * <p>Trois défauts de cette famille ont été trouvés en production le 2026-09-07 : un
 * réel dans le classement des unités, un entier non signé au-delà de {@code 2^63} pour
 * un identifiant, et une différence signée dans le statut. Chacun rendait muet tout un
 * pan du mod, sans message. Ces tests fixent le contrat.
 */
class CborReaderTest {

    private static Object decode(int... bytes) throws CborReader.InvalidCbor {
        byte[] blob = new byte[bytes.length];
        for (int i = 0; i < bytes.length; i++) {
            blob[i] = (byte) bytes[i];
        }
        return CborReader.decode(blob);
    }

    /**
     * Une différence signée est une valeur légitime : le profilage peut mesurer un
     * écart négatif quand le bruit l'emporte, et c'est ainsi qu'il doit entrer dans une
     * médiane pour s'y annuler. Le décodeur n'avait aucun cas pour le type majeur 1.
     */
    @Test
    @DisplayName("Un entier négatif se décode, quelle que soit sa largeur")
    void aNegativeIntegerDecodes() throws Exception {
        // 0x20 : -1 encodé dans l'en-tête même.
        assertEquals(-1L, decode(0x20));
        // 0x38 0x63 : -100.
        assertEquals(-100L, decode(0x38, 0x63));
        // 0x3b suivi de huit octets : -1 - 0x0000_0000_0000_00ff = -256.
        assertEquals(-256L, decode(0x3b, 0, 0, 0, 0, 0, 0, 0, 0xff));
    }

    @Test
    @DisplayName("Une table peut porter des valeurs négatives et positives mêlées")
    void aTableCarriesBothSigns() throws Exception {
        // { "a": -100, "b": 24 }
        Object decoded = decode(0xa2,
                0x61, 'a', 0x38, 0x63,
                0x61, 'b', 0x18, 0x18);

        assertEquals(Map.of("a", -100L, "b", 24L), decoded);
    }

    /**
     * Les réels restent refusés, et c'est délibéré : le natif exprime ses fractions en
     * point fixe — {@code overhead_pct_x100} — précisément pour ne pas avoir à les
     * transmettre.
     */
    @Test
    @DisplayName("Un réel reste refusé, contrairement aux entiers signés")
    void aFloatIsStillRejected() {
        // 0xfb suivi de huit octets : réel sur soixante-quatre bits.
        assertThrows(CborReader.InvalidCbor.class,
                () -> decode(0xfb, 0x3f, 0xf0, 0, 0, 0, 0, 0, 0));
    }

    /**
     * Java n'a pas de type non signé : un {@code u64} au-delà de {@code 2^63} ne peut
     * pas être représenté, et l'accepter en le tronquant serait pire que le refuser.
     */
    @Test
    @DisplayName("Un entier non signé au-delà de 2^63 est refusé, pas tronqué")
    void anUnsignedIntegerBeyondTwoToTheSixtyThreeIsRejected() {
        assertThrows(CborReader.InvalidCbor.class,
                () -> decode(0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff));
    }

    @Test
    @DisplayName("Les booléens du type majeur 7 restent acceptés")
    void booleansAreStillAccepted() throws Exception {
        assertEquals(Boolean.TRUE, decode(0xf5));
        assertEquals(Boolean.FALSE, decode(0xf4));
    }
}
