package dev.rustforgex.command;

import dev.rustforgex.bootstrap.Bootstrap;
import dev.rustforgex.config.Configuration;
import net.minecraft.network.chat.Component;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * C-38 : composition du rapport affiché par {@code /rfx status}.
 *
 * <p>Cahier des charges : PARTIE 5.36. Test : T-420. Maturité : {@code STABLE}.
 *
 * <p>Aucun texte n'est codé en dur : chaque ligne est un {@link Component#translatable}
 * dont la clé est traduite dans {@code en_us.json} et {@code fr_fr.json}. La
 * composition est séparée de l'enregistrement de la commande afin d'être testable sans
 * démarrer Minecraft, et ne fait aucun appel natif : elle met en forme un statut déjà
 * lu, ce qui garantit que l'affichage ne peut pas bloquer le thread serveur (R-601).
 *
 * <p>Aucune valeur n'est inventée : un champ que la sonde n'a pas mesuré utilise une
 * clé « non mesuré » distincte, jamais un zéro qui passerait pour une mesure (R-660,
 * contrat agent 3.2).
 */
public final class StatusReport {

    /** Préfixe commun à toutes les clés de traduction du rapport. */
    public static final String KEY_PREFIX = "rustforgex.status.";

    private StatusReport() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Compose les lignes du rapport.
     *
     * @param report rapport de démarrage produit par C-02
     * @param configuration configuration effective
     * @param nativeStatus statut décodé publié par le runtime natif, ou {@code null}
     *     si le runtime n'est pas actif
     * @param observeOnly {@code true} si C-01 a imposé l'observation seule
     * @return les lignes à afficher, jamais vides
     */
    public static List<Component> compose(
            Bootstrap.Report report,
            Configuration configuration,
            Map<String, Object> nativeStatus,
            boolean observeOnly) {

        List<Component> lines = new ArrayList<>();
        lines.add(Component.translatable(KEY_PREFIX + "title"));
        lines.add(Component.translatable(KEY_PREFIX + "state", stateLabel(report, observeOnly)));
        lines.add(Component.translatable(KEY_PREFIX + "mode", configuration.mode()));
        lines.add(Component.translatable(KEY_PREFIX + "startup", report.durationMs()));

        if (report.code() != null) {
            lines.add(Component.translatable(
                    KEY_PREFIX + "cause", report.code().id(), report.code().description()));
        }
        if (report.library() != null) {
            lines.add(Component.translatable(
                    KEY_PREFIX + "binary", report.library().getFileName().toString()));
        }

        if (nativeStatus == null) {
            lines.add(Component.translatable(KEY_PREFIX + "native_inactive"));
            return lines;
        }

        lines.add(Component.translatable(KEY_PREFIX + "abi",
                longValue(nativeStatus, "abi_version"), stringValue(nativeStatus, "native_version")));
        lines.add(Component.translatable(KEY_PREFIX + "panics", longValue(nativeStatus, "panics")));

        Map<String, Object> hardware = mapValue(nativeStatus, "hardware");
        Map<String, Object> coverage = mapValue(nativeStatus, "probe_coverage");
        if (hardware != null) {
            appendHardware(lines, hardware, coverage);
        }

        Object components = nativeStatus.get("components");
        if (components instanceof List<?> list && !list.isEmpty()) {
            lines.add(Component.translatable(KEY_PREFIX + "components"));
            for (Object element : list) {
                if (element instanceof Map<?, ?> component) {
                    lines.add(describeComponent(component));
                }
            }
        }
        return lines;
    }

    /** Libellé d'état, sous forme de composant traduit. */
    private static Component stateLabel(Bootstrap.Report report, boolean observeOnly) {
        if (observeOnly) {
            return Component.translatable("rustforgex.state.observe_only");
        }
        return switch (report.state()) {
            case READY -> Component.translatable("rustforgex.state.ready");
            case DEGRADED -> Component.translatable("rustforgex.state.degraded");
            case DISABLED -> Component.translatable("rustforgex.state.disabled");
            default -> Component.literal(report.state().name());
        };
    }

    /** Ajoute les lignes décrivant la machine, en distinguant mesuré et non mesuré. */
    private static void appendHardware(
            List<Component> lines, Map<String, Object> hardware, Map<String, Object> coverage) {

        lines.add(Component.translatable(KEY_PREFIX + "cores",
                longValue(hardware, "physical_cores"), longValue(hardware, "logical_cores")));

        if (measured(coverage, "mem_total")) {
            long bytes = longValue(hardware, "mem_total_bytes");
            lines.add(Component.translatable(
                    KEY_PREFIX + "memory", bytes / (1024L * 1024L * 1024L)));
        } else {
            lines.add(Component.translatable(KEY_PREFIX + "memory_unknown"));
        }

        // Un champ non sondé n'est pas affiché du tout : mieux vaut une ligne absente
        // qu'une ligne annonçant zéro.
        if (measured(coverage, "l3")) {
            lines.add(Component.translatable(KEY_PREFIX + "l3", longValue(hardware, "l3_bytes") / 1024L));
        }
        if (measured(coverage, "numa")) {
            lines.add(Component.translatable(KEY_PREFIX + "numa", longValue(hardware, "numa_nodes")));
        }

        Map<String, Object> simd = mapValue(hardware, "simd");
        if (simd != null) {
            List<String> sets = new ArrayList<>();
            for (String name : List.of("sse2", "avx2", "avx512", "neon")) {
                if (Boolean.TRUE.equals(simd.get(name))) {
                    sets.add(name.toUpperCase(Locale.ROOT));
                }
            }
            lines.add(sets.isEmpty()
                    ? Component.translatable(KEY_PREFIX + "simd_none")
                    : Component.translatable(KEY_PREFIX + "simd", String.join(" ", sets)));
        }

        lines.add(measured(coverage, "ffi_call")
                ? Component.translatable(KEY_PREFIX + "ffi_call", longValue(hardware, "jni_call_ns"))
                : Component.translatable(KEY_PREFIX + "ffi_call_unknown"));
        lines.add(measured(coverage, "ffi_transfer")
                ? Component.translatable(
                        KEY_PREFIX + "ffi_transfer", longValue(hardware, "ffi_batch_ns_per_kb"))
                : Component.translatable(KEY_PREFIX + "ffi_transfer_unknown"));
    }

    private static Component describeComponent(Map<?, ?> component) {
        Component state = Boolean.TRUE.equals(component.get("active"))
                ? Component.translatable(KEY_PREFIX + "component_active")
                : Component.translatable(KEY_PREFIX + "component_inactive");
        return Component.translatable(KEY_PREFIX + "component",
                String.valueOf(component.get("id")),
                String.valueOf(component.get("name")),
                String.valueOf(component.get("maturity")),
                state);
    }

    private static boolean measured(Map<String, Object> coverage, String key) {
        return coverage != null && Boolean.TRUE.equals(coverage.get(key));
    }

    private static long longValue(Map<String, Object> table, String key) {
        Object v = table == null ? null : table.get(key);
        return v instanceof Long n ? n : 0L;
    }

    private static String stringValue(Map<String, Object> table, String key) {
        Object v = table == null ? null : table.get(key);
        return v instanceof String s ? s : "?";
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> mapValue(Map<String, Object> parent, String key) {
        Object v = parent == null ? null : parent.get(key);
        return v instanceof Map ? (Map<String, Object>) v : null;
    }
}
