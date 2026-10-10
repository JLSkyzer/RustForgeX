package dev.rustforgex.diag;

import dev.rustforgex.telemetry.Anonymizer;
import dev.rustforgex.telemetry.MetricSet;
import dev.rustforgex.telemetry.MetricsJson;

import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Set;

/**
 * C-35 : mise en forme d'un dump d'incident, {@code crash/rfx-crash-<ts>.json}.
 *
 * <p>Cahier des charges : PARTIE 5.33, PARTIE 19.6, R-571. Test : T-411. Maturité :
 * {@code STABLE}.
 *
 * <p>Un dump sert à reproduire : il dit ce qui a échoué, sur quelle installation, et
 * par où l'exception est passée. Il contient donc les versions de la PARTIE 19.6
 * (Minecraft, Forge, RUSTFORGE-X, JVM, système, processeur), la chaîne complète des
 * causes, et l'état des métriques au moment de l'incident. Les parties « unité de
 * travail » de la PARTIE 19.6 — descripteur, décision, entrées capturées, séquences de
 * commandes — n'ont pas d'objet tant qu'aucune unité n'est déportée (ADR-033).
 *
 * <p>Tout texte libre passe par l'anonymiseur (R-571) : un dump est fait pour être
 * joint à un rapport de défaut, et un message d'exception porte volontiers un chemin.
 */
public final class IncidentDump {

    /** Version du format. Toute modification de structure l'incrémente. */
    public static final int SCHEMA = 1;

    /**
     * Trames gardées par exception.
     *
     * <p>Une trace de Forge dépasse rarement la centaine de trames, et l'origine d'un
     * défaut est en tête : au-delà, ce sont les couches du chargeur d'événements.
     */
    static final int MAX_FRAMES = 64;

    /** Causes gardées dans une chaîne : borne contre une chaîne pathologique. */
    static final int MAX_CAUSES = 8;

    private IncidentDump() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Versions de l'installation, telles que la PARTIE 19.6 les demande.
     *
     * <p>Le jeu fournit les siennes ; la JVM, le système et le processeur sont lus ici.
     *
     * @param minecraft version de Minecraft
     * @param forge version de Forge
     * @param rustforgex version de RUSTFORGE-X
     * @return les versions, dans un ordre stable
     */
    public static Map<String, String> versions(String minecraft, String forge, String rustforgex) {
        Map<String, String> versions = new LinkedHashMap<>();
        versions.put("minecraft", String.valueOf(minecraft));
        versions.put("forge", String.valueOf(forge));
        versions.put("rustforgex", String.valueOf(rustforgex));
        versions.put("java", System.getProperty("java.version") + " ("
                + System.getProperty("java.vendor") + ")");
        versions.put("os", System.getProperty("os.name") + " " + System.getProperty("os.version"));
        versions.put("arch", System.getProperty("os.arch"));
        versions.put("cpus", Integer.toString(Runtime.getRuntime().availableProcessors()));
        return Collections.unmodifiableMap(versions);
    }

    /**
     * Sérialise un incident.
     *
     * @param incident l'incident
     * @param versions versions de l'installation, voir {@link #versions}
     * @param metrics métriques relevées au moment de l'incident, ou {@code null} si le
     *     relevé a échoué
     * @param anonymizer assainisseur des textes libres (R-571)
     * @return le document JSON, terminé par un saut de ligne
     */
    public static String render(
            Incident incident, Map<String, String> versions, MetricSet metrics,
            Anonymizer anonymizer) {
        StringBuilder json = new StringBuilder(8192);
        json.append("{\n  \"schema\": ").append(SCHEMA).append(",\n");
        json.append("  \"kind\": ").append(quote(incident.kind().name())).append(",\n");
        json.append("  \"subject\": ").append(quote(incident.subject())).append(",\n");
        json.append("  \"occurrences\": ").append(incident.occurrences()).append(",\n");
        json.append("  \"at\": ").append(quote(incident.at().toString())).append(",\n");
        json.append("  \"error_code\": ")
                .append(incident.code() == null ? "null" : quote(incident.code().id()))
                .append(",\n");

        json.append("  \"versions\": {");
        int i = 0;
        for (Map.Entry<String, String> version : versions.entrySet()) {
            json.append(i++ == 0 ? "\n" : ",\n")
                    .append("    ").append(quote(version.getKey())).append(": ")
                    .append(quote(anonymizer.clean(version.getValue())));
        }
        json.append("\n  },\n");

        json.append("  \"trace\": ");
        appendTrace(json, incident.cause(), anonymizer);
        json.append(",\n");

        json.append("  \"metrics\": ")
                .append(metrics == null ? "null" : MetricsJson.render(metrics, anonymizer).strip())
                .append("\n}\n");
        return json.toString();
    }

    /**
     * Écrit la chaîne des causes, de l'exception levée à son origine.
     *
     * <p>L'ensemble par identité borne les chaînes cycliques, qu'un {@code initCause}
     * maladroit suffit à produire.
     */
    private static void appendTrace(StringBuilder json, Throwable cause, Anonymizer anonymizer) {
        if (cause == null) {
            json.append("[]");
            return;
        }
        Set<Throwable> seen = Collections.newSetFromMap(new IdentityHashMap<>());
        json.append('[');
        int depth = 0;
        for (Throwable t = cause; t != null && seen.add(t) && depth < MAX_CAUSES;
                t = t.getCause(), depth++) {
            json.append(depth == 0 ? "\n" : ",\n");
            json.append("    {\"class\": ").append(quote(t.getClass().getName()))
                    .append(", \"message\": ")
                    .append(t.getMessage() == null ? "null" : quote(anonymizer.clean(t.getMessage())))
                    .append(", \"frames\": [");
            StackTraceElement[] frames = t.getStackTrace();
            int kept = Math.min(frames.length, MAX_FRAMES);
            for (int f = 0; f < kept; f++) {
                json.append(f == 0 ? "\n" : ",\n").append("      ")
                        .append(quote(anonymizer.clean(frames[f].toString())));
            }
            json.append(kept == 0 ? "]" : "\n    ]")
                    .append(", \"frames_omitted\": ").append(frames.length - kept).append('}');
        }
        json.append("\n  ]");
    }

    private static String quote(String text) {
        return MetricsJson.quote(text);
    }
}
