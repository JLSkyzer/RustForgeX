package dev.rustforgex.telemetry;

import java.util.List;
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
        return collect(nativeStatus, instrumentation, discovery, events, sampling, List.of());
    }

    /**
     * Relève les métriques, accroches comprises.
     *
     * @param nativeStatus statut natif décodé, ou {@code null} s'il est indisponible
     * @param instrumentation vue de C-04, ou {@code null}
     * @param discovery inventaire de C-41, ou {@code null}
     * @param events table de distribution de C-06, ou {@code null}
     * @param sampling compteurs de C-05, ou {@code null} si l'échantillonneur dort
     * @param hooks compteurs des gardes d'accroche de C-01, jamais {@code null}
     * @return le recueil, jamais {@code null}
     */
    public static MetricSet collect(
            Map<String, Object> nativeStatus,
            InstrumentationCounts instrumentation,
            DiscoveryCounts discovery,
            EventCounts events,
            SamplingCounts sampling,
            List<HookCounts> hooks) {
        MetricSet set = new MetricSet();
        appendTick(set, mapOf(nativeStatus, "tick"));
        appendProbes(set, mapOf(nativeStatus, "probes"));
        appendProfiler(set, mapOf(nativeStatus, "profiler"));
        appendScheduler(set, mapOf(nativeStatus, "scheduler"));
        appendDb(set, mapOf(nativeStatus, "db"));
        appendCallGraph(set, mapOf(nativeStatus, "analysis"));
        appendInstrumentation(set, instrumentation);
        appendDiscovery(set, discovery);
        appendEvents(set, events);
        appendSampling(set, sampling);
        appendHooks(set, hooks);
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

    /**
     * Compteurs d'une garde d'accroche de C-01 (PARTIE 4.19, FM-02).
     *
     * <p>Le nom de l'accroche devient une étiquette de série. Il est fixé par le code —
     * sept gardes, nommées dans {@code RustForgeX} — et jamais par une donnée
     * d'exécution : la cardinalité reste bornée (T-402).
     *
     * @param hook nom de l'accroche
     * @param calls exécutions, réussies ou non
     * @param timedCalls exécutions chronométrées ({@code telemetry.enabled})
     * @param averageDurationNs durée moyenne d'une exécution chronométrée
     * @param errors échecs capturés
     * @param disabled {@code true} si l'accroche a été désactivée
     */
    public record HookCounts(String hook, long calls, long timedCalls, long averageDurationNs,
            long errors, boolean disabled) {
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

    /**
     * Charge externe relevée pour l'ordonnanceur (C-17, R-421, ADR-046).
     *
     * <p>Zéro relevé veut dire « pas encore mesuré », jamais « aucune charge » : les
     * jauges n'apparaissent qu'après le premier relevé (R-660).
     */
    private static void appendScheduler(MetricSet set, Map<String, Object> scheduler) {
        if (scheduler == null) {
            return;
        }
        long reports = longOf(scheduler, "external_load_reports");
        set.add(Metric.counter("rfx.sched.external_load_reports", reports,
                "Relevés de charge externe reçus par le natif, un par seconde (R-421)."));
        set.addIfMeasured(reports > 0, () -> Metric.gauge("rfx.sched.external_runnable",
                longOf(scheduler, "external_runnable"), "fils",
                "Fils tiers ayant consommé au moins 10 % d'un cœur, au dernier relevé (R-421)."));
        set.addIfMeasured(reports > 0, () -> Metric.gauge("rfx.sched.external_runnable_max",
                longOf(scheduler, "external_runnable_max"), "fils",
                "Plus haut relevé de fils exécutables tiers depuis le démarrage."));
    }

    /**
     * Base des unités de travail (C-09, ADR-049).
     *
     * <p>Rien n'est publié tant que la base n'est pas ouverte, hormis les échecs
     * d'ouverture (R-660). Les entrées relues n'ont de sens qu'une fois la relecture
     * terminée, la durée d'écriture qu'après la première écriture.
     */
    /**
     * Graphe d'appel (C-07, ADR-051).
     *
     * <p>Rien tant que la hiérarchie n'est pas déclarée : avant, aucun graphe ne peut
     * exister. La part d'appels non résolus n'a de sens qu'une fois un site rencontré.
     */
    private static void appendCallGraph(MetricSet set, Map<String, Object> analysis) {
        if (analysis == null || !Boolean.TRUE.equals(analysis.get("enabled"))) {
            return;
        }
        long hierarchy = longOf(analysis, "hierarchy_classes");
        set.add(Metric.gauge("rfx.callgraph.hierarchy_classes", hierarchy, "classes",
                "Classes de l'installation déclarées à l'analyse (nom, super-classe, "
                        + "drapeaux)."));
        set.add(Metric.counter("rfx.callgraph.budget_refusals",
                longOf(analysis, "budget_refusals"),
                "Refus du budget mémoire natif ; le premier arrête l'analyse (R-533)."));
        if (hierarchy == 0) {
            return;
        }
        long resolved = longOf(analysis, "resolved_calls");
        long unresolved = longOf(analysis, "unresolved_calls");
        long buildNs = longOf(analysis, "last_build_ns");
        set.add(Metric.counter("rfx.callgraph.graphs", longOf(analysis, "graphs"),
                "Graphes d'appel construits, un par unité chaude."));
        set.add(Metric.counter("rfx.callgraph.rounds", longOf(analysis, "rounds"),
                "Tours de construction, relances comprises quand une classe manquait."));
        set.add(Metric.counter("rfx.callgraph.nodes", longOf(analysis, "nodes"),
                "Méthodes, sommées sur tous les graphes."));
        set.add(Metric.counter("rfx.callgraph.resolved_calls", resolved,
                "Sites d'appel dont toutes les cibles sont connues."));
        set.add(Metric.counter("rfx.callgraph.unresolved_calls", unresolved,
                "Sites d'appel non résolus : interface, classe inconnue, réflexion..."));
        set.addIfMeasured(resolved + unresolved > 0, () -> Metric.ratio(
                "rfx.callgraph.unresolved_ratio", unresolved * 100.0 / (resolved + unresolved),
                "Part des sites d'appel non résolus (FM-15)."));
        set.add(Metric.counter("rfx.callgraph.truncated", longOf(analysis, "truncated"),
                "Graphes arrêtés par une borne : profondeur ou nombre de méthodes (R-340)."));
        set.add(Metric.counter("rfx.callgraph.reflection", longOf(analysis, "reflection"),
                "Graphes où un appel réflexif a été rencontré."));
        set.add(Metric.counter("rfx.callgraph.classes_provided",
                longOf(analysis, "classes_provided"),
                "Classes dont le bytecode a été lu pour l'analyse."));
        set.add(Metric.counter("rfx.callgraph.classes_absent",
                longOf(analysis, "classes_absent"),
                "Classes tenues pour inconnues : cibles de mixin, d'access transformer ou "
                        + "de coremod, illisibles."));
        set.addIfMeasured(buildNs > 0, () -> Metric.gauge("rfx.callgraph.build_ms",
                buildNs / 1_000_000.0, "ms", "Durée de la dernière construction, hors tick."));
        appendWorkloadProfiler(set, analysis);
    }

    /**
     * Profileur d'unités de travail (C-08, ADR-054) : descripteurs tirés des graphes.
     * Publié avec la hiérarchie, comme le graphe d'appel dont il dépend.
     */
    private static void appendWorkloadProfiler(MetricSet set, Map<String, Object> analysis) {
        set.add(Metric.gauge("rfx.analysis.queue_depth", longOf(analysis, "queue"), "unités",
                "Unités chaudes en attente d'analyse."));
        set.add(Metric.counter("rfx.analysis.completed", longOf(analysis, "completed"),
                "Descripteurs calculés, recalculs compris."));
        set.add(Metric.counter("rfx.analysis.truncated",
                longOf(analysis, "descriptors_truncated"),
                "Descripteurs tronqués par une borne ou par le budget : UNKNOWN (FM-16)."));
        set.add(Metric.gauge("rfx.analysis.cpu_ms", longOf(analysis, "step_ns") / 1_000_000.0,
                "ms", "Temps passé dans les pas d'analyse, mesuré à l'horloge monotone "
                        + "du fil d'analyse : borne haute du temps processeur."));
        set.add(Metric.gauge("rfx.analysis.descriptors", longOf(analysis, "descriptors"),
                "unités", "Descripteurs tenus à jour."));
        set.add(Metric.gauge("rfx.analysis.unknown", longOf(analysis, "descriptors_unknown"),
                "unités", "Descripteurs UNKNOWN : un fait manque, traité comme le pire cas "
                        + "(P-02)."));
        set.add(Metric.counter("rfx.analysis.descriptor_changes",
                longOf(analysis, "descriptor_changes"),
                "Changements significatifs d'un descripteur : drapeaux, ou confiance de 0,1."));
        set.add(Metric.counter("rfx.analysis.recomputes", longOf(analysis, "recomputes"),
                "Recalculs sur changement du bytecode de la méthode racine (R-350)."));
    }

    private static void appendDb(MetricSet set, Map<String, Object> db) {
        if (db == null || !Boolean.TRUE.equals(db.get("enabled"))) {
            return;
        }
        set.add(Metric.counter("rfx.db.open_failures", longOf(db, "open_failures"),
                "Ouvertures échouées faute de fil d'écriture ; la base reste fermée."));
        if (!Boolean.TRUE.equals(db.get("open"))) {
            return;
        }
        long load = longOf(db, "load");
        long writes = longOf(db, "writes");
        set.add(Metric.gauge("rfx.db.load", load, "code",
                "Relecture : 0 en cours, 1 aucun fichier, 2 autre environnement, 3 fichier "
                        + "mis de côté, 4 relue, 5 lecture refusée."));
        set.addIfMeasured(load == 4, () -> Metric.gauge("rfx.db.loaded_entries",
                longOf(db, "loaded_entries"), "unités", "Unités de travail relues du fichier."));
        set.addIfMeasured(load == 4, () -> Metric.counter("rfx.db.load_rejected_entries",
                longOf(db, "rejected_entries"),
                "Entrées écartées à la relecture : leur code a changé (R-360)."));
        set.add(Metric.gauge("rfx.db.entries", longOf(db, "entries"), "unités",
                "Unités de travail dans la base."));
        set.add(Metric.gauge("rfx.db.memory_bytes", longOf(db, "estimated_bytes"), "octets",
                "Mémoire estimée de la base (cache.max_memory_mb)."));
        set.add(Metric.counter("rfx.db.evictions", longOf(db, "evictions"),
                "Unités de travail évincées de la base, les plus froides d'abord (R-362)."));
        set.add(Metric.gauge("rfx.db.pending", longOf(db, "pending"), "unités",
                "Unités découvertes, pas encore confiées au fil d'écriture."));
        set.add(Metric.counter("rfx.db.syncs", longOf(db, "syncs"),
                "Lots confiés au fil d'écriture."));
        set.add(Metric.counter("rfx.db.handed_back", longOf(db, "handed_back"),
                "Lots rendus, file pleine ; repris au cycle suivant."));
        set.add(Metric.counter("rfx.db.writes", writes, "Fichiers workload.db écrits."));
        set.add(Metric.counter("rfx.db.write_failures", longOf(db, "write_failures"),
                "Échecs d'écriture ; le premier désactive l'écriture (R-692)."));
        set.add(Metric.gauge("rfx.db.write_disabled",
                Boolean.TRUE.equals(db.get("disabled")) ? 1 : 0, "booléen",
                "1 si l'écriture est désactivée après un échec ; la base continue en mémoire."));
        set.addIfMeasured(writes > 0, () -> Metric.gauge("rfx.db.persist_ms",
                longOf(db, "persist_ms"), "ms",
                "Durée de la dernière écriture, encodage compris, sur le fil RFX-IO."));
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
        set.add(Metric.counter("rfx.profiler.call_capped", longOf(profiler, "call_capped"),
                "Sondes éteintes en ce moment pour trop d'appels par tick (ADR-038)."));
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

    private static void appendHooks(MetricSet set, List<HookCounts> hooks) {
        long disabled = 0;
        for (HookCounts hook : hooks) {
            String label = "{hook=" + hook.hook() + "}";
            set.add(Metric.counter("rfx.hook.calls" + label, hook.calls(),
                    "Exécutions de l'accroche, réussies ou non (C-01)."));
            set.add(Metric.counter("rfx.hook.errors" + label, hook.errors(),
                    "Exceptions capturées par la garde de l'accroche (FM-02)."));
            // Télémétrie coupée : la durée n'est pas mesurée, elle n'est donc pas publiée.
            set.addIfMeasured(hook.timedCalls() > 0, () -> Metric.gauge(
                    "rfx.hook.duration_ns" + label, hook.averageDurationNs(), "ns",
                    "Durée moyenne d'une exécution chronométrée de l'accroche."));
            if (hook.disabled()) {
                disabled++;
            }
        }
        if (!hooks.isEmpty()) {
            set.add(Metric.counter("rfx.hook.disabled", disabled,
                    "Accroches désactivées après cinq échecs ; le jeu continue sans elles."));
        }
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
