package dev.rustforgex.command;

import net.minecraft.network.chat.Component;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/**
 * C-38 : composition du rapport affiché par {@code /rfx discover} (C-05).
 *
 * <p>Cahier des charges : PARTIE 5.5, PARTIE 5.36. Exigence : R-660.
 * Maturité : {@code STABLE}.
 *
 * <h2>Ce que ce classement dit, et ce qu'il ne dit pas</h2>
 *
 * <p>{@code /rfx top} répond à « qu'est-ce qui coûte, parmi ce que je mesure ». Celui-ci
 * répond à la question d'avant : <strong>qu'est-ce que je ne mesure pas</strong>. Ce sont
 * les méthodes que l'échantillonneur a vues s'exécuter sur le fil autoritatif sans
 * qu'aucune sonde ne les couvre.
 *
 * <p>La part affichée est celle du <strong>temps du fil autoritatif</strong>, pas celle
 * du tick : l'échantillonneur prélève aussi entre deux ticks, quand le serveur attend.
 * Les deux ne se confondent pas, et présenter l'une pour l'autre gonflerait chaque ligne
 * d'un facteur qu'on ne connaît pas.
 *
 * <p>C'est une mesure statistique : cent prélèvements par seconde. Une méthode vue trois
 * fois n'est pas une mesure, c'est un indice. Le nombre d'échantillons est donc affiché
 * à côté de la part, pour qu'on puisse juger de ce qu'elle vaut (R-660).
 */
public final class DiscoveryReport {

    /** Préfixe commun à toutes les clés de traduction du rapport. */
    public static final String KEY_PREFIX = "rustforgex.discover.";

    /** Nombre de méthodes affichées par défaut. */
    public static final int DEFAULT_LIMIT = 15;

    private DiscoveryReport() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Une méthode chaude non sondée, telle que le rapport la reçoit.
     *
     * @param owner identifiant du mod propriétaire, jamais {@code null}
     * @param label {@code classe#methode}
     * @param samples nombre d'échantillons où elle s'exécutait
     */
    public record Row(String owner, String label, long samples) {
    }

    /**
     * État du recensement à l'instant de la commande.
     *
     * @param samplesTaken piles prélevées depuis le démarrage de l'échantillonneur
     * @param recorded échantillons ayant désigné une méthode candidate non sondée
     * @param distinct méthodes distinctes recensées
     * @param distinctDropped méthodes distinctes non apprises, le recensement étant plein
     * @param rows classement décroissant, déjà tronqué à la limite demandée
     */
    public record Census(long samplesTaken, long recorded, int distinct, long distinctDropped,
            List<Row> rows) {
    }

    /**
     * Compose les lignes du rapport.
     *
     * @param census recensement, ou {@code null} s'il est indisponible
     * @return les lignes à afficher, jamais vides
     */
    public static List<Component> lines(Census census) {
        if (census == null) {
            return List.of(Component.translatable(KEY_PREFIX + "unavailable"));
        }
        if (census.samplesTaken() == 0L) {
            // Aucune pile prélevée : l'échantillonneur n'a pas tourné. Afficher un
            // classement vide donnerait à croire qu'on a cherché et rien trouvé.
            return List.of(Component.translatable(KEY_PREFIX + "not_sampled"));
        }

        List<Component> lines = new ArrayList<>();
        lines.add(Component.translatable(KEY_PREFIX + "summary",
                census.distinct(), census.samplesTaken(),
                Component.literal(share(census.recorded(), census.samplesTaken()))));

        if (census.rows().isEmpty()) {
            lines.add(Component.translatable(KEY_PREFIX + "nothing"));
            return List.copyOf(lines);
        }

        int rank = 0;
        for (Row row : census.rows()) {
            if (row.samples() <= 0L) {
                // Le classement est décroissant : la première méthode sans échantillon
                // annonce que toutes les suivantes n'en ont pas non plus.
                break;
            }
            lines.add(Component.translatable(KEY_PREFIX + "entry",
                    ++rank,
                    Component.literal(row.owner()),
                    Component.literal(row.label()),
                    Component.literal(share(row.samples(), census.samplesTaken())),
                    row.samples()));
        }

        if (census.distinctDropped() > 0L) {
            // Le recensement est plafonné. Le taire laisserait croire à un classement
            // complet, alors qu'il ignore tout ce qui est apparu après saturation.
            lines.add(Component.translatable(KEY_PREFIX + "truncated",
                    census.distinctDropped()));
        }
        return List.copyOf(lines);
    }

    /** Part d'un décompte dans le total des prélèvements, en pourcent. */
    private static String share(long part, long total) {
        if (total == 0L) {
            return "—";
        }
        return String.format(Locale.ROOT, "%.1f %%", part * 100.0 / total);
    }
}
