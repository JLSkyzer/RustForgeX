package dev.rustforgex.command;

import net.minecraft.network.chat.Component;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests de {@code /rfx top} (C-35, C-38).
 *
 * <p>Cahier des charges : PARTIE 5.33. Sans Minecraft ni runtime natif : le rapport
 * met en forme un classement déjà décodé, et c'est ce qui le rend vérifiable.
 */
class TopReportTest {

    private static Map<String, Object> entry(int probeId, long cost, String source) {
        Map<String, Object> e = new LinkedHashMap<>();
        e.put("probe_id", (long) probeId);
        e.put("work_id_hex", String.format("%016x", 1_000L + probeId));
        e.put("cost_ns_per_tick", cost);
        e.put("source", source);
        // Point fixe, comme sur la frontiere : le decodeur CBOR de Java ne lit pas
        // les reels, et un test qui en emploierait ne testerait pas le vrai format.
        e.put("calls_per_tick_x100", 1_200L);
        e.put("heat", "HOT");
        return e;
    }

    @SafeVarargs
    private static Map<String, Object> top(long tracked, long measured,
            Map<String, Object>... entries) {
        Map<String, Object> t = new LinkedHashMap<>();
        t.put("schema", 1L);
        t.put("tracked", tracked);
        t.put("measured", measured);
        t.put("tick_ns", 20_000_000L);
        t.put("entries", List.of(entries));
        return t;
    }

    /** Concatène les lignes en une chaîne cherchable, clés de traduction comprises. */
    private static String flatten(List<Component> lines) {
        StringBuilder text = new StringBuilder();
        for (Component line : lines) {
            text.append(line.toString()).append('\n');
        }
        return text.toString();
    }

    @Test
    @DisplayName("Un classement absent est dit indisponible, sans échouer")
    void anAbsentRankingIsReportedAsUnavailable() {
        List<Component> lines = TopReport.lines(null, null);

        assertEquals(1, lines.size());
        assertTrue(flatten(lines).contains(TopReport.KEY_PREFIX + "unavailable"));
    }

    /**
     * R-660 : n'avoir rien mesuré et avoir mesuré zéro sont deux choses différentes.
     * Aligner des unités à {@code 0 ns} donnerait à croire qu'on a regardé et trouvé
     * zéro ; dire qu'on n'a rien mesuré invite au contraire à chercher pourquoi.
     */
    @Test
    @DisplayName("R-660 : sans mesure, le rapport le dit au lieu d'aligner des zéros")
    void withoutMeasurementTheReportSaysSoInsteadOfListingZeros() {
        List<Component> lines = TopReport.lines(
                top(2_600, 0, entry(0, 0, "None"), entry(1, 0, "None")), null);

        String text = flatten(lines);
        assertTrue(text.contains(TopReport.KEY_PREFIX + "nothing_measured"));
        assertFalse(text.contains(TopReport.KEY_PREFIX + "entry"),
                "aucune unité ne doit être listée quand rien n'a été mesuré");
    }

    @Test
    @DisplayName("Le résumé rapporte les unités mesurées au total suivi")
    void theSummaryRelatesMeasuredToTracked() {
        List<Component> lines = TopReport.lines(
                top(2_600, 3, entry(0, 5_000, "Probe")), null);

        assertTrue(flatten(lines).contains(TopReport.KEY_PREFIX + "summary"));
        assertEquals(2, lines.size(), "un résumé, puis la seule unité mesurée");
    }

    /**
     * Le classement est décroissant : la première unité sans coût annonce que toutes
     * les suivantes n'en ont pas non plus. Les lister remplirait le chat de méthodes
     * dont on ne sait rien.
     */
    @Test
    @DisplayName("Les unités sans coût ne sont pas listées à la suite des autres")
    void workloadsWithoutCostAreNotListed() {
        List<Component> lines = TopReport.lines(
                top(10, 2,
                        entry(0, 9_000, "Probe"),
                        entry(1, 4_000, "Sampling"),
                        entry(2, 0, "None"),
                        entry(3, 0, "None")),
                null);

        assertEquals(3, lines.size(), "un résumé et deux unités mesurées");
    }

    /**
     * Le natif ne connaît pas les noms : il rend des identifiants de sonde. Sans
     * registre pour les résoudre, le rapport doit rester lisible plutôt que vide.
     */
    @Test
    @DisplayName("Sans registre, l'identifiant de sonde est affiché tel quel")
    void withoutARegistryTheProbeIdIsShownAsIs() {
        List<Component> lines = TopReport.lines(top(5, 1, entry(42, 7_000, "Probe")), null);

        assertTrue(flatten(lines).contains("sonde#42"));
    }

    /**
     * Ce qu'un administrateur de serveur veut savoir en premier n'est pas quelle classe
     * coûte, mais **quel mod**. C'est sur le mod qu'il agira.
     */
    @Test
    @DisplayName("Le mod propriétaire et la part du tick sont affichés")
    void theOwningModAndTickShareAreShown() {
        List<Component> lines = TopReport.lines(top(10, 1, entry(0, 2_000_000, "Probe")), null);

        String text = flatten(lines);
        assertTrue(text.contains("10.0 %"), "2 ms sur un tick de 20 ms font 10 % : " + text);
    }

    /**
     * ADR-030 : le gouverneur coupe une session de chronométrage dès que le budget est
     * dépassé. Sans ce report, l'opérateur à qui {@code /rfx profile 60} a promis
     * soixante secondes lit un classement vide et conclut à une absence de couverture.
     * Une campagne entière l'a conclu à tort.
     */
    @Test
    @DisplayName("ADR-030 : un chronométrage écourté est dit, avec ce qu'il a duré")
    void acutShortProfilingSessionIsReportedWithWhatItLasted() {
        List<Component> lines = TopReport.lines(
                top(10, 1, entry(0, 2_000_000, "Probe")), null,
                new TopReport.Session(1_200, 7, true));

        String text = flatten(lines);
        assertTrue(text.contains(TopReport.KEY_PREFIX + "profile_cancelled"), text);
    }

    /**
     * R-660 à l'envers : quand rien n'a été mesuré, savoir que la fenêtre a été coupée
     * est justement l'information la plus utile. C'est là qu'elle doit apparaître.
     */
    @Test
    @DisplayName("Sans aucune mesure, la coupure est dite quand même")
    void thecutIsReportedEvenWhenNothingWasMeasured() {
        List<Component> lines = TopReport.lines(
                top(2_600, 0, entry(0, 0, "None")), null,
                new TopReport.Session(1_200, 7, true));

        assertTrue(flatten(lines).contains(TopReport.KEY_PREFIX + "profile_cancelled"));
    }

    /**
     * Une session allée à son terme est le cas normal : l'annoncer à chaque fois
     * noierait le seul message qui compte.
     */
    @Test
    @DisplayName("Une session arrivée à son terme ne dit rien")
    void acompletedSessionSaysNothing() {
        List<Component> lines = TopReport.lines(
                top(10, 1, entry(0, 2_000_000, "Probe")), null,
                new TopReport.Session(1_200, 1_200, false));

        assertFalse(flatten(lines).contains(TopReport.KEY_PREFIX + "profile_cancelled"));
    }

    /**
     * R-660 : sans tick connu, la part n'est pas calculable. Choisir un total plausible
     * reviendrait à publier un chiffre que personne n'a mesuré.
     */
    @Test
    @DisplayName("Sans durée de tick, la part est marquée absente et non estimée")
    void withoutATickDurationTheShareIsMarkedAbsent() {
        Map<String, Object> t = top(10, 1, entry(0, 2_000_000, "Probe"));
        t.put("tick_ns", 0L);

        assertTrue(flatten(TopReport.lines(t, null)).contains("—"));
    }
}
