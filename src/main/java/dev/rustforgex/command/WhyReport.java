package dev.rustforgex.command;

import dev.rustforgex.instrument.ProbeRegistry;
import net.minecraft.network.chat.Component;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * C-38 : composition de la fiche affichée par {@code /rfx why} et {@code /rfx workload}
 * (C-35).
 *
 * <p>Cahier des charges : PARTIE 5.33 et 5.36. Décision : ADR-063. Maturité :
 * {@code STABLE}.
 *
 * <p>Trois blocs, comme la fiche normative : ce qui est mesuré, ce que l'analyse en sait,
 * et la décision. La décision est calculée par le natif (risque C-14, compatibilité
 * C-33, décision C-15) et expliquée, mais n'est pas appliquée : c'est le jalon M3
 * (ADR-068). Une unité {@code UNKNOWN} dit pourquoi : quels faits manquent ou restent
 * inconnus.
 *
 * <p>Séparé de la commande pour être testable sans démarrer Minecraft ; aucun appel
 * natif ici.
 */
public final class WhyReport {

    /** Préfixe commun à toutes les clés de traduction de la fiche. */
    public static final String KEY_PREFIX = "rustforgex.why.";

    private WhyReport() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Lit un identifiant d'unité tel que {@code /rfx top} le propose : seize chiffres
     * hexadécimaux au plus, préfixe {@code 0x} admis.
     *
     * @param text saisie de l'opérateur
     * @return les 64 bits de l'identifiant, ou {@code null} si la saisie n'en est pas un
     */
    public static Long parseWorkId(String text) {
        if (text == null) {
            return null;
        }
        String digits = text.trim().toLowerCase(Locale.ROOT);
        if (digits.startsWith("0x")) {
            digits = digits.substring(2);
        }
        if (digits.isEmpty() || digits.length() > 16) {
            return null;
        }
        try {
            return Long.parseUnsignedLong(digits, 16);
        } catch (NumberFormatException e) {
            return null;
        }
    }

    /**
     * Compose la fiche.
     *
     * @param card fiche décodée, ou {@code null} si le natif n'a rien rendu
     * @param registry registre des sondes, pour nommer l'unité ; peut être {@code null}
     * @param detailed {@code true} pour {@code /rfx workload} : domaines, sources et
     *     drapeaux en entier
     * @return les lignes à afficher
     */
    public static List<Component> lines(Map<String, Object> card, ProbeRegistry registry,
            boolean detailed) {
        if (card == null) {
            return List.of(Component.translatable(KEY_PREFIX + "unavailable"));
        }
        List<Component> lines = new ArrayList<>();
        Map<String, Object> measured = mapOf(card, "measured");
        int probeId = measured == null ? -1 : (int) longOf(measured, "probe_id");
        String owner = registry == null || probeId < 0 ? null : registry.ownerOf(probeId);
        String name = registry == null || probeId < 0 ? null : registry.nameOf(probeId);
        lines.add(Component.translatable(KEY_PREFIX + "header",
                Component.literal(stringOf(card, "work_id_hex")),
                Component.literal(owner == null ? "?" : owner),
                Component.literal(name == null ? "?" : name)));

        appendMeasured(lines, measured, longOf(card, "tick_ns"));
        appendAnalysis(lines, card, detailed);
        appendDecision(lines, mapOf(card, "decision"));
        return lines;
    }

    /**
     * Bloc {@code DECISION} : stratégie retenue, risque, compatibilité, genres rejetés
     * avec leurs motifs. Rien n'est appliqué en M3, et la fiche le dit.
     */
    private static void appendDecision(List<Component> lines, Map<String, Object> decision) {
        lines.add(Component.translatable(KEY_PREFIX + "decision_title"));
        if (decision == null) {
            lines.add(Component.translatable(KEY_PREFIX + "decision_none"));
            return;
        }
        lines.add(Component.translatable(KEY_PREFIX + "decision_strategy",
                Component.literal(stringOf(decision, "strategy")),
                longOf(decision, "level"),
                Component.literal(stringOf(decision, "mode")),
                Component.literal(String.format(Locale.ROOT, "%.1f",
                        longOf(decision, "expected_gain_ns") / 1_000.0))));
        lines.add(Component.translatable(KEY_PREFIX + "decision_risk",
                Component.literal(perMille(decision, "correctness_x1000")),
                Component.literal(perMille(decision, "stability_x1000")),
                Component.literal(perMille(decision, "compat_x1000")),
                Component.literal(perMille(decision, "data_x1000")),
                Component.literal(stringOf(decision, "blast")),
                longOf(decision, "max_level"),
                Component.literal(join(decision.get("risk_rules")))));
        lines.add(Component.translatable(KEY_PREFIX + "decision_compat",
                Component.literal(stringOf(decision, "compat_verdict")),
                Component.literal(stringOf(decision, "compat_source"))));
        if (decision.get("rejected") instanceof List<?> rejected) {
            for (Object item : rejected) {
                if (item instanceof Map<?, ?> entry) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> r = (Map<String, Object>) entry;
                    lines.add(Component.translatable(KEY_PREFIX + "decision_rejected",
                            Component.literal(stringOf(r, "strategy")),
                            Component.literal(join(r.get("reasons")))));
                }
            }
        }
        lines.add(Component.translatable(KEY_PREFIX + "decision_not_applied"));
    }

    private static String perMille(Map<String, Object> table, String key) {
        return String.format(Locale.ROOT, "%.3f", longOf(table, key) / 1000.0);
    }

    private static void appendMeasured(List<Component> lines, Map<String, Object> measured,
            long tickNs) {
        lines.add(Component.translatable(KEY_PREFIX + "measured_title"));
        if (measured == null) {
            lines.add(Component.translatable(KEY_PREFIX + "measured_none"));
            return;
        }
        long cost = TopReport.rankingCost(measured);
        lines.add(Component.translatable(KEY_PREFIX + "measured",
                Component.literal(String.format(Locale.ROOT, "%.1f",
                        longOf(measured, "calls_per_tick_x100") / 100.0)),
                Component.literal(TopReport.microseconds(longOf(measured, "cpu_ns_p50"))),
                Component.literal(TopReport.microseconds(longOf(measured, "cpu_ns_p95"))),
                Component.literal(TopReport.share(cost, tickNs)),
                Component.literal(stringOf(measured, "heat")),
                Component.literal(TopReport.source(measured))));
    }

    private static void appendAnalysis(List<Component> lines, Map<String, Object> card,
            boolean detailed) {
        lines.add(Component.translatable(KEY_PREFIX + "analysis_title"));
        if (Boolean.TRUE.equals(card.get("analysis_busy"))) {
            lines.add(Component.translatable(KEY_PREFIX + "analysis_busy"));
            return;
        }
        Map<String, Object> analysis = mapOf(card, "analysis");
        if (analysis == null) {
            lines.add(Component.translatable(KEY_PREFIX + "analysis_none"));
            return;
        }
        lines.add(Component.translatable(KEY_PREFIX + "graph",
                longOf(analysis, "graph_nodes"), longOf(analysis, "resolved_calls"),
                longOf(analysis, "unresolved_calls")));
        lines.add(Component.translatable(KEY_PREFIX + "reads",
                Component.literal(domains(analysis, "reads", detailed)),
                Component.literal(stringOf(analysis, "reads_precision"))));
        lines.add(Component.translatable(KEY_PREFIX + "writes",
                Component.literal(domains(analysis, "writes", detailed)),
                Component.literal(stringOf(analysis, "writes_precision"))));
        lines.add(Component.translatable(KEY_PREFIX + "thread",
                Component.literal(stringOf(analysis, "main_thread_required"))));
        lines.add(Component.translatable(KEY_PREFIX + "determinism",
                Component.literal(stringOf(analysis, "deterministic")),
                Component.literal(stringOf(analysis, "order_sensitive"))));
        if (detailed) {
            lines.add(Component.translatable(KEY_PREFIX + "effects",
                    Component.literal(join(analysis.get("side_effects")))));
            lines.add(Component.translatable(KEY_PREFIX + "sources",
                    Component.literal(join(analysis.get("nondet_sources")))));
            lines.add(Component.translatable(KEY_PREFIX + "deps",
                    Component.literal(countOrUnknown(analysis, "data_deps")),
                    Component.literal(countOrUnknown(analysis, "order_deps"))));
        }
        lines.add(Component.translatable(KEY_PREFIX + "flags",
                Component.literal(join(analysis.get("flags"))),
                Component.literal(String.format(Locale.ROOT, "%.3f",
                        longOf(analysis, "confidence_x1000") / 1000.0))));
        List<Component> reasons = unknownReasons(analysis);
        if (!reasons.isEmpty()) {
            lines.add(Component.translatable(KEY_PREFIX + "unknown_because"));
            lines.addAll(reasons);
        }
        if (Boolean.TRUE.equals(analysis.get("stale"))) {
            lines.add(Component.translatable(KEY_PREFIX + "stale"));
        }
    }

    /**
     * Ce qui maintient une unité en {@code UNKNOWN} : chaque fait manquant ou inconnu,
     * dans l'ordre des analyses.
     *
     * <p>Visible dans le paquet pour les tests.
     */
    static List<Component> unknownReasons(Map<String, Object> analysis) {
        List<Component> reasons = new ArrayList<>();
        if (longOf(analysis, "unresolved_calls") > 0) {
            reasons.add(reason("unresolved", longOf(analysis, "unresolved_calls")));
        }
        if (Boolean.TRUE.equals(analysis.get("truncated"))) {
            reasons.add(reason("truncated"));
        }
        if ("Unknown".equals(stringOf(analysis, "reads_precision"))) {
            reasons.add(reason("reads"));
        }
        if ("Unknown".equals(stringOf(analysis, "writes_precision"))) {
            reasons.add(reason("writes"));
        }
        if (Boolean.TRUE.equals(analysis.get("side_effects_unknown"))) {
            reasons.add(reason("effects"));
        }
        if (!analysis.containsKey("data_deps")) {
            reasons.add(reason("data_deps"));
        }
        if (!analysis.containsKey("order_deps")) {
            reasons.add(Boolean.TRUE.equals(analysis.get("may_be_dispatched"))
                    ? reason("dispatched") : reason("order_deps"));
        }
        if (!Boolean.TRUE.equals(analysis.get("external_locks_known"))) {
            reasons.add(reason("locks"));
        }
        if ("Unknown".equals(stringOf(analysis, "deterministic"))) {
            reasons.add(reason("determinism"));
        }
        return reasons;
    }

    private static Component reason(String key, Object... args) {
        return Component.translatable(KEY_PREFIX + "reason." + key, args);
    }

    private static String domains(Map<String, Object> analysis, String key,
            boolean detailed) {
        Object value = analysis.get(key);
        if (!(value instanceof List<?> list) || list.isEmpty()) {
            return "-";
        }
        if (detailed || list.size() <= 4) {
            return join(list);
        }
        return join(list.subList(0, 4)) + ", +" + (list.size() - 4);
    }

    private static String countOrUnknown(Map<String, Object> analysis, String key) {
        return analysis.containsKey(key) ? Long.toString(longOf(analysis, key)) : "?";
    }

    private static String join(Object value) {
        if (!(value instanceof List<?> list) || list.isEmpty()) {
            return "-";
        }
        StringBuilder text = new StringBuilder();
        for (Object item : list) {
            if (!text.isEmpty()) {
                text.append(", ");
            }
            text.append(item);
        }
        return text.toString();
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> mapOf(Map<String, Object> table, String key) {
        Object value = table.get(key);
        return value instanceof Map<?, ?> map ? (Map<String, Object>) map : null;
    }

    private static long longOf(Map<String, Object> table, String key) {
        return table.get(key) instanceof Long number ? number : 0L;
    }

    private static String stringOf(Map<String, Object> table, String key) {
        return table.get(key) instanceof String text ? text : "?";
    }
}
