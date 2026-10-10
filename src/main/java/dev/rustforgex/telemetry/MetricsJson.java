package dev.rustforgex.telemetry;

import java.util.List;
import java.util.Locale;

/**
 * Export JSON des métriques (C-34).
 *
 * <p>Cahier des charges : PARTIE 5.32, « export : fichier JSON à la demande ».
 * Maturité : {@code STABLE}.
 *
 * <p>Écrit à la main, sans bibliothèque. Le format produit est plat et connu — un
 * objet, des séries, quatre champs chacune — et lui ajouter une dépendance
 * d'exécution au JAR pour cela contreviendrait à la règle du dépôt : aucune dépendance
 * Java d'exécution sans nécessité démontrée.
 *
 * <p>Chaque série porte sa description : R-562 exige qu'une métrique soit documentée,
 * et un export qui ne dirait que des nombres obligerait son lecteur à revenir au code.
 */
public final class MetricsJson {

    private MetricsJson() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Sérialise un recueil.
     *
     * @param metrics les mesures relevées
     * @param anonymizer assainisseur appliqué aux descriptions (R-571), ou {@code null}
     * @return le document JSON, terminé par un saut de ligne
     */
    public static String render(MetricSet metrics, Anonymizer anonymizer) {
        StringBuilder json = new StringBuilder(4096);
        json.append("{\n  \"schema\": 1,\n  \"metrics\": [\n");

        List<Metric> all = metrics.all();
        for (int i = 0; i < all.size(); i++) {
            Metric metric = all.get(i);
            json.append("    {")
                    .append("\"name\": ").append(quote(metric.name())).append(", ")
                    .append("\"type\": ").append(quote(metric.type().name().toLowerCase(Locale.ROOT)))
                    .append(", ")
                    .append("\"unit\": ").append(quote(metric.unit())).append(", ")
                    .append("\"value\": ").append(number(metric.value())).append(", ")
                    .append("\"description\": ")
                    .append(quote(anonymizer == null
                            ? metric.description()
                            : anonymizer.clean(metric.description())))
                    .append("}");
            if (i < all.size() - 1) {
                json.append(',');
            }
            json.append('\n');
        }
        json.append("  ]\n}\n");
        return json.toString();
    }

    /**
     * Écrit un nombre sans notation exponentielle ni virgule décimale.
     *
     * <p>{@code Double.toString} produit {@code 1.0E-4} et, sous une locale française,
     * {@code String.format} produirait {@code 0,0001} — deux formes que JSON refuse. Le
     * format est donc explicite, et la locale racine imposée.
     */
    private static String number(double value) {
        if (value == Math.rint(value) && Math.abs(value) < 1e15) {
            return Long.toString((long) value);
        }
        return String.format(Locale.ROOT, "%.4f", value);
    }

    /**
     * Échappe une chaîne selon RFC 8259.
     *
     * <p>Publique pour le dump d'incident (C-35), qui écrit son JSON de la même façon et
     * pour la même raison : aucune dépendance d'exécution.
     *
     * @param text texte brut
     * @return la chaîne JSON, guillemets compris
     */
    public static String quote(String text) {
        StringBuilder out = new StringBuilder(text.length() + 16);
        out.append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format(Locale.ROOT, "\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        return out.append('"').toString();
    }
}
