package dev.rustforgex.bench;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.file.Path;

/**
 * Tests de la surveillance des ouvrages de test et du nom du fichier de crafting (C-36,
 * PARTIE 20.3.4).
 *
 * <p>La surveillance est ce qui rend invalide une comparaison entre ouvrages morts : si
 * elle comptait mal, trois circuits immobiles passeraient pour égaux.
 */
class PartWatchTest {

    @Test
    @DisplayName("Le premier relevé fixe l'état, seuls les suivants comptent")
    void theFirstRecordIsNotAChange() {
        PartWatch watch = new PartWatch("a", "b");
        watch.record(5, 7);
        watch.record(5, 8);
        watch.record(6, 8);
        watch.record(6, 8);

        assertEquals("a 1, b 1", watch.summary());
        assertTrue(watch.everyPartMoved());
    }

    @Test
    @DisplayName("Une partie restée immobile rend l'ouvrage inerte")
    void aStillPartMakesTheBuildInert() {
        PartWatch watch = new PartWatch("anneau", "lampe");
        watch.record(0, 1);
        watch.record(1, 1);
        watch.record(0, 1);

        assertEquals("anneau 2, lampe 0", watch.summary());
        assertFalse(watch.everyPartMoved());
    }

    @Test
    @DisplayName("Un ouvrage jamais relevé n'a rien prouvé")
    void anUnobservedBuildProvesNothing() {
        assertFalse(new PartWatch("a").everyPartMoved());
    }

    @Test
    @DisplayName("Un relevé du mauvais nombre de parties est refusé")
    void aMismatchedRecordIsRefused() {
        PartWatch watch = new PartWatch("a", "b");
        assertThrows(IllegalArgumentException.class, () -> watch.record(1));
    }

    @Test
    @DisplayName("Le fichier de crafting est voisin de l'empreinte, suffixé -craft")
    void theCraftFileSitsNextToTheDigest() {
        Path out = Path.of("runs", "gameplay", "g12-ref1.json");

        assertEquals(Path.of("runs", "gameplay", "g12-ref1-craft.json"),
                DigestRecorder.craftPath(out));
    }
}
