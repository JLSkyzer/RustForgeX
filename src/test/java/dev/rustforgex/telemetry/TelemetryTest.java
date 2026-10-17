package dev.rustforgex.telemetry;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
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

    /**
     * R-421, R-660 : la charge externe n'est publiée qu'une fois relevée ; avant, seul
     * le nombre de relevés, nul, apparaît.
     */
    @Test
    @DisplayName("R-421 : la charge externe n'est publiée qu'après le premier relevé")
    void theExternalLoadIsPublishedOnlyOnceMeasured() {
        Map<String, Object> before = status();
        before.put("scheduler", new LinkedHashMap<String, Object>(Map.of(
                "external_runnable", 0L, "external_runnable_max", 0L,
                "external_load_reports", 0L)));
        MetricSet unmeasured = Telemetry.collect(before, null, null, null);
        assertEquals(0, unmeasured.get("rfx.sched.external_load_reports").value());
        assertNull(unmeasured.get("rfx.sched.external_runnable"));

        Map<String, Object> after = status();
        after.put("scheduler", new LinkedHashMap<String, Object>(Map.of(
                "external_runnable", 12L, "external_runnable_max", 31L,
                "external_load_reports", 4L)));
        MetricSet measured = Telemetry.collect(after, null, null, null);
        assertEquals(12, measured.get("rfx.sched.external_runnable").value());
        assertEquals("fils", measured.get("rfx.sched.external_runnable").unit());
        assertEquals(31, measured.get("rfx.sched.external_runnable_max").value());
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

    @Test
    @DisplayName("Les gardes d'accroche sont exportées ; une durée non mesurée ne l'est pas")
    void hookGuardsAreExported() {
        MetricSet set = Telemetry.collect(null, null, null, null, null, List.of(
                new Telemetry.HookCounts("serverTickPre", 40, 40, 350, 0, false),
                new Telemetry.HookCounts("serverTickPost", 40, 0, 0, 5, true)));

        assertEquals(40, set.get("rfx.hook.calls{hook=serverTickPre}").value());
        assertEquals(350, set.get("rfx.hook.duration_ns{hook=serverTickPre}").value());
        assertEquals(5, set.get("rfx.hook.errors{hook=serverTickPost}").value());
        assertNull(set.get("rfx.hook.duration_ns{hook=serverTickPost}"),
                "aucune exécution chronométrée : la durée n'est pas publiée (R-660)");
        assertEquals(1, set.get("rfx.hook.disabled").value());
    }

    @Test
    @DisplayName("Sans garde rattachée, aucune série d'accroche n'est publiée")
    void noHookNoSeries() {
        MetricSet set = Telemetry.collect(null, null, null, null, null, List.of());
        assertNull(set.get("rfx.hook.disabled"));
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
        complete.put("scheduler", new LinkedHashMap<String, Object>(Map.of(
                "external_runnable", 12L, "external_runnable_max", 31L,
                "external_load_reports", 4L)));
        complete.put("db", openDb(4L, 3L));
        complete.put("analysis", analysis(52_000L, 7L, 3L, 1_500_000L));

        MetricSet set = Telemetry.collect(complete,
                new Telemetry.InstrumentationCounts(true, 1, 1, 1, 0, 1, 0),
                new Telemetry.DiscoveryCounts(1, 1, 1, 1),
                new Telemetry.EventCounts(1, 1, 1, 0),
                new Telemetry.SamplingCounts(1, 1, 1, 0, 1, 1, 0));

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
        assertTrue(set.size() >= 33, "relevé trop maigre pour vérifier le catalogue : "
                + set.size() + " séries");
    }

    /** Sous-table {@code db} d'une base ouverte. */
    private static Map<String, Object> openDb(long load, long writes) {
        Map<String, Object> db = new LinkedHashMap<>();
        db.put("enabled", true);
        db.put("open", true);
        db.put("open_failures", 0L);
        db.put("load", load);
        db.put("loaded_entries", 1_800L);
        db.put("rejected_entries", 2L);
        db.put("entries", 2_000L);
        db.put("estimated_bytes", 3_000_000L);
        db.put("evictions", 0L);
        db.put("syncs", 3L);
        db.put("handed_back", 0L);
        db.put("pending", 0L);
        db.put("writes", writes);
        db.put("write_failures", 0L);
        db.put("persist_ms", 12L);
        db.put("disabled", false);
        return db;
    }

    @Test
    @DisplayName("C-09 : rien n'est publié de la base avant son ouverture (R-660)")
    void theDatabaseIsSilentUntilOpened() {
        Map<String, Object> status = status();
        Map<String, Object> closed = openDb(0L, 0L);
        closed.put("open", false);
        status.put("db", closed);
        MetricSet set = Telemetry.collect(status, null, null, null);
        assertEquals(0, set.get("rfx.db.open_failures").value());
        assertNull(set.get("rfx.db.entries"), "pas encore ouverte");

        status.put("db", openDb(0L, 0L));
        set = Telemetry.collect(status, null, null, null);
        assertEquals(2_000, set.get("rfx.db.entries").value());
        assertNull(set.get("rfx.db.loaded_entries"), "relecture en cours");
        assertNull(set.get("rfx.db.persist_ms"), "rien d'écrit encore");

        status.put("db", openDb(4L, 3L));
        set = Telemetry.collect(status, null, null, null);
        assertEquals(1_800, set.get("rfx.db.loaded_entries").value());
        assertEquals(2, set.get("rfx.db.load_rejected_entries").value());
        assertEquals(12, set.get("rfx.db.persist_ms").value());
        assertEquals("ms", set.get("rfx.db.persist_ms").unit());
    }

    /** Sous-table {@code analysis} du statut natif. */
    private static Map<String, Object> analysis(long hierarchy, long resolved, long unresolved,
            long buildNs) {
        Map<String, Object> analysis = new LinkedHashMap<>();
        analysis.put("enabled", true);
        analysis.put("hierarchy_classes", hierarchy);
        analysis.put("classes_provided", 40L);
        analysis.put("classes_absent", 5L);
        analysis.put("graphs", 2L);
        analysis.put("rounds", 6L);
        analysis.put("nodes", 90L);
        analysis.put("resolved_calls", resolved);
        analysis.put("unresolved_calls", unresolved);
        analysis.put("truncated", 1L);
        analysis.put("reflection", 0L);
        analysis.put("last_build_ns", buildNs);
        analysis.put("queue", 4L);
        analysis.put("budget_refusals", 0L);
        analysis.put("descriptors", 2L);
        analysis.put("completed", 3L);
        analysis.put("descriptors_truncated", 1L);
        analysis.put("descriptors_unknown", 2L);
        analysis.put("descriptor_changes", 1L);
        analysis.put("recomputes", 1L);
        analysis.put("step_ns", 2_500_000L);
        return analysis;
    }

    @Test
    @DisplayName("C-07 : rien des graphes avant la hiérarchie, puis la part non résolue")
    void theCallGraphIsPublishedOnceTheHierarchyIsKnown() {
        Map<String, Object> status = status();
        status.put("analysis", analysis(0L, 0L, 0L, 0L));
        MetricSet set = Telemetry.collect(status, null, null, null);
        assertEquals(0, set.get("rfx.callgraph.hierarchy_classes").value());
        assertNull(set.get("rfx.callgraph.graphs"), "hiérarchie pas encore déclarée");

        status.put("analysis", analysis(52_000L, 0L, 0L, 0L));
        set = Telemetry.collect(status, null, null, null);
        assertEquals(2, set.get("rfx.callgraph.graphs").value());
        assertNull(set.get("rfx.callgraph.unresolved_ratio"), "aucun site d'appel encore");
        assertNull(set.get("rfx.callgraph.build_ms"), "aucune construction mesurée");

        status.put("analysis", analysis(52_000L, 7L, 3L, 1_500_000L));
        set = Telemetry.collect(status, null, null, null);
        assertEquals(30.0, set.get("rfx.callgraph.unresolved_ratio").value(), 1e-9);
        assertEquals(1.5, set.get("rfx.callgraph.build_ms").value(), 1e-9);
        assertEquals(7, set.get("rfx.callgraph.resolved_calls").value());
        assertEquals(5, set.get("rfx.callgraph.classes_absent").value());
        assertEquals(4, set.get("rfx.analysis.queue_depth").value());
        assertEquals(3, set.get("rfx.analysis.completed").value());
        assertEquals(1, set.get("rfx.analysis.truncated").value());
        assertEquals(2.5, set.get("rfx.analysis.cpu_ms").value(), 1e-9);
        assertEquals(2, set.get("rfx.analysis.unknown").value());
        assertEquals(1, set.get("rfx.analysis.recomputes").value());

        Map<String, Object> disabled = analysis(52_000L, 7L, 3L, 0L);
        disabled.put("enabled", false);
        status.put("analysis", disabled);
        set = Telemetry.collect(status, null, null, null);
        assertNull(set.get("rfx.callgraph.hierarchy_classes"), "analysis.enabled = false");
    }

    @Test
    @DisplayName("C-09 : une base désactivée ne publie rien")
    void aDisabledDatabasePublishesNothing() {
        Map<String, Object> status = status();
        Map<String, Object> disabled = openDb(0L, 0L);
        disabled.put("enabled", false);
        disabled.put("open", false);
        status.put("db", disabled);
        MetricSet set = Telemetry.collect(status, null, null, null);
        assertNull(set.get("rfx.db.open_failures"));
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
