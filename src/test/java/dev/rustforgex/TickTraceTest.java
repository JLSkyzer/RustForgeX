package dev.rustforgex;

import dev.rustforgex.bridge.FakeNativeBridge;
import dev.rustforgex.forge.TickCycle;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertDoesNotThrow;
import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * Journal de tick (C-34) : il ne s'écrit pas sur le fil du serveur.
 *
 * <p>T-401 a mesuré 144 à 193 µs par écriture sur le fil du serveur, {@code DEBUG} étant
 * réellement écrit sur un serveur Forge de production (ADR-034).
 */
class TickTraceTest {

    @Test
    @DisplayName("Le journal de tick est confié à l'exécuteur, rien ne tourne sur le fil appelant")
    void theTraceIsHandedOverNotWritten() {
        TickCycle cycle = new TickCycle(new FakeNativeBridge(), 1L, TickCycle.SIDE_SERVER);
        List<Runnable> submitted = new ArrayList<>();

        TickTrace.log(TickTrace.INTERVAL, cycle, submitted::add);

        assertEquals(1, submitted.size(), "une tâche, et une seule, est déposée");
        // Exécutée ailleurs, elle lit les compteurs et écrit, sans runtime démarré.
        assertDoesNotThrow(() -> submitted.get(0).run());
    }
}
