package dev.rustforgex.command;

import net.minecraft.network.chat.Component;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests de {@code /rfx discover} (C-05, C-38).
 *
 * <p>Cahier des charges : PARTIE 5.5, PARTIE 5.36. Sans Minecraft ni runtime natif : le
 * rapport met en forme un recensement déjà dressé, et c'est ce qui le rend vérifiable.
 */
class DiscoveryReportTest {

    private static DiscoveryReport.Row row(String owner, String label, long samples) {
        return new DiscoveryReport.Row(owner, label, samples);
    }

    private static String flatten(List<Component> lines) {
        StringBuilder text = new StringBuilder();
        for (Component line : lines) {
            text.append(line.toString()).append('\n');
        }
        return text.toString();
    }

    @Test
    @DisplayName("Sans échantillonneur, le rapport le dit sans échouer")
    void withoutASamplerTheReportSaysSo() {
        List<Component> lines = DiscoveryReport.lines(null);

        assertEquals(1, lines.size());
        assertTrue(flatten(lines).contains(DiscoveryReport.KEY_PREFIX + "unavailable"));
    }

    /**
     * R-660 : n'avoir prélevé aucune pile et n'avoir rien trouvé sont deux choses
     * différentes. La première dit d'aller voir pourquoi l'échantillonneur dort.
     */
    @Test
    @DisplayName("R-660 : aucune pile prélevée n'est pas « rien à découvrir »")
    void noSampleTakenIsNotNothingToDiscover() {
        List<Component> lines = DiscoveryReport.lines(
                new DiscoveryReport.Census(0, 0, 0, 0, List.of()));

        assertTrue(flatten(lines).contains(DiscoveryReport.KEY_PREFIX + "not_sampled"));
    }

    @Test
    @DisplayName("Tout sondé : le rapport le dit au lieu d'un classement vide")
    void anEntirelyProbedWorkloadIsReportedAsSuch() {
        List<Component> lines = DiscoveryReport.lines(
                new DiscoveryReport.Census(1_000, 0, 0, 0, List.of()));

        String text = flatten(lines);
        assertTrue(text.contains(DiscoveryReport.KEY_PREFIX + "summary"));
        assertTrue(text.contains(DiscoveryReport.KEY_PREFIX + "nothing"));
    }

    @Test
    @DisplayName("La part affichée est celle des prélèvements du fil autoritatif")
    void theShareIsTakenOverTheSamplesOfTheAuthoritativeThread() {
        List<Component> lines = DiscoveryReport.lines(new DiscoveryReport.Census(
                1_000, 250, 2, 0,
                List.of(row("examplemod", "net.example.Hot#compute", 200),
                        row("minecraft", "net.minecraft.Level#tick", 50))));

        String text = flatten(lines);
        assertEquals(3, lines.size(), "un résumé et deux méthodes");
        assertTrue(text.contains("20.0 %"), "200 sur 1 000 font 20 % : " + text);
        assertTrue(text.contains("examplemod"), "le mod propriétaire guide l'action");
        assertTrue(text.contains("net.example.Hot#compute"));
    }

    /**
     * Le classement est décroissant : la première méthode sans échantillon annonce que
     * toutes les suivantes n'en ont pas non plus.
     */
    @Test
    @DisplayName("Une méthode sans échantillon n'est pas listée")
    void amethodWithoutSamplesIsNotListed() {
        List<Component> lines = DiscoveryReport.lines(new DiscoveryReport.Census(
                100, 5, 2, 0,
                List.of(row("examplemod", "net.example.Hot#compute", 5),
                        row("unknown", "net.example.Cold#idle", 0))));

        assertEquals(2, lines.size(), "un résumé et la seule méthode vue");
        assertFalse(flatten(lines).contains("Cold"));
    }

    /**
     * R-660 : un classement tronqué qui s'annonce reste utilisable ; un classement
     * tronqué qui se tait laisse croire qu'il est complet.
     */
    @Test
    @DisplayName("Un recensement plein le signale sous le classement")
    void afullCensusSaysSoBelowTheRanking() {
        List<Component> lines = DiscoveryReport.lines(new DiscoveryReport.Census(
                100, 5, 8_192, 41,
                List.of(row("examplemod", "net.example.Hot#compute", 5))));

        assertTrue(flatten(lines).contains(DiscoveryReport.KEY_PREFIX + "truncated"));
    }
}
