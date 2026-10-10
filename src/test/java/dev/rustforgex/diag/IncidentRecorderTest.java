package dev.rustforgex.diag;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.rustforgex.forge.HookGuard;
import dev.rustforgex.telemetry.Anonymizer;
import dev.rustforgex.telemetry.Telemetry;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * T-411 : un incident laisse un dump exploitable pour reproduction (C-35, PARTIE 19.6).
 *
 * <p>« Exploitable » se vérifie champ par champ : un dump qui existe mais ne dit pas
 * où l'exception est née, ou sur quelle installation, ne permet de rien rejouer.
 */
class IncidentRecorderTest {

    /** Répertoire personnel fictif : il ne doit apparaître dans aucun dump (R-571). */
    private static final Path HOME = Path.of("/maison/joueur");

    @TempDir
    Path gameDir;

    private IncidentRecorder recorder(boolean enabled) {
        return new IncidentRecorder(gameDir.resolve("crash"),
                IncidentDump.versions("1.20.1", "47.4.23", "0.1.0"),
                () -> Telemetry.collect(null, null, null, null, null),
                Anonymizer.of(gameDir, HOME), Runnable::run, enabled);
    }

    /** Lève depuis une méthode nommée : sa trame doit figurer dans le dump. */
    private static void failingHookBody() {
        throw new IllegalStateException("état incohérent",
                new IOException("lecture impossible de " + HOME.resolve("saves/monde")));
    }

    private List<Path> dumps() throws IOException {
        Path crash = gameDir.resolve("crash");
        if (!Files.isDirectory(crash)) {
            return List.of();
        }
        try (Stream<Path> files = Files.list(crash)) {
            return files.sorted().toList();
        }
    }

    @Test
    @DisplayName("T-411 : une accroche désactivée laisse un seul dump, complet et anonymisé")
    void aDisabledHookLeavesOneReproducibleDump() throws Exception {
        IncidentRecorder recorder = recorder(true);
        List<CompletableFuture<Path>> pending = new ArrayList<>();
        HookGuard guard = new HookGuard("serverTickPost", message -> { },
                incident -> pending.add(recorder.record(incident)));

        for (int i = 0; i < HookGuard.DISABLE_THRESHOLD + 3; i++) {
            guard.run(IncidentRecorderTest::failingHookBody);
        }

        assertEquals(1, pending.size(), "la désactivation est un incident, et un seul");
        Path written = pending.get(0).get();
        assertEquals(List.of(written), dumps(), "aucun fichier temporaire ne doit rester");
        assertTrue(written.getFileName().toString().matches("rfx-crash-\\d+\\.json"),
                written.getFileName().toString());

        String text = Files.readString(written, StandardCharsets.UTF_8);
        assertFalse(text.contains(HOME.toString()), "aucun chemin personnel (R-571)");
        assertFalse(text.contains(gameDir.toString()), "aucun chemin d'installation (R-571)");

        JsonObject dump = JsonParser.parseString(text).getAsJsonObject();
        assertEquals(IncidentDump.SCHEMA, dump.get("schema").getAsInt());
        assertEquals("HOOK_DISABLED", dump.get("kind").getAsString());
        assertEquals("serverTickPost", dump.get("subject").getAsString());
        assertEquals(HookGuard.DISABLE_THRESHOLD, dump.get("occurrences").getAsInt());

        JsonObject versions = dump.getAsJsonObject("versions");
        for (String key : List.of("minecraft", "forge", "rustforgex", "java", "os", "arch", "cpus")) {
            assertTrue(versions.has(key) && !versions.get(key).getAsString().isBlank(),
                    "version manquante : " + key);
        }

        JsonArray trace = dump.getAsJsonArray("trace");
        assertEquals(2, trace.size(), "l'exception et sa cause");
        JsonObject thrown = trace.get(0).getAsJsonObject();
        assertEquals(IllegalStateException.class.getName(), thrown.get("class").getAsString());
        assertEquals("état incohérent", thrown.get("message").getAsString());
        assertTrue(thrown.getAsJsonArray("frames").get(0).getAsString()
                        .contains("IncidentRecorderTest.failingHookBody"),
                "la première trame désigne l'origine : " + thrown.getAsJsonArray("frames").get(0));
        JsonObject cause = trace.get(1).getAsJsonObject();
        assertEquals(IOException.class.getName(), cause.get("class").getAsString());
        assertTrue(cause.get("message").getAsString().contains(Anonymizer.HOME),
                "le chemin du message est remplacé, pas effacé : " + cause.get("message"));

        assertTrue(dump.get("metrics").isJsonObject(), "les métriques au moment de l'incident");
    }

    @Test
    @DisplayName("Une panic native répétée à chaque tick n'est consignée qu'une fois")
    void aRepeatedNativePanicIsRecordedOnce() throws Exception {
        IncidentRecorder recorder = recorder(true);

        Path first = recorder.record(Incident.nativePanic("tick")).get();
        for (int i = 0; i < 100; i++) {
            assertNull(recorder.record(Incident.nativePanic("tick")).get());
        }

        assertEquals(List.of(first), dumps());
        JsonObject dump = JsonParser.parseString(
                Files.readString(first, StandardCharsets.UTF_8)).getAsJsonObject();
        assertEquals("NATIVE_PANIC", dump.get("kind").getAsString());
        assertEquals("E-3001", dump.get("error_code").getAsString());
        assertEquals(0, dump.getAsJsonArray("trace").size(), "aucune trace Java pour le natif");
    }

    @Test
    @DisplayName("Le nombre de dumps par partie est plafonné")
    void dumpsAreCapped() throws Exception {
        IncidentRecorder recorder = recorder(true);

        for (int i = 0; i < IncidentRecorder.MAX_DUMPS + 10; i++) {
            recorder.record(Incident.hookDisabled("hook" + i, 5, new RuntimeException())).get();
        }

        assertEquals(IncidentRecorder.MAX_DUMPS, dumps().size());
    }

    @Test
    @DisplayName("diagnostics.report_on_incident = false : aucun fichier")
    void disabledRecorderWritesNothing() throws Exception {
        assertNull(recorder(false).record(Incident.nativePanic("tick")).get());
        assertEquals(List.of(), dumps());
    }

    @Test
    @DisplayName("Un relevé de métriques qui lève n'empêche pas le dump")
    void failingMetricsStillLeaveADump() throws Exception {
        IncidentRecorder recorder = new IncidentRecorder(gameDir.resolve("crash"),
                IncidentDump.versions("1.20.1", "47.4.23", "0.1.0"),
                () -> {
                    throw new IllegalStateException("runtime détruit");
                },
                Anonymizer.of(gameDir, HOME), Runnable::run, true);

        Path written = recorder.record(Incident.nativePanic("tick")).get();

        assertNotNull(written);
        assertTrue(JsonParser.parseString(Files.readString(written, StandardCharsets.UTF_8))
                .getAsJsonObject().get("metrics").isJsonNull());
    }

    @Test
    @DisplayName("Une consignation qui lève ne fait pas échouer la garde")
    void aFailingSinkDoesNotBreakTheGuard() {
        List<String> log = new ArrayList<>();
        HookGuard guard = new HookGuard("hook", log::add, incident -> {
            throw new IllegalStateException("disque plein");
        });

        for (int i = 0; i < HookGuard.DISABLE_THRESHOLD; i++) {
            assertFalse(guard.run(IncidentRecorderTest::failingHookBody));
        }

        assertTrue(guard.disabled());
        assertTrue(log.get(log.size() - 1).contains("incident non consigné"), log.toString());
    }
}
