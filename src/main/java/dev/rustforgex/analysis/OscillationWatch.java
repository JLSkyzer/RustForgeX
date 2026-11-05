package dev.rustforgex.analysis;

import java.util.Map;

/**
 * Surveille les verrouillages pour oscillation de décision (C-15, FM-21, {@code E-2201}).
 *
 * <p>Composant : C-15, côté Java. Maturité : {@code STABLE}.
 *
 * <p>Le natif compte les unités verrouillées sur {@code JAVA_ONLY} parce que leur
 * décision changeait trop souvent ; il ne journalise rien. Cette classe compare chaque
 * relevé au précédent et ne signale que les nouveaux verrouillages : un verrouillage qui
 * dure n'est dit qu'une fois. Pure : aucun appel natif, aucun journal.
 */
public final class OscillationWatch {

    private long lastLocked;

    /**
     * Prend un relevé du statut natif décodé.
     *
     * @param status statut natif, ou {@code null} s'il est indisponible
     * @return le nombre de nouvelles unités verrouillées depuis le relevé précédent ;
     *     zéro si rien de neuf, ou si le statut est illisible
     */
    public long observe(Map<String, Object> status) {
        if (status == null || !(status.get("decision") instanceof Map<?, ?> decision)
                || !(decision.get("locked") instanceof Long locked)) {
            return 0L;
        }
        long fresh = Math.max(0L, locked - lastLocked);
        lastLocked = locked;
        return fresh;
    }
}
