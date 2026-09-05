package dev.rustforgex.command;

import dev.rustforgex.bootstrap.Bootstrap;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.ErrorCode;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.contents.TranslatableContents;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Test T-420 : {@code /rfx status} rend compte de l'état réel, sans rien inventer, et
 * n'affiche aucun texte codé en dur.
 *
 * <p>Les composants sont inspectés par leur clé de traduction plutôt que par leur
 * rendu : le rendu exige le chargement du fichier de langue par le jeu, alors que la
 * clé est exactement ce que ce composant doit garantir.
 */
class StatusReportTest {

    private static Bootstrap.Report readyReport() {
        return new Bootstrap.Report(
                Bootstrap.State.READY, null, "Runtime natif prêt (rfx_native.dll).",
                0x5246_5800_0000_0001L, Path.of("native", "rfx_native.dll"), 42, List.of());
    }

    private static Map<String, Object> nativeStatus(boolean costsMeasured, boolean l3Measured) {
        return nativeStatus(costsMeasured, l3Measured, profiler("LIGHT", 0L, 0L, 0L));
    }

    /** Compteurs du profiler (C-05), tels que le natif les publie. */
    private static Map<String, Object> profiler(
            String level, long workloads, long recordsIngested, long evictions) {
        Map<String, Object> profiler = new LinkedHashMap<>();
        profiler.put("level", level);
        profiler.put("workloads_tracked", workloads);
        profiler.put("probes_allocated", workloads);
        profiler.put("overhead_pct_x100", 42L);
        profiler.put("mspt_pct_x100", 7L);
        profiler.put("records_ingested", recordsIngested);
        profiler.put("records_unknown", 0L);
        profiler.put("records_ignored", 0L);
        profiler.put("samples_ingested", 0L);
        profiler.put("evictions", evictions);
        profiler.put("collisions", 0L);
        profiler.put("zero_duration_exits", 0L);
        profiler.put("level_changes", 1L);
        // Aucune pause de mesure n'a encore abouti : c'est l'état d'un serveur pendant
        // ses cinq premières minutes (PARTIE 12.4).
        profiler.put("baseline_measurements", 0L);
        profiler.put("baseline_overhead_ns", 0L);
        profiler.put("baseline_overhead_pct_x100", 0L);
        profiler.put("baseline_tick", 0L);
        profiler.put("baseline_ticks_until_pause", 5_900L);
        return profiler;
    }

    private static Map<String, Object> nativeStatus(
            boolean costsMeasured, boolean l3Measured, Map<String, Object> profiler) {
        Map<String, Object> simd = new LinkedHashMap<>();
        simd.put("sse2", Boolean.TRUE);
        simd.put("avx2", Boolean.TRUE);
        simd.put("avx512", Boolean.FALSE);
        simd.put("neon", Boolean.FALSE);

        Map<String, Object> hardware = new LinkedHashMap<>();
        hardware.put("physical_cores", 8L);
        hardware.put("logical_cores", 16L);
        hardware.put("l3_bytes", l3Measured ? 33_554_432L : 0L);
        hardware.put("numa_nodes", 1L);
        hardware.put("mem_total_bytes", 34_359_738_368L);
        hardware.put("jni_call_ns", costsMeasured ? 250L : 0L);
        hardware.put("ffi_batch_ns_per_kb", costsMeasured ? 40L : 0L);
        hardware.put("simd", simd);

        Map<String, Object> coverage = new LinkedHashMap<>();
        coverage.put("cores", Boolean.TRUE);
        coverage.put("topology", Boolean.TRUE);
        coverage.put("l3", l3Measured);
        coverage.put("numa", Boolean.TRUE);
        coverage.put("simd", Boolean.TRUE);
        coverage.put("mem_total", Boolean.TRUE);
        coverage.put("ffi_call", costsMeasured);
        coverage.put("ffi_transfer", costsMeasured);

        Map<String, Object> component = new LinkedHashMap<>();
        component.put("id", "C-27");
        component.put("name", "Rust Runtime Core");
        component.put("maturity", "Stable");
        component.put("active", Boolean.TRUE);

        Map<String, Object> status = new LinkedHashMap<>();
        status.put("schema", 1L);
        status.put("abi_version", 1L);
        status.put("native_version", "0.1.0");
        status.put("state", "RUNNING");
        status.put("panics", 0L);
        status.put("hardware", hardware);
        status.put("probe_coverage", coverage);
        status.put("components", List.of(component));
        status.put("profiler", profiler);
        return status;
    }

    /** Extrait récursivement toutes les clés de traduction d'une liste de composants. */
    private static List<String> keysOf(List<Component> lines) {
        List<String> keys = new ArrayList<>();
        for (Component line : lines) {
            collectKeys(line, keys);
        }
        return keys;
    }

    private static void collectKeys(Component component, List<String> keys) {
        if (component.getContents() instanceof TranslatableContents translatable) {
            keys.add(translatable.getKey());
            for (Object argument : translatable.getArgs()) {
                if (argument instanceof Component nested) {
                    collectKeys(nested, keys);
                }
            }
        }
    }

    /** Extrait les arguments de la première ligne portant la clé donnée. */
    private static Object[] argumentsOf(List<Component> lines, String key) {
        for (Component line : lines) {
            if (line.getContents() instanceof TranslatableContents t && t.getKey().equals(key)) {
                return t.getArgs();
            }
        }
        throw new AssertionError("clé absente du rapport : " + key);
    }

    @Test
    @DisplayName("T-420 : le statut nominal expose l'état, le matériel et les composants")
    void nominalStatus() {
        List<Component> lines = StatusReport.compose(
                readyReport(), Configuration.defaults(), nativeStatus(true, true), false);
        List<String> keys = keysOf(lines);

        assertTrue(keys.contains("rustforgex.status.title"), keys.toString());
        assertTrue(keys.contains("rustforgex.state.ready"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.cores"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.memory"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.l3"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.simd"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.ffi_call"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.ffi_transfer"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.component"), keys.toString());

        assertArrayEqualsAsStrings(new Object[] {8L, 16L}, argumentsOf(lines, "rustforgex.status.cores"));
        assertArrayEqualsAsStrings(new Object[] {32L}, argumentsOf(lines, "rustforgex.status.memory"));
        assertArrayEqualsAsStrings(new Object[] {32768L}, argumentsOf(lines, "rustforgex.status.l3"));
        assertArrayEqualsAsStrings(new Object[] {250L}, argumentsOf(lines, "rustforgex.status.ffi_call"));
        assertArrayEqualsAsStrings(new Object[] {"SSE2 AVX2"}, argumentsOf(lines, "rustforgex.status.simd"));
    }

    private static void assertArrayEqualsAsStrings(Object[] expected, Object[] actual) {
        assertEquals(expected.length, actual.length, "nombre d'arguments");
        for (int i = 0; i < expected.length; i++) {
            assertEquals(String.valueOf(expected[i]), String.valueOf(actual[i]), "argument " + i);
        }
    }

    @Test
    @DisplayName("R-660 : un coût non mesuré utilise une clé distincte, jamais un zéro")
    void unmeasuredCostsUseTheirOwnKey() {
        List<String> keys = keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(), nativeStatus(false, false), false));

        assertTrue(keys.contains("rustforgex.status.ffi_call_unknown"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.ffi_transfer_unknown"), keys.toString());
        assertFalse(keys.contains("rustforgex.status.ffi_call"), "aucun zéro ne doit passer pour une mesure");
        assertFalse(keys.contains("rustforgex.status.l3"), "un L3 non sondé ne doit pas être affiché");
    }

    @Test
    @DisplayName("C-05 : le profiler est exposé avec son niveau et ses unités suivies")
    void profilerIsReported() {
        List<Component> lines = StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, profiler("NORMAL", 12L, 4_000L, 0L)), false);

        assertTrue(keysOf(lines).contains("rustforgex.status.profiler"), keysOf(lines).toString());
        assertArrayEqualsAsStrings(
                new Object[] {"NORMAL", 12L}, argumentsOf(lines, "rustforgex.status.profiler"));
        assertArrayEqualsAsStrings(
                new Object[] {"0.42", "0.07"},
                argumentsOf(lines, "rustforgex.status.profiler_overhead"));
    }

    @Test
    @DisplayName("R-660 : un coût de profilage jamais mesuré n'est pas affiché comme nul")
    void unmeasuredProfilingCostUsesItsOwnKey() {
        List<String> keys = keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, profiler("OFF", 0L, 0L, 0L)), false));

        assertTrue(keys.contains("rustforgex.status.profiler_overhead_unknown"), keys.toString());
        assertFalse(keys.contains("rustforgex.status.profiler_overhead"),
                "un profiler qui n'a rien vu passer n'a pas mesuré 0,00 %");
    }

    /** Remplace les compteurs de ligne de base par ceux d'une mesure aboutie. */
    private static Map<String, Object> measuredBaseline(Map<String, Object> profiler) {
        profiler.put("baseline_measurements", 3L);
        profiler.put("baseline_overhead_ns", 850_000L);
        profiler.put("baseline_overhead_pct_x100", 170L);
        profiler.put("baseline_tick", 18_000L);
        profiler.put("baseline_ticks_until_pause", 4_200L);
        return profiler;
    }

    @Test
    @DisplayName("PARTIE 12.4 : le coût mesuré par mise en pause est affiché")
    void measuredBaselineCostIsReported() {
        List<Component> lines = StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, measuredBaseline(profiler("NORMAL", 12L, 4_000L, 0L))),
                false);

        assertTrue(keysOf(lines).contains("rustforgex.status.profiler_baseline"),
                keysOf(lines).toString());
        assertArrayEqualsAsStrings(
                new Object[] {"850.000", "1.70", 18_000L},
                argumentsOf(lines, "rustforgex.status.profiler_baseline"));
    }

    @Test
    @DisplayName("R-770 : tant qu'aucune pause n'a abouti, le coût n'est pas annoncé nul")
    void pendingBaselineAnnouncesTheWaitRatherThanZero() {
        List<Component> lines = StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, profiler("LIGHT", 3L, 100L, 0L)), false);
        List<String> keys = keysOf(lines);

        assertTrue(keys.contains("rustforgex.status.profiler_baseline_pending"), keys.toString());
        assertFalse(keys.contains("rustforgex.status.profiler_baseline"),
                "ne pas savoir ce qu'on coûte n'est pas coûter zéro");
        assertArrayEqualsAsStrings(
                new Object[] {5_900L},
                argumentsOf(lines, "rustforgex.status.profiler_baseline_pending"));
    }

    @Test
    @DisplayName("Les anomalies du profiler ne s'affichent que lorsqu'elles se produisent")
    void profilerAnomaliesAreOnlyShownWhenTheyHappen() {
        List<String> quiet = keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, profiler("NORMAL", 3L, 100L, 0L)), false));
        assertFalse(quiet.contains("rustforgex.status.profiler_anomalies"), quiet.toString());

        List<Component> noisy = StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, profiler("NORMAL", 3L, 100L, 5L)), false);
        assertTrue(keysOf(noisy).contains("rustforgex.status.profiler_anomalies"));
        assertArrayEqualsAsStrings(
                new Object[] {5L, 0L, 0L},
                argumentsOf(noisy, "rustforgex.status.profiler_anomalies"));
    }

    @Test
    @DisplayName("Le mode dégradé est signalé avec sa cause")
    void degradedStatus() {
        Bootstrap.Report degraded = new Bootstrap.Report(
                Bootstrap.State.DEGRADED, ErrorCode.NATIVE_MISSING,
                "Aucun binaire natif embarqué pour linux-aarch64.",
                0, null, 7, List.of());

        List<Component> lines = StatusReport.compose(degraded, Configuration.defaults(), null, false);
        List<String> keys = keysOf(lines);

        assertTrue(keys.contains("rustforgex.state.degraded"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.cause"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.native_inactive"), keys.toString());
        assertArrayEqualsAsStrings(
                new Object[] {"E-1005", ErrorCode.NATIVE_MISSING.description()},
                argumentsOf(lines, "rustforgex.status.cause"));
    }

    @Test
    @DisplayName("FM-01 : l'observation seule est signalée avec sa cause")
    void observeOnlyStatus() {
        Bootstrap.Report outOfRange = new Bootstrap.Report(
                Bootstrap.State.DEGRADED, ErrorCode.FORGE_OUT_OF_RANGE,
                "Version de Forge « 48.0.1 » hors de la plage supportée [47,48).",
                0, null, 1, List.of());

        List<String> keys = keysOf(
                StatusReport.compose(outOfRange, Configuration.defaults(), null, true));

        assertTrue(keys.contains("rustforgex.state.observe_only"), keys.toString());
        assertTrue(keys.contains("rustforgex.status.cause"), keys.toString());
    }

    @Test
    @DisplayName("Un statut natif illisible ne fait pas échouer la commande")
    void unreadableNativeStatusIsTolerated() {
        List<Component> lines =
                StatusReport.compose(readyReport(), Configuration.defaults(), null, false);

        assertFalse(lines.isEmpty());
        assertTrue(keysOf(lines).contains("rustforgex.status.native_inactive"));
    }

    @Test
    @DisplayName("Toute clé utilisée est traduite en français ET en anglais")
    void everyKeyIsTranslatedInBothLanguages() throws IOException {
        List<String> used = new ArrayList<>();
        used.addAll(keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(), nativeStatus(true, true), false)));
        used.addAll(keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(), nativeStatus(false, false), false)));
        used.addAll(keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(), null, true)));
        used.addAll(keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, profiler("NORMAL", 3L, 100L, 5L)), false)));
        used.addAll(keysOf(StatusReport.compose(
                readyReport(), Configuration.defaults(),
                nativeStatus(true, true, measuredBaseline(profiler("NORMAL", 3L, 100L, 5L))),
                false)));
        used.add("rustforgex.status.not_started");
        used.add("rustforgex.state.disabled");

        String french = readLangFile("fr_fr.json");
        String english = readLangFile("en_us.json");

        for (String key : used) {
            assertTrue(french.contains('"' + key + '"'), key + " absente de fr_fr.json");
            assertTrue(english.contains('"' + key + '"'), key + " absente de en_us.json");
        }
    }

    private static String readLangFile(String name) throws IOException {
        Path file = Path.of("src/main/resources/assets/rustforgex/lang", name).toAbsolutePath();
        assertTrue(Files.isRegularFile(file), "fichier de langue introuvable : " + file);
        return Files.readString(file, StandardCharsets.UTF_8);
    }
}
