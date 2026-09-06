package dev.rustforgex.telemetry;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests de C-34 Telemetry et de l'anonymisation de C-35 (T-401, T-402, T-412).
 *
 * <p>Cahier des charges : PARTIE 5.32 et 5.33. Exigences : R-560 à R-562, R-571.
 *
 * <p>T-400 — aucune socket ouverte — n'est pas ici : il porte sur l'ensemble des
 * sources, pas sur cette classe, et vit donc dans {@code FoundationsTest} avec les
 * autres vérifications qui portent sur le dépôt.
 */
class TelemetryTest {

    private static Map<String, Object> status() {
        Map<String, Object> tick = new LinkedHashMap<>();
        tick.put("ticks", 1_200L);
        tick.put("unbalanced", 0L);
        tick.put("invalid_transitions", 0L);
        tick.put("hook_budget_exceeded", 3L);

        Map<String, Object> profiler = new LinkedHashMap<>();
        profiler.put("workloads_tracked", 2_600L);
        profiler.put("records_ingested", 900_000L);
        profiler.put("overhead_pct_x100", 32L);
        profiler.put("mspt_pct_x100", 650L);
        profiler.put("baseline_measurements", 0L);
        profiler.put("baseline_overhead_pct_x100", 0L);

        Map<String, Object> root = new LinkedHashMap<>();
        root.put("tick", tick);
        root.put("profiler", profiler);
        return root;
    }

    @Test
    @DisplayName("Les métriques du statut natif sont relevées avec leur unité")
    void nativeStatusMetricsAreCollectedWithTheirUnit() {
        MetricSet set = Telemetry.collect(status(), null, null, null);

        assertEquals(1_200, set.get("rfx.tick.count").value());
        assertEquals(Metric.Type.COUNTER, set.get("rfx.tick.count").type());
        assertEquals(3, set.get("rfx.tick.hook_budget_exceeded").value());
        assertEquals(6.5, set.get("rfx.profiler.mspt_pct").value(), 1e-9);
        assertEquals("%", set.get("rfx.profiler.mspt_pct").unit());
    }

    /**
     * R-660 : une valeur non mesurée ne s'écrit pas. La ligne de base de la PARTIE 12.4
     * ne vaut que si une pause a eu lieu ; publier {@code 0 %} sans mesure ferait passer
     * une absence pour un coût nul, ce qui est exactement l'inverse de la vérité.
     */
    @Test
    @DisplayName("R-660 : une ligne de base jamais mesurée n'est pas publiée à zéro")
    void anUnmeasuredBaselineIsNotPublishedAsZero() {
        MetricSet withoutMeasurement = Telemetry.collect(status(), null, null, null);
        assertNull(withoutMeasurement.get("rfx.profiler.baseline_overhead_pct"),
                "zéro mesure doit vouloir dire « pas encore mesuré », jamais « coût nul »");

        Map<String, Object> measured = status();
        @SuppressWarnings("unchecked")
        Map<String, Object> profiler = (Map<String, Object>) measured.get("profiler");
        profiler.put("baseline_measurements", 4L);
        profiler.put("baseline_overhead_pct_x100", 210L);

        MetricSet withMeasurement = Telemetry.collect(measured, null, null, null);
        assertEquals(2.1, withMeasurement.get("rfx.profiler.baseline_overhead_pct").value(),
                1e-9);
    }

    @Test
    @DisplayName("Une source absente ne produit aucune métrique, plutôt que des zéros")
    void anAbsentSourceProducesNoMetricRatherThanZeros() {
        MetricSet set = Telemetry.collect(null, null, null, null);

        assertEquals(0, set.size());
        assertNull(set.get("rfx.tick.count"));
    }

    @Test
    @DisplayName("Les compteurs du mod complètent ceux du natif")
    void modCountersCompleteTheNativeOnes() {
        MetricSet set = Telemetry.collect(status(),
                new Telemetry.InstrumentationCounts(true, 43_000, 32_712, 2_649, 0, 2_641, 0),
                new Telemetry.DiscoveryCounts(290, 288, 9_757, 16),
                new Telemetry.EventCounts(64_494, 30, 1_006, 0));

        assertEquals(1, set.get("rfx.instr.armed").value());
        assertEquals(32_712, set.get("rfx.instr.classes_missed").value());
        assertEquals(290, set.get("rfx.discovery.mods").value());
        assertEquals(16, set.get("rfx.discovery.duration_ms").value());
        assertEquals(30, set.get("rfx.events.known_types").value());
        assertEquals(0.0, set.get("rfx.discovery.unknown_owner_ratio").value(), 1e-9);
    }

    @Test
    @DisplayName("Sans sonde demandée, le taux de non-attribution n'est pas publié")
    void withoutRequestedProbesTheUnattributedRatioIsNotPublished() {
        MetricSet set = Telemetry.collect(null,
                new Telemetry.InstrumentationCounts(false, 0, 0, 0, 0, 0, 0),
                null, null);

        assertNull(set.get("rfx.discovery.unknown_owner_ratio"),
                "un rapport sans dénominateur n'est pas zéro, il n'existe pas");
    }

    /** R-562 : une métrique sans nom conforme ou sans sémantique n'est pas exploitable. */
    @Test
    @DisplayName("R-562 : un nom non conforme ou une description absente sont refusés")
    void anInvalidNameOrAnAbsentDescriptionIsRejected() {
        assertThrows(IllegalArgumentException.class,
                () -> Metric.counter("tick.count", 1, "sans le préfixe rfx"));
        assertThrows(IllegalArgumentException.class,
                () -> Metric.counter("rfx.count", 1, "sans domaine"));
        assertThrows(IllegalArgumentException.class,
                () -> Metric.counter("rfx.tick.count", 1, "  "));
        assertThrows(IllegalArgumentException.class,
                () -> Metric.gauge("rfx.tick.ratio", Double.NaN, "%", "valeur impossible"));
    }

    /**
     * T-402 : la cardinalité est bornée, et la borne est vérifiée à l'exécution.
     *
     * <p>Un recueil dont le nombre de séries dépend d'une donnée d'exécution grossit
     * avec la partie. La PARTIE 5.32 l'interdit ; encore faut-il que l'interdiction
     * échoue quelque part, sans quoi elle n'est qu'un commentaire.
     */
    @Test
    @DisplayName("T-402 : au-delà de la borne, le recueil échoue au lieu de grossir")
    void beyondTheBoundTheSetFailsInsteadOfGrowing() {
        MetricSet set = new MetricSet();
        for (int i = 0; i < MetricSet.MAX_SERIES; i++) {
            set.add(Metric.counter("rfx.essai.serie" + i, i, "série de remplissage"));
        }
        assertEquals(MetricSet.MAX_SERIES, set.size());

        assertThrows(IllegalStateException.class,
                () -> set.add(Metric.counter("rfx.essai.detrop", 1, "une de trop")));
    }

    @Test
    @DisplayName("T-402 : un nom en double est refusé, jamais écrasé en silence")
    void aDuplicateNameIsRejected() {
        MetricSet set = new MetricSet();
        set.add(Metric.counter("rfx.tick.count", 1, "première"));

        assertThrows(IllegalStateException.class,
                () -> set.add(Metric.counter("rfx.tick.count", 2, "seconde")));
    }

    /**
     * T-412 : R-571 interdit qu'un chemin utilisateur complet apparaisse dans un rapport
     * exportable. Un rapport est fait pour être envoyé ; il ne doit pas dire où habite
     * celui qui l'envoie.
     */
    @Test
    @DisplayName("T-412 : le répertoire de jeu et le répertoire personnel sont remplacés")
    void theGameDirectoryAndHomeAreReplaced() {
        Anonymizer anonymizer = Anonymizer.of(
                Path.of("/maison/joueur/instance"), Path.of("/maison/joueur"));

        String cleaned = anonymizer.clean("/maison/joueur/instance/mods/un.jar");

        assertEquals(Anonymizer.GAME_DIR + "/mods/un.jar", cleaned);
        assertFalse(cleaned.contains("joueur"), "le nom de session ne doit pas survivre");
    }

    /**
     * Le répertoire de jeu est presque toujours SOUS le répertoire personnel. Remplacer
     * le plus court d'abord masquerait le plus informatif des deux.
     */
    @Test
    @DisplayName("T-412 : la racine la plus longue est remplacée en premier")
    void theLongestRootIsReplacedFirst() {
        Anonymizer anonymizer = Anonymizer.of(
                Path.of("/maison/joueur/instance"), Path.of("/maison/joueur"));

        assertEquals(Anonymizer.HOME + "/ailleurs/note.txt",
                anonymizer.clean("/maison/joueur/ailleurs/note.txt"));
        assertEquals(Anonymizer.GAME_DIR + "/config",
                anonymizer.clean("/maison/joueur/instance/config"));
    }

    @Test
    @DisplayName("T-412 : les deux séparateurs de chemin sont reconnus")
    void bothPathSeparatorsAreRecognised() {
        Anonymizer anonymizer = Anonymizer.of(null, Path.of("/maison/joueur"));

        assertTrue(anonymizer.clean("/maison/joueur/x").startsWith(Anonymizer.HOME));
        assertTrue(anonymizer.clean("\\maison\\joueur\\x").startsWith(Anonymizer.HOME),
                "une trace peut porter l'autre séparateur");
    }

    @Test
    @DisplayName("Sans racine connue, l'anonymiseur ne modifie rien et n'échoue pas")
    void withoutAKnownRootNothingIsChanged() {
        Anonymizer anonymizer = Anonymizer.of(null, null);

        assertEquals(0, anonymizer.rootCount());
        assertEquals("un texte", anonymizer.clean("un texte"));
        assertNull(anonymizer.clean((String) null));
        assertNull(anonymizer.clean((Path) null));
    }

    /**
     * R-562 : toute métrique doit être documentée dans {@code BENCHMARKS.md}.
     *
     * <p>Un catalogue qu'aucun test ne garde diverge à la première métrique ajoutée, et
     * devient alors pire qu'absent : il décrit un système qui n'existe plus. Le test
     * porte sur toutes les métriques que le code sait produire, y compris les
     * conditionnelles — d'où le relevé fait avec toutes les sources présentes.
     */
    @Test
    @DisplayName("R-562 : chaque métrique produite figure dans BENCHMARKS.md")
    void everyProducedMetricIsDocumented() throws java.io.IOException {
        Map<String, Object> complete = status();
        @SuppressWarnings("unchecked")
        Map<String, Object> profiler = (Map<String, Object>) complete.get("profiler");
        profiler.put("baseline_measurements", 1L);
        complete.put("probes", new LinkedHashMap<String, Object>(Map.of(
                "records_consumed", 1L, "records_lost", 0L,
                "native_bytes", 0L, "native_limit_bytes", 0L)));

        MetricSet set = Telemetry.collect(complete,
                new Telemetry.InstrumentationCounts(true, 1, 1, 1, 0, 1, 0),
                new Telemetry.DiscoveryCounts(1, 1, 1, 1),
                new Telemetry.EventCounts(1, 1, 1, 0));

        Path catalogue = Path.of("").toAbsolutePath().resolve("BENCHMARKS.md");
        assertTrue(java.nio.file.Files.isRegularFile(catalogue),
                "catalogue introuvable : " + catalogue);
        String documented = java.nio.file.Files.readString(
                catalogue, java.nio.charset.StandardCharsets.UTF_8);

        for (Metric metric : set.all()) {
            assertTrue(documented.contains("`" + metric.name() + "`"),
                    "R-562 : « " + metric.name() + " » n'est pas documentée dans "
                            + "BENCHMARKS.md");
        }
        assertTrue(set.size() >= 25, "relevé trop maigre pour vérifier le catalogue : "
                + set.size() + " séries");
    }

    @Test
    @DisplayName("L'export JSON porte le nom, le type, l'unité et la sémantique")
    void theJsonExportCarriesNameTypeUnitAndMeaning() {
        MetricSet set = Telemetry.collect(status(), null, null, null);

        String json = MetricsJson.render(set, null);

        assertTrue(json.startsWith("{\n  \"schema\": 1"), json.substring(0, 40));
        assertTrue(json.contains("\"name\": \"rfx.tick.count\""));
        assertTrue(json.contains("\"type\": \"counter\""));
        assertTrue(json.contains("\"unit\": \"%\""));
        assertTrue(json.contains("\"value\": 1200"));
        assertTrue(json.contains("\"description\": \"Ticks dont la fenêtre"));
        // Les virgules séparent les champs ; ce qu'il ne doit pas y avoir, c'est une
        // virgule DÉCIMALE, que produirait un formatage sous locale française.
        assertTrue(json.contains("\"value\": 6.5"), "le séparateur décimal doit être un point");
        assertFalse(json.contains("6,5"));
    }

    @Test
    @DisplayName("L'export JSON assainit ce qu'il écrit")
    void theJsonExportSanitisesWhatItWrites() {
        MetricSet set = new MetricSet();
        set.add(Metric.counter("rfx.essai.chemin", 1, "relevé depuis /maison/joueur/x"));

        String json = MetricsJson.render(set, Anonymizer.of(null, Path.of("/maison/joueur")));

        assertFalse(json.contains("joueur"));
        assertTrue(json.contains(Anonymizer.HOME + "/x"));
    }

    @Test
    @DisplayName("Un guillemet dans une description ne casse pas le document")
    void aQuoteInADescriptionDoesNotBreakTheDocument() {
        MetricSet set = new MetricSet();
        set.add(Metric.counter("rfx.essai.guillemet", 1, "dit \"ceci\"\net cela"));

        String json = MetricsJson.render(set, null);

        assertTrue(json.contains("\\\"ceci\\\""));
        assertTrue(json.contains("\\n"));
        assertNotNull(json);
    }
}
