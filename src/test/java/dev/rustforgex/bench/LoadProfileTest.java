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

    /**
     * PARTIE 22 : le profil `heavy` demande 8 000 entités et 2 500 chunks.
     *
     * <p>Les profils précédents étaient tous en dessous de la ligne `vanilla` de la
     * table — 200 entités et 256 chunks contre 200 et 400. Toutes les mesures faites
     * jusqu'ici décrivaient donc un serveur au repos, ce qui compte directement pour un
     * budget exprimé en pourcentage du MSPT.
     */
    @Test
    @DisplayName("PARTIE 22 : le profil heavy invoque 8 000 entités")
    void theHeavyProfileSummonsEightThousandEntities() {
        long summons = LoadProfile.commandsFor(LoadProfile.HEAVY).stream()
                .filter(c -> c.contains("summon"))
                .count();

        assertEquals(8_000, summons);
    }

    /**
     * `/forceload add` refuse au-delà de 256 régions par commande. Une seule commande
     * couvrant les 2 500 chunks échouerait **sans que rien ne le signale** : la charge
     * manquerait et la mesure porterait sur un serveur vide en croyant le contraire.
     */
    @Test
    @DisplayName("Aucune commande forceload ne dépasse la limite de 256 régions")
    void noForceloadCommandExceedsTheLimit() {
        for (String command : LoadProfile.commandsFor(LoadProfile.HEAVY)) {
            if (!command.startsWith("forceload add")) {
                continue;
            }
            String[] parts = command.split(" ");
            int x1 = Integer.parseInt(parts[2]);
            int z1 = Integer.parseInt(parts[3]);
            int x2 = Integer.parseInt(parts[4]);
            int z2 = Integer.parseInt(parts[5]);
            long chunks = (long) (Math.abs(x2 - x1) / 16 + 1) * (Math.abs(z2 - z1) / 16 + 1);

            assertTrue(chunks <= 256,
                    command + " couvre " + chunks + " régions, la limite est 256");
        }
    }

    @Test
    @DisplayName("Le profil heavy charge au moins les 2 500 chunks demandés")
    void theHeavyProfileLoadsTheRequestedChunks() {
        long chunks = 0;
        for (String command : LoadProfile.commandsFor(LoadProfile.HEAVY)) {
            if (!command.startsWith("forceload add")) {
                continue;
            }
            String[] parts = command.split(" ");
            chunks += (long) (Math.abs(Integer.parseInt(parts[4]) - Integer.parseInt(parts[2])) / 16 + 1)
                    * (Math.abs(Integer.parseInt(parts[5]) - Integer.parseInt(parts[3])) / 16 + 1);
        }

        assertTrue(chunks >= 2_500, "seulement " + chunks + " chunks chargés");
    }

    /**
     * À huit mille entités sur la zone, l'entassement est inévitable. Sans désactiver
     * le cramming, le serveur tuerait les entités entassées et ferait disparaître la
     * charge qu'on vient d'installer.
     */
    @Test
    @DisplayName("Le profil heavy désactive le cramming, sinon la charge s'évapore")
    void theHeavyProfileDisablesCramming() {
        assertTrue(LoadProfile.commandsFor(LoadProfile.HEAVY)
                .contains("gamerule maxEntityCramming 0"));
    }
}
