package dev.rustforgex.analysis;

import java.util.Map;

/**
 * Suit l'étalonnage A/A de la chaîne de mesure (C-15, ADR-069).
 *
 * <p>Composant : C-15, côté Java. Maturité : {@code STABLE}.
 *
 * <p>Le natif compte les comparaisons A/A conclues et celles qui ont conclu, à tort, à un
 * gain. Cette classe compare chaque relevé au précédent et ne rend un état que si de
 * nouvelles comparaisons ont été conclues, ou si le nombre d'unités suivies a changé : le
 * journal dit le progrès sans se répéter.
 * Pure : aucun appel natif, aucun journal.
 */
public final class CalibrationWatch {

    /**
     * État de l'étalonnage à un relevé.
     *
     * @param runs comparaisons conclues depuis le démarrage
     * @param falseGains comparaisons conclues à un gain, toutes fausses
     * @param ticks ticks relevés intégrés
     * @param traced unités relevées en ce moment
     */
    public record Calibration(long runs, long falseGains, long ticks, long traced) {
    }

    private long lastRuns;
    private long lastTraced = -1L;

    /**
     * Prend un relevé du statut natif décodé.
     *
     * @param status statut natif, ou {@code null} s'il est indisponible
     * @return l'état si de nouvelles comparaisons ont été conclues ou si le nombre d'unités
     *     suivies a changé depuis le relevé précédent ; {@code null} sinon, ou si le statut
     *     est illisible
     */
    public Calibration observe(Map<String, Object> status) {
        if (status == null || !(status.get("decision") instanceof Map<?, ?> decision)
                || !(decision.get("aa_runs") instanceof Long runs)
                || !(decision.get("aa_false_gains") instanceof Long falseGains)
                || !(decision.get("aa_ticks") instanceof Long ticks)
                || !(decision.get("traced") instanceof Long traced)) {
            return null;
        }
        if (runs <= lastRuns && traced == lastTraced) {
            return null;
        }
        lastRuns = Math.max(lastRuns, runs);
        lastTraced = traced;
        return new Calibration(runs, falseGains, ticks, traced);
    }
}
