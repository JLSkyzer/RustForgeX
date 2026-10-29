package dev.rustforgex.command;

import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.contents.TranslatableContents;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests de {@code /rfx why} et {@code /rfx workload} (C-35, C-38, ADR-063).
 *
 * <p>Sans Minecraft ni runtime natif : la fiche met en forme un CBOR déjà décodé, avec
 * les clés et les types que le natif écrit (entiers en {@code Long}, textes, booléens).
 */
class WhyReportTest {

    private static Map<String, Object> measured() {
        Map<String, Object> e = new LinkedHashMap<>();
        e.put("probe_id", 3L);
        e.put("work_id_hex", "ffffffffffffffff");
        e.put("window_ns_per_tick", 1_000_000L);
        e.put("source", "Probe");
        e.put("calls_per_tick_x100", 250L);
        e.put("cpu_ns_p50", 1_500L);
        e.put("cpu_ns_p95", 4_000L);
        e.put("heat", "HOT");
        return e;
    }

    /** Une analyse complète : rien n'y est inconnu. */
    private static Map<String, Object> knownAnalysis() {
        Map<String, Object> a = new LinkedHashMap<>();
        a.put("graph_nodes", 4L);
        a.put("resolved_calls", 3L);
        a.put("unresolved_calls", 0L);
        a.put("truncated", false);
        a.put("reads", List.of("EntityState"));
        a.put("reads_precision", "Exact");
        a.put("writes", List.of());
        a.put("writes_precision", "Exact");
        a.put("side_effects", List.of());
        a.put("side_effects_unknown", false);
        a.put("data_deps", 0L);
        a.put("order_deps", 0L);
        a.put("external_locks_known", true);
        a.put("may_be_dispatched", false);
        a.put("main_thread_required", "No");
        a.put("deterministic", "Yes");
        a.put("nondet_sources", List.of());
        a.put("order_sensitive", "No");
        a.put("flags", List.of("PURE"));
        a.put("confidence_x1000", 900L);
        a.put("stale", false);
        return a;
    }

    private static Map<String, Object> card(Map<String, Object> measured,
            Map<String, Object> analysis, boolean busy) {
        Map<String, Object> c = new LinkedHashMap<>();
        c.put("schema", 1L);
        c.put("work_id_hex", "ffffffffffffffff");
        c.put("tick_ns", 20_000_000L);
        if (measured != null) {
            c.put("measured", measured);
        }
        if (analysis != null) {
            c.put("analysis", analysis);
        }
        c.put("analysis_busy", busy);
        return c;
    }

    private static String flatten(List<Component> lines) {
        StringBuilder text = new StringBuilder();
        for (Component line : lines) {
            text.append(line.toString()).append('\n');
        }
        return text.toString();
    }

    @Test
    @DisplayName("Un identifiant se lit en hexadécimal non signé, sur 64 bits")
    void aWorkIdIsReadAsUnsignedHexadecimal() {
        assertEquals(-1L, WhyReport.parseWorkId("ffffffffffffffff"));
        assertEquals(-1L, WhyReport.parseWorkId("0xFFFFFFFFFFFFFFFF"));
        assertEquals(0x2aL, WhyReport.parseWorkId(" 2a "));
        assertEquals(0L, WhyReport.parseWorkId("0"));
    }

    @Test
    @DisplayName("Une saisie qui n'est pas un identifiant est refusée, pas tronquée")
    void aNonIdentifierIsRefused() {
        assertNull(WhyReport.parseWorkId(null));
        assertNull(WhyReport.parseWorkId(""));
        assertNull(WhyReport.parseWorkId("0x"));
        assertNull(WhyReport.parseWorkId("12345678901234567"), "17 chiffres : trop long");
        assertNull(WhyReport.parseWorkId("xyz"));
        assertNull(WhyReport.parseWorkId("-1"));
    }

    @Test
    @DisplayName("Une fiche absente est dite indisponible")
    void anAbsentCardIsUnavailable() {
        List<Component> lines = WhyReport.lines(null, null, false);

        assertEquals(1, lines.size());
        assertTrue(flatten(lines).contains(WhyReport.KEY_PREFIX + "unavailable"));
    }

    /**
     * ADR-068 : la décision calculée s'affiche avec son risque, son verdict et chaque
     * genre rejeté avec ses motifs, et la fiche dit qu'elle n'est pas appliquée.
     */
    @Test
    @DisplayName("La décision calculée s'affiche, motifs compris, comme non appliquée")
    void theComputedDecisionIsShownAsNotApplied() {
        Map<String, Object> rejected = new LinkedHashMap<>();
        rejected.put("strategy", "JAVA_BATCH");
        rejected.put("reasons", List.of("UnsafeSharedState"));
        Map<String, Object> decision = new LinkedHashMap<>();
        decision.put("mode", "Balanced");
        decision.put("strategy", "JAVA_ONLY");
        decision.put("level", 0L);
        decision.put("expected_gain_ns", 0L);
        decision.put("correctness_x1000", 100L);
        decision.put("stability_x1000", 800L);
        decision.put("compat_x1000", 0L);
        decision.put("data_x1000", 1_000L);
        decision.put("blast", "WORLD");
        decision.put("max_level", 2L);
        decision.put("risk_rules", List.of("R4", "R5", "R10"));
        decision.put("compat_verdict", "ALLOW");
        decision.put("compat_source", "Default");
        decision.put("rejected", List.of(rejected));
        decision.put("requires_validation", false);
        Map<String, Object> card = card(measured(), knownAnalysis(), false);
        card.put("decision", decision);

        String text = flatten(WhyReport.lines(card, null, false));

        assertTrue(text.contains(WhyReport.KEY_PREFIX + "decision_strategy"), text);
        assertTrue(text.contains("JAVA_ONLY"), text);
        assertTrue(text.contains("0.800"), "stabilité en millièmes : " + text);
        assertTrue(text.contains("R4, R5, R10"), text);
        assertTrue(text.contains(WhyReport.KEY_PREFIX + "decision_rejected"), text);
        assertTrue(text.contains("UnsafeSharedState"), text);
        assertTrue(text.contains(WhyReport.KEY_PREFIX + "decision_not_applied"), text);
        assertFalse(text.contains(WhyReport.KEY_PREFIX + "decision_none"), text);
    }

    /** Sans bloc de décision (unité non décrite), la fiche le dit. */
    @Test
    @DisplayName("Sans bloc de décision, la fiche dit qu'il n'y en a pas")
    void theCardSaysNoDecisionIsTaken() {
        String text = flatten(WhyReport.lines(card(measured(), knownAnalysis(), false),
                null, false));

        assertTrue(text.contains(WhyReport.KEY_PREFIX + "decision_none"), text);
        assertTrue(text.contains(WhyReport.KEY_PREFIX + "measured'"), text);
        assertTrue(text.contains("PURE"), text);
        assertTrue(text.contains("0.900"), "confiance en millièmes : " + text);
        assertTrue(text.contains("sonde"), "provenance traduite comme dans /rfx top : " + text);
        assertFalse(text.contains(WhyReport.KEY_PREFIX + "unknown_because"),
                "une analyse complète n'a rien d'inconnu : " + text);
    }

    @Test
    @DisplayName("Ni mesurée ni analysée : la fiche le dit, bloc par bloc")
    void anUnknownUnitSaysWhatIsMissing() {
        String text = flatten(WhyReport.lines(card(null, null, false), null, false));

        assertTrue(text.contains(WhyReport.KEY_PREFIX + "measured_none"), text);
        assertTrue(text.contains(WhyReport.KEY_PREFIX + "analysis_none"), text);
    }

    @Test
    @DisplayName("Une analyse occupée est dite, sans détail périmé")
    void aBusyAnalysisIsSaid() {
        String text = flatten(WhyReport.lines(card(measured(), knownAnalysis(), true),
                null, true));

        assertTrue(text.contains(WhyReport.KEY_PREFIX + "analysis_busy"), text);
        assertFalse(text.contains(WhyReport.KEY_PREFIX + "graph"), text);
    }

    @Test
    @DisplayName("Le détail n'apparaît qu'avec /rfx workload")
    void detailOnlyWithWorkload() {
        Map<String, Object> card = card(measured(), knownAnalysis(), false);

        assertFalse(flatten(WhyReport.lines(card, null, false))
                .contains(WhyReport.KEY_PREFIX + "deps"));
        assertTrue(flatten(WhyReport.lines(card, null, true))
                .contains(WhyReport.KEY_PREFIX + "deps"));
    }

    @Test
    @DisplayName("Une unité UNKNOWN dit chacun des faits qui lui manquent")
    void everyMissingFactIsAReason() {
        Map<String, Object> analysis = knownAnalysis();
        analysis.put("unresolved_calls", 2L);
        analysis.put("truncated", true);
        analysis.put("reads_precision", "Unknown");
        analysis.put("writes_precision", "Unknown");
        analysis.put("side_effects_unknown", true);
        analysis.remove("data_deps");
        analysis.remove("order_deps");
        analysis.put("external_locks_known", false);
        analysis.put("deterministic", "Unknown");

        String text = flatten(WhyReport.unknownReasons(analysis));

        for (String reason : List.of("unresolved", "truncated", "reads", "writes", "effects",
                "data_deps", "order_deps", "locks", "determinism")) {
            assertTrue(text.contains(WhyReport.KEY_PREFIX + "reason." + reason + "'"),
                    reason + " absent : " + text);
        }
        assertTrue(WhyReport.unknownReasons(knownAnalysis()).isEmpty());
    }

    /** ADR-060 : un rappel possible n'a pas d'order_deps, et la fiche dit pourquoi. */
    @Test
    @DisplayName("Un rappel possible est dit comme tel, pas comme un ordre inconnu")
    void aPossibleCallbackIsNamed() {
        Map<String, Object> analysis = knownAnalysis();
        analysis.remove("order_deps");
        analysis.put("may_be_dispatched", true);

        String text = flatten(WhyReport.unknownReasons(analysis));

        assertTrue(text.contains(WhyReport.KEY_PREFIX + "reason.dispatched"), text);
        assertFalse(text.contains(WhyReport.KEY_PREFIX + "reason.order_deps"), text);
    }

    @Test
    @DisplayName("Dans /rfx top, le nom d'une unité propose /rfx why")
    void topProposesWhy() {
        Map<String, Object> entry = measured();
        entry.put("cost_ns_per_tick", 1_000_000L);
        Map<String, Object> top = new LinkedHashMap<>();
        top.put("schema", 2L);
        top.put("tracked", 1L);
        top.put("measured", 1L);
        top.put("tick_ns", 20_000_000L);
        top.put("entries", List.of(entry));

        boolean proposed = false;
        for (Component line : TopReport.lines(top, null)) {
            proposed |= suggests(line, "/rfx why ffffffffffffffff");
        }
        assertTrue(proposed, "aucun composant ne propose /rfx why");
    }

    private static boolean suggests(Component component, String command) {
        ClickEvent click = component.getStyle().getClickEvent();
        if (click != null && click.getAction() == ClickEvent.Action.SUGGEST_COMMAND
                && command.equals(click.getValue())) {
            return true;
        }
        if (component.getContents() instanceof TranslatableContents translatable) {
            for (Object arg : translatable.getArgs()) {
                if (arg instanceof Component child && suggests(child, command)) {
                    return true;
                }
            }
        }
        for (Component sibling : component.getSiblings()) {
            if (suggests(sibling, command)) {
                return true;
            }
        }
        return false;
    }
}
