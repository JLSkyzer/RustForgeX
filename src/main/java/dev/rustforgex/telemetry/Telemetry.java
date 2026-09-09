package dev.rustforgex.telemetry;

import java.util.Map;

/**
 * C-34 : relevé des métriques du runtime, en local et rien qu'en local.
 *
 * <p>Cahier des charges : PARTIE 5.32. Exigences : R-560, R-561, R-562.
 * Tests : T-400 à T-402. Maturité : {@code STABLE}.
 *
 * <h2>Rien ne sort de la machine</h2>
 *
 * <p>R-560 : aucune télémétrie externe, aucune requête réseau sortante. Ce n'est pas une
 * promesse tenue par la discipline mais par l'absence de code : rien dans RUSTFORGE-X
 * n'ouvre de socket, et T-400 le vérifie en relisant les sources plutôt qu'en faisant
 * confiance à cette phrase.
 *
 * <h2>Ce que le relevé ne fait pas</h2>
 *
 * <p>Il n'appelle rien : il met en forme un statut déjà lu. C'est ce qui garantit
 * R-561 — un relevé qui déclencherait un appel natif ou une lecture de fichier ne
 * pourrait pas tenir sous 0,2 % du MSPT, et surtout ne serait pas prévisible.
 *
 * <p>Une valeur que la source n'a pas mesurée n'est pas relevée du tout. Un zéro à la
 * place d'une mesure absente est indiscernable d'un zéro mesuré (R-660).
 */
public final class Telemetry {

    private Telemetry() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Relève les métriques à partir d'un état déjà lu.
     *
     * @param nativeStatus statut natif décodé, ou {@code null} s'il est indisponible
     * @param instrumentation vue de C-04, ou {@code null}
     * @param discovery inventaire de C-41, ou {@code null}
     * @param events table de distribution de C-06, ou {@code null}
     * @return le recueil, jamais {@code null}
     */
    public static MetricSet collect(
            Map<String, Object> nativeStatus,
            InstrumentationCounts instrumentation,
            DiscoveryCounts discovery,
            EventCounts events) {
        return collect(nativeStatus, instrumentation, discovery, events, null);
    }

    /**
     * Relève les métriques, échantillonnage de piles compris.
     *
     * @param nativeStatus statut natif décodé, ou {@code null} s'il est indisponible
     * @param instrumentation vue de C-04, ou {@code null}
     * @param discovery inventaire de C-41, ou {@code null}
     * @param events table de distribution de C-06, ou {@code null}
     * @param sampling compteurs de C-05, ou {@code null} si l'échantillonneur dort
     * @return le recueil, jamais {@code null}
     */
    public static MetricSet collect(
            Map<String, Object> nativeStatus,
            InstrumentationCounts instrumentation,
            DiscoveryCounts discovery,
            EventCounts events,
            SamplingCounts sampling) {
        MetricSet set = new MetricSet();
        appendTick(set, mapOf(nativeStatus, "tick"));
        appendProbes(set, mapOf(nativeStatus, "probes"));
        appendProfiler(set, mapOf(nativeStatus, "profiler"));
        appendInstrumentation(set, instrumentation);
        appendDiscovery(set, discovery);
        appendEvents(set, events);
        appendSampling(set, sampling);
        return set;
    }

    /**
     * Compteurs de C-04, tels que le mod les connaît.
     *
     * @param armed {@code true} si le transformateur est armé
     * @param classesSeen classes vues passer depuis l'armement
     * @param classesMissed classes passées avant l'armement, hors de portée (ADR-022)
     * @param methodsProbed méthodes portant une sonde
     * @param transformFailures échecs de transformation (FM-09)
     * @param probesRequested sondes demandées au natif
     * @param probesUnattributed sondes sur une classe sans mod propriétaire (C-41)
     */
    public record InstrumentationCounts(boolean armed, long classesSeen, long classesMissed,
            long methodsProbed, long transformFailures, long probesRequested,
            long probesUnattributed) {
    }

    /**
     * Compteurs de C-41.
     *
     * @param mods mods inventoriés
     * @param modules modules connus
     * @param packages paquets connus
     * @param durationMs durée de la découverte (R-621)
     */
    public record DiscoveryCounts(int mods, int modules, int packages, long durationMs) {
    }

    /**
     * Compteurs de C-06.
     *
     * @param dispatched événements vus passer
     * @param knownTypes types distincts rencontrés
     * @param timed distributions chronométrées
     * @param abandoned chronométrages abandonnés
     */
    public record EventCounts(long dispatched, int knownTypes, long timed, long abandoned) {
    }

    /**
     * Compteurs de C-05 : échantillonnage de piles et découverte des trames inconnues.
     *
     * @param samplesTaken piles prélevées sur le fil autoritatif
     * @param samplesQueued échantillons attribués à une sonde et mis en file
     * @param samplesUnattributed échantillons qu'aucune sonde ne couvrait
     * @param samplesDropped échantillons perdus, le fil autoritatif ne drainant plus
     * @param unknownFrames méthodes distinctes vues s'exécuter sans sonde
     * @param unknownSamples échantillons ayant désigné une de ces méthodes
     * @param unknownDropped méthodes distinctes non apprises, le recensement étant plein
     */
    public record SamplingCounts(long samplesTaken, long samplesQueued,
            long samplesUnattributed, long samplesDropped, int unknownFrames,
            long unknownSamples, long unknownDropped) {
    }

    private static void appendTick(MetricSet set, Map<String, Object> tick) {
        if (tick == null) {
            return;
        }
        set.add(Metric.counter("rfx.tick.count", longOf(tick, "ticks"),
                "Ticks dont la fenêtre a été ouverte et fermée (IF-02)."));
        set.add(Metric.counter("rfx.tick.unbalanced", longOf(tick, "unbalanced"),
                "Fenêtres de tick ouvertes sans fermeture correspondante."));
        set.add(Metric.counter("rfx.tick.invalid_transitions",
                longOf(tick, "invalid_transitions"),
                "Transitions d'état refusées par la machine du cycle de tick (SM-01)."));
        set.add(Metric.counter("rfx.tick.hook_budget_exceeded",
                longOf(tick, "hook_budget_exceeded"),
                "Accroches ayant dépassé leur budget de temps (INV-14)."));
    }

    private static void appendProbes(MetricSet set, Map<String, Object> probes) {
        if (probes == null) {
            return;
        }
        set.add(Metric.counter("rfx.probes.records_consumed",
                longOf(probes, "records_consumed"),
                "Enregistrements de sonde lus par le natif."));
        set.add(Metric.counter("rfx.probes.records_lost", longOf(probes, "records_lost"),
                "Enregistrements perdus faute de place dans le tampon."));
        set.add(Metric.gauge("rfx.probes.native_bytes", longOf(probes, "native_bytes"),
                "octets", "Mémoire native détenue par les tampons de sonde."));
        set.add(Metric.gauge("rfx.probes.native_limit_bytes",
                longOf(probes, "native_limit_bytes"), "octets",
                "Plafond de mémoire native (memory.max_native_mb)."));
    }

    private static void appendProfiler(MetricSet set, Map<String, Object> profiler) {
        if (profiler == null) {
            return;
        }
        set.add(Metric.counter("rfx.profiler.workloads_tracked",
                longOf(profiler, "workloads_tracked"),
                "Unités de travail suivies simultanément (C-05)."));
        set.add(Metric.counter("rfx.profiler.records_ingested",
                longOf(profiler, "records_ingested"),
                "Enregistrements intégrés aux mesures glissantes."));
        set.add(Metric.counter("rfx.profiler.records_unknown",
                longOf(profiler, "records_unknown"),
                "Enregistrements portant un identifiant de sonde inconnu."));
        set.add(Metric.counter("rfx.profiler.evictions", longOf(profiler, "evictions"),
                "Unités de travail évincées, la plus froide en premier."));
        set.add(Metric.counter("rfx.profiler.level_changes",
                longOf(profiler, "level_changes"),
                "Changements d'état du profiler (SM-02)."));
        set.add(Metric.ratio("rfx.profiler.overhead_pct",
                longOf(profiler, "overhead_pct_x100") / 100.0,
                "Coût du profilage estimé par ses propres compteurs, en part d'un cœur."));
        set.add(Metric.ratio("rfx.profiler.mspt_pct",
                longOf(profiler, "mspt_pct_x100") / 100.0,
                "Coût du profilage estimé, en part du temps de tick."));

        // La ligne de base de la PARTIE 12.4 ne vaut que si une pause a eu lieu. Zéro
        // mesure signifie « pas encore mesuré », jamais « coût nul » (R-660).
        long measurements = longOf(profiler, "baseline_measurements");
        set.add(Metric.counter("rfx.profiler.baseline_measurements", measurements,
                "Mises en pause ayant produit une mesure de coût (PARTIE 12.4)."));
        set.addIfMeasured(measurements > 0, () -> Metric.ratio(
                "rfx.profiler.baseline_overhead_pct",
                longOf(profiler, "baseline_overhead_pct_x100") / 100.0,
                "Coût du profilage réellement mesuré par mise en pause (PARTIE 12.4)."));
    }

    private static void appendInstrumentation(MetricSet set, InstrumentationCounts counts) {
        if (counts == null) {
            return;
        }
        set.add(Metric.gauge("rfx.instr.armed", counts.armed() ? 1 : 0, Metric.NO_UNIT,
                "1 si le transformateur de bytecode est armé, 0 sinon (ADR-017)."));
        set.add(Metric.counter("rfx.instr.classes_seen", counts.classesSeen(),
                "Classes soumises au transformateur depuis son armement."));
        set.add(Metric.counter("rfx.instr.classes_missed", counts.classesMissed(),
                "Classes passées avant l'armement, donc hors de portée à jamais "
                        + "(ADR-022)."));
        set.add(Metric.counter("rfx.instr.methods_probed", counts.methodsProbed(),
                "Méthodes portant effectivement une sonde."));
        set.add(Metric.counter("rfx.instr.transform_failures", counts.transformFailures(),
                "Transformations abandonnées sur erreur, classe rendue intacte (FM-09)."));
        set.add(Metric.counter("rfx.instr.probes_requested", counts.probesRequested(),
                "Identifiants de sonde demandés au runtime natif."));
        set.addIfMeasured(counts.probesRequested() > 0, () -> Metric.ratio(
                "rfx.discovery.unknown_owner_ratio",
                counts.probesUnattributed() * 100.0 / counts.probesRequested(),
                "Part des sondes posées sur une classe non rattachée à un mod (C-41). "
                        + "Porte sur les méthodes sondées, toutes, pas seulement les "
                        + "chaudes : ne prononce pas l'acceptance de la PARTIE 5.39."));
    }

    private static void appendDiscovery(MetricSet set, DiscoveryCounts counts) {
        if (counts == null) {
            return;
        }
        set.add(Metric.counter("rfx.discovery.mods", counts.mods(),
                "Mods inventoriés par C-41."));
        set.add(Metric.counter("rfx.discovery.modules", counts.modules(),
                "Modules Java distincts portant ces mods."));
        set.add(Metric.counter("rfx.discovery.packages", counts.packages(),
                "Paquets déclarés, tous mods confondus."));
        set.add(Metric.gauge("rfx.discovery.duration_ms", counts.durationMs(), "ms",
                "Durée de la découverte, hachages exclus (R-621, ADR-023)."));
    }

    private static void appendEvents(MetricSet set, EventCounts counts) {
        if (counts == null) {
            return;
        }
        set.add(Metric.counter("rfx.events.dispatched", counts.dispatched(),
                "Événements vus passer sur le bus Forge (C-06)."));
        set.add(Metric.counter("rfx.events.known_types", counts.knownTypes(),
                "Types d'événements distincts rencontrés."));
        set.add(Metric.counter("rfx.events.timed", counts.timed(),
                "Distributions chronométrées, une sur soixante-quatre."));
        set.add(Metric.counter("rfx.events.abandoned", counts.abandoned(),
                "Chronométrages abandonnés, distribution imbriquée trop profonde."));
    }

    /**
     * Échantillonnage de piles et découverte (C-05).
     *
     * <p>Le ratio de découverte est ce qui compte : il dit quelle part du temps du fil
     * autoritatif s'exécute dans des méthodes qu'aucune sonde ne couvre. C'est la mesure
     * du trou de couverture, celle qu'aucun compteur ne donnait avant.
     */
    private static void appendSampling(MetricSet set, SamplingCounts counts) {
        if (counts == null) {
            return;
        }
        set.add(Metric.counter("rfx.sampling.samples_taken", counts.samplesTaken(),
                "Piles prélevées sur le fil autoritatif (R-322)."));
        set.add(Metric.counter("rfx.sampling.samples_queued", counts.samplesQueued(),
                "Échantillons attribués à une sonde et mis en file."));
        set.add(Metric.counter("rfx.sampling.samples_unattributed",
                counts.samplesUnattributed(),
                "Échantillons qu'aucune sonde connue ne couvrait."));
        set.add(Metric.counter("rfx.sampling.samples_dropped", counts.samplesDropped(),
                "Échantillons perdus, le fil autoritatif ne drainant plus."));
        set.add(Metric.counter("rfx.discovery.unknown_frames", counts.unknownFrames(),
                "Méthodes distinctes vues s'exécuter sans porter de sonde (C-05)."));
        set.add(Metric.counter("rfx.discovery.unknown_samples", counts.unknownSamples(),
                "Échantillons ayant désigné une méthode chaude non sondée."));
        set.add(Metric.counter("rfx.discovery.unknown_frames_dropped",
                counts.unknownDropped(),
                "Méthodes distinctes non apprises, le recensement étant plein."));

        // Sans prélèvement, il n'y a pas de part à publier : zéro voudrait dire « tout
        // est sondé », ce qui est l'inverse de « on n'a pas regardé » (R-660).
        set.addIfMeasured(counts.samplesTaken() > 0, () -> Metric.ratio(
                "rfx.discovery.unknown_frame_ratio",
                counts.unknownSamples() * 100.0 / counts.samplesTaken(),
                "Part du temps du fil autoritatif passée dans une méthode candidate au "
                        + "sondage mais non sondée. C'est la mesure du trou de "
                        + "couverture (ADR-027)."));
    }

    private static long longOf(Map<String, Object> table, String key) {
        Object value = table == null ? null : table.get(key);
        return value instanceof Long number ? number : 0L;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> mapOf(Map<String, Object> parent, String key) {
        Object value = parent == null ? null : parent.get(key);
        return value instanceof Map ? (Map<String, Object>) value : null;
    }
}
