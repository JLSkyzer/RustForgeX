package dev.rustforgex.bench;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;

/**
 * Tests des profils de charge (C-36, PARTIE 22).
 *
 * <p>Ce qui se vérifie ici est ce qui rend une campagne comparable : l'idempotence, et
 * l'ordre des commandes. Une entité placée dans une région non chargée ne tick pas — elle
 * reste figée, et la charge n'existe que sur le papier. C'est exactement le genre de
 * défaut qui ne se voit pas dans un journal et qui invalide toute une campagne.
 */
class LoadProfileTest {

    @Test
    @DisplayName("Aucun profil demandé, aucune commande")
    void noProfileMeansNoCommand() {
        assertTrue(LoadProfile.commandsFor(LoadProfile.NONE).isEmpty());
        assertTrue(LoadProfile.commandsFor(null).isEmpty());
        assertTrue(LoadProfile.commandsFor("profil-inexistant").isEmpty(),
                "un profil inconnu ne s'invente pas une charge");
    }

    /**
     * Le monde est réutilisé d'une exécution à l'autre. Une charge qui s'ajouterait à
     * elle-même ferait dériver la mesure, et la cinquième exécution ne serait plus
     * comparable à la première.
     */
    @Test
    @DisplayName("Le profil commence par remettre le monde dans le même état")
    void theProfileStartsByResettingTheWorld() {
        List<String> commands = LoadProfile.commandsFor(LoadProfile.MOBS);

        int kill = indexOf(commands, "kill @e");
        int removeForceload = indexOf(commands, "forceload remove all");
        int addForceload = indexOf(commands, "forceload add");

        assertTrue(kill >= 0, "les entités de l'exécution précédente doivent disparaître");
        assertTrue(removeForceload >= 0, "les régions forcées précédentes aussi");
        assertTrue(kill < addForceload, "on nettoie avant de recharger");
        assertTrue(removeForceload < addForceload, "on retire avant d'ajouter");
    }

    /**
     * L'ordre décisif : une entité invoquée dans une région non chargée ne tick pas.
     * Inverser ces deux commandes produirait un serveur qui semble chargé et qui ne
     * l'est pas.
     */
    @Test
    @DisplayName("Les régions sont chargées AVANT que les entités y soient placées")
    void chunksAreLoadedBeforeEntitiesArePlaced() {
        List<String> commands = LoadProfile.commandsFor(LoadProfile.MOBS);

        int addForceload = indexOf(commands, "forceload add");
        int firstSummon = indexOf(commands, "summon");

        assertTrue(addForceload >= 0 && firstSummon >= 0);
        assertTrue(addForceload < firstSummon,
                "une entité dans une région non chargée reste figée : la charge serait fictive");
    }

    @Test
    @DisplayName("Ce qui varierait tout seul est figé")
    void whatWouldDriftOnItsOwnIsPinned() {
        List<String> commands = LoadProfile.commandsFor(LoadProfile.CHUNKS);

        // Un cycle jour/nuit, une météo ou des apparitions naturelles rendraient deux
        // exécutions incomparables sans que rien ne le signale.
        assertTrue(commands.contains("gamerule doDaylightCycle false"));
        assertTrue(commands.contains("gamerule doWeatherCycle false"));
        assertTrue(commands.contains("gamerule doMobSpawning false"));
        assertTrue(commands.contains("time set noon"));
        assertTrue(commands.contains("weather clear"));
    }

    @Test
    @DisplayName("Le profil « chunks » ne place aucune entité")
    void theChunksProfilePlacesNoEntity() {
        List<String> commands = LoadProfile.commandsFor(LoadProfile.CHUNKS);

        assertEquals(-1, indexOf(commands, "summon"));
        assertTrue(commands.contains(
                "gamerule randomTickSpeed " + LoadProfile.RANDOM_TICK_SPEED));
    }

    @Test
    @DisplayName("Le profil « mobs » place exactement le nombre d'entités annoncé")
    void theMobsProfilePlacesExactlyTheAnnouncedCount() {
        long summons = LoadProfile.commandsFor(LoadProfile.MOBS).stream()
                .filter(c -> c.contains("summon"))
                .count();

        assertEquals(LoadProfile.MOB_COUNT, summons);
    }

    /**
     * {@code /forceload add} refuse au-delà de 256 régions par commande. Une seule
     * commande trop large échouerait, et la charge serait silencieusement absente.
     */
    @Test
    @DisplayName("La zone forcée tient dans la limite de 256 régions par commande")
    void theForcedAreaFitsTheTwoHundredFiftySixChunkLimit() {
        int chunks = (LoadProfile.FORCELOAD_SPAN / 16) * (LoadProfile.FORCELOAD_SPAN / 16);

        assertTrue(chunks <= 256, "obtenu " + chunks + " régions, la limite est 256");
    }

    @Test
    @DisplayName("Les entités sont réparties, pas empilées sur un même point")
    void entitiesAreSpreadRatherThanStacked() {
        List<String> summons = LoadProfile.commandsFor(LoadProfile.MOBS).stream()
                .filter(c -> c.contains("summon"))
                .toList();

        long distinct = summons.stream().distinct().count();

        assertTrue(distinct > LoadProfile.MOB_COUNT / 2,
                "deux cents entités au même endroit ne produisent pas la même charge que "
                        + "deux cents entités réparties, obtenu " + distinct + " positions");
    }

    @Test
    @DisplayName("Deux appels rendent exactement la même suite")
    void twoCallsProduceTheSameSequence() {
        // Reproductible : un générateur non ensemencé rendrait la campagne incomparable
        // d'une exécution à l'autre.
        assertEquals(LoadProfile.commandsFor(LoadProfile.MOBS),
                LoadProfile.commandsFor(LoadProfile.MOBS));
    }

    @Test
    @DisplayName("Le profil demandé se lit en minuscules, et vaut « none » par défaut")
    void theRequestedProfileIsLowercasedAndDefaultsToNone() {
        String previous = System.getProperty(LoadProfile.PROPERTY);
        try {
            System.clearProperty(LoadProfile.PROPERTY);
            assertEquals(LoadProfile.NONE, LoadProfile.requested());

            System.setProperty(LoadProfile.PROPERTY, "  MOBS  ");
            assertEquals(LoadProfile.MOBS, LoadProfile.requested());

            System.setProperty(LoadProfile.PROPERTY, "   ");
            assertEquals(LoadProfile.NONE, LoadProfile.requested());
        } finally {
            if (previous == null) {
                System.clearProperty(LoadProfile.PROPERTY);
            } else {
                System.setProperty(LoadProfile.PROPERTY, previous);
            }
        }
    }

    @Test
    @DisplayName("Aucune commande n'est vide")
    void noCommandIsBlank() {
        for (String command : LoadProfile.commandsFor(LoadProfile.MOBS)) {
            assertFalse(command.isBlank(), "commande vide dans le profil");
        }
    }

    /** Index de la première commande contenant le fragment, ou {@code -1}. */
    private static int indexOf(List<String> commands, String fragment) {
        for (int i = 0; i < commands.size(); i++) {
            if (commands.get(i).contains(fragment)) {
                return i;
            }
        }
        return -1;
    }
}
