package dev.rustforgex.command;

import dev.rustforgex.instrument.ProbeRegistry;
import net.minecraft.network.chat.Component;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * C-38 : composition du rapport affiché par {@code /rfx top} (C-35).
 *
 * <p>Cahier des charges : PARTIE 5.33 et 5.36. Maturité : {@code STABLE}.
 *
 * <p>Séparé de la commande pour être testable sans démarrer Minecraft. Il ne fait
 * aucun appel natif : il met en forme un classement déjà lu.
 *
 * <h2>Un coût dit toujours d'où il vient</h2>
 *
 * <p>Quand aucune sonde n'est armée, la seule mesure disponible est l'échantillonnage
 * de piles (R-322), qui est statistique. Le classement porte donc la provenance de
 * chaque coût, et le rapport l'affiche. Un nombre sans sa provenance ne se compare pas.
 *
 * <h2>Ce qui n'est pas mesuré n'est pas affiché comme mesuré</h2>
 *
 * <p>Une unité sans aucune observation apparaît comme telle. Elle ne se voit pas
 * attribuer {@code 0 ns}, qui serait indiscernable d'une méthode réellement gratuite
 * (R-660).
 */
public final class TopReport {

    /** Préfixe commun à toutes les clés de traduction du rapport. */
    public static final String KEY_PREFIX = "rustforgex.top.";

    /** Nombre d'unités demandées par défaut. */
    public static final int DEFAULT_LIMIT = 15;

    private TopReport() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Compose les lignes du rapport.
     *
     * @param top classement décodé, ou {@code null} s'il est indisponible
     * @param registry registre des sondes, pour retrouver les noms, ou {@code null}
     * @return les lignes à afficher, jamais vides
     */
    public static List<Component> lines(Map<String, Object> top, ProbeRegistry registry) {
        if (top == null) {
            return List.of(Component.translatable(KEY_PREFIX + "unavailable"));
        }

        long tracked = longOf(top, "tracked");
        long measured = longOf(top, "measured");
        List<Component> lines = new ArrayList<>();
        lines.add(Component.translatable(KEY_PREFIX + "summary", measured, tracked));

        if (measured == 0) {
            // Aligner des unités à zéro donnerait à croire qu'on a mesuré et trouvé
            // zéro. On n'a rien mesuré : c'est une information différente, et plus
            // utile — elle dit d'aller voir pourquoi.
            lines.add(Component.translatable(KEY_PREFIX + "nothing_measured"));
            return List.copyOf(lines);
        }

        Object entries = top.get("entries");
        if (entries instanceof List<?> list) {
            int rank = 0;
            for (Object element : list) {
                if (!(element instanceof Map<?, ?> raw)) {
                    continue;
                }
                @SuppressWarnings("unchecked")
                Map<String, Object> entry = (Map<String, Object>) raw;
                if (longOf(entry, "cost_ns_per_tick") == 0) {
                    // Le classement est décroissant : la première unité sans coût
                    // signale que toutes les suivantes n'en ont pas non plus.
                    break;
                }
                lines.add(describe(++rank, entry, registry));
            }
        }
        return List.copyOf(lines);
    }

    /** Décrit une unité de travail en une ligne. */
    private static Component describe(int rank, Map<String, Object> entry, ProbeRegistry registry) {
        int probeId = (int) longOf(entry, "probe_id");
        String name = registry == null ? null : registry.nameOf(probeId);

        return Component.translatable(KEY_PREFIX + "entry",
                rank,
                Component.literal(name == null ? "sonde#" + probeId : name),
                Component.literal(microseconds(longOf(entry, "cost_ns_per_tick"))),
                Component.literal(source(entry)),
                Component.literal(String.format(Locale.ROOT, "%.1f",
                        longOf(entry, "calls_per_tick_x100") / 100.0)),
                Component.literal(stringOf(entry, "heat")));
    }

    /**
     * Provenance du coût, en clair.
     *
     * <p>Le natif sérialise l'énumération sous son nom Rust ; la traduire ici plutôt
     * que d'exposer {@code Probe} et {@code Sampling} évite de faire fuir un détail de
     * l'autre côté de la frontière dans une interface joueur.
     */
    private static String source(Map<String, Object> entry) {
        String raw = stringOf(entry, "source");
        return switch (raw) {
            case "Probe" -> "sonde";
            case "Sampling" -> "échantillon";
            case "None" -> "—";
            default -> raw;
        };
    }

    /** Nanosecondes en microsecondes, avec une décimale. */
    private static String microseconds(long nanos) {
        return String.format(Locale.ROOT, "%.1f us", nanos / 1_000.0);
    }

    private static long longOf(Map<String, Object> table, String key) {
        Object value = table == null ? null : table.get(key);
        return value instanceof Long number ? number : 0L;
    }

    private static String stringOf(Map<String, Object> table, String key) {
        Object value = table == null ? null : table.get(key);
        return value instanceof String text ? text : "?";
    }
}
