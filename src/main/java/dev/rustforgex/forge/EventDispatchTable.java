package dev.rustforgex.forge;

import java.util.ArrayList;
import java.util.List;

/**
 * C-06, étape 1 : comptage des événements, par type, sans rien modifier.
 *
 * <p>Cahier des charges : PARTIE 5.6. Exigences : R-330 (l'ordre observable ne change
 * jamais), R-332 (un événement porteur de résultat est {@code ORDER_SENSITIVE}).
 * Invariant : INV-14. Maturité : {@code STABLE}.
 *
 * <p>Cette classe ne connaît ni Forge ni le moindre événement : elle reçoit des
 * {@link Class} et des horodatages. C'est ce qui la rend vérifiable sans démarrer un
 * serveur, et c'est délibéré — la glu qui l'alimente, elle, ne se teste qu'en jeu.
 *
 * <h2>Ce qui est compté toujours, et ce qui ne l'est pas</h2>
 *
 * <p>Le comptage d'un passage coûte une lecture de table et une incrémentation. La
 * <strong>durée</strong>, elle, coûte deux lectures d'horloge à vingt-cinq nanosecondes
 * pièce. Sur un serveur moddé qui poste des milliers d'événements par tick, chronométrer
 * chacun coûterait le dixième de milliseconde — soit, à lui seul, l'ordre de grandeur du
 * budget entier de RUSTFORGE-X (ADR-021).
 *
 * <p>Donc : <strong>on compte tout, on ne chronomètre qu'un événement sur
 * {@link #SAMPLE_EVERY}.</strong> La moyenne par type reste estimable, et le coût de
 * l'instrument redevient négligeable. C'est la leçon de la journée du 2026-09-05,
 * appliquée avant d'avoir à la réapprendre.
 *
 * <h2>Un seul fil</h2>
 *
 * <p>Seuls les événements du fil autoritatif sont observés. Les autres ne consomment pas
 * de temps de tick, et les ignorer permet de se passer de tout verrou et de tout
 * compteur atomique dans un chemin appelé des milliers de fois par tick. Le filtrage est
 * l'affaire de l'appelant.
 */
public final class EventDispatchTable {

    /** Un événement sur combien est chronométré. */
    public static final int SAMPLE_EVERY = 64;

    /**
     * Profondeur maximale d'imbrication chronométrée.
     *
     * <p>Un gestionnaire d'événement en poste souvent un autre. Au-delà de cette
     * profondeur, le passage est encore compté, mais plus chronométré : c'est une
     * situation assez rare pour que la perdre ne coûte rien, et une pile bornée évite
     * d'avoir à allouer.
     */
    static final int MAX_NESTING = 16;

    /** Types observés, dans l'ordre où ils sont apparus. */
    private final List<EventStats> known = new ArrayList<>(256);

    /**
     * Association type d'événement vers ses compteurs.
     *
     * <p>{@link ClassValue} est fait pour cela : la valeur est calculée une fois par
     * classe puis lue sans allocation ni verrou.
     */
    private final ClassValue<EventStats> stats = new ClassValue<>() {
        @Override
        protected EventStats computeValue(Class<?> type) {
            EventStats created = new EventStats(type.getName());
            // Rare — une fois par type d'événement — et jamais dans le chemin chaud.
            synchronized (known) {
                known.add(created);
            }
            return created;
        }
    };

    /** Pile des événements en cours de chronométrage, du plus ancien au plus récent. */
    private final Object[] timedEvent = new Object[MAX_NESTING];
    private final long[] timedStart = new long[MAX_NESTING];
    private int depth;

    private long dispatched;
    private long timed;
    private long abandoned;

    /**
     * Compte un événement et dit s'il faut le chronométrer.
     *
     * <p>Rendre la décision <strong>avant</strong> que l'appelant lise l'horloge est
     * tout l'intérêt : c'est la lecture d'horloge qui coûte, pas le comptage.
     *
     * @param type type de l'événement
     * @return {@code true} si l'appelant doit lire l'horloge et appeler
     *     {@link #beginTimed(Object, long)}
     */
    public boolean shouldTime(Class<?> type) {
        EventStats entry = stats.get(type);
        entry.dispatched++;
        dispatched++;
        // Masque plutôt que modulo : SAMPLE_EVERY est une puissance de deux, et une
        // division entière n'a rien à faire dans un chemin appelé mille fois par tick.
        return (dispatched & (SAMPLE_EVERY - 1)) == 0 && depth < MAX_NESTING;
    }

    /**
     * Empile un événement dont le chronométrage vient d'être décidé.
     *
     * @param event l'instance, retenue pour reconnaître sa clôture
     * @param nowNs horodatage d'ouverture
     */
    public void beginTimed(Object event, long nowNs) {
        if (depth >= MAX_NESTING) {
            return;
        }
        timedEvent[depth] = event;
        timedStart[depth] = nowNs;
        depth++;
    }

    /**
     * Indique si cette instance est en cours de chronométrage.
     *
     * <p>Une comparaison de références sur au plus seize cases : bien moins cher qu'une
     * lecture d'horloge, que l'appelant s'épargne ainsi quand l'événement n'est pas
     * échantillonné.
     *
     * @param event l'instance qui se clôt
     * @return {@code true} si l'appelant doit lire l'horloge
     */
    public boolean isTimed(Object event) {
        for (int i = depth - 1; i >= 0; i--) {
            if (timedEvent[i] == event) {
                return true;
            }
        }
        return false;
    }

    /**
     * Clôt l'observation d'un événement.
     *
     * <p>Cherche l'instance dans la pile plutôt que de dépiler aveuglément : un
     * gestionnaire qui lève interrompt la distribution, et la clôture de l'événement
     * qu'il traitait n'arrive jamais. Sans cette recherche, une seule exception
     * désynchroniserait la pile pour le reste de la partie.
     *
     * @param type type de l'événement
     * @param event l'instance qui se clôt
     * @param nowNs horodatage, ou {@code 0} si l'appelant n'a pas chronométré
     * @param canceled {@code true} si l'événement a été annulé
     * @param hasResult {@code true} si l'événement porte un résultat non neutre
     */
    public void end(Class<?> type, Object event, long nowNs, boolean canceled, boolean hasResult) {
        EventStats entry = stats.get(type);
        if (canceled) {
            entry.canceled++;
        }
        if (hasResult) {
            // R-332 : un événement porteur de résultat est ORDER_SENSITIVE par défaut.
            entry.withResult++;
        }

        if (depth == 0 || nowNs == 0L) {
            return;
        }
        for (int i = depth - 1; i >= 0; i--) {
            if (timedEvent[i] != event) {
                continue;
            }
            long elapsed = nowNs - timedStart[i];
            if (elapsed > 0) {
                entry.timed++;
                entry.totalNs += elapsed;
                if (elapsed > entry.maxNs) {
                    entry.maxNs = elapsed;
                }
                timed++;
            }
            // Ce qui était empilé au-dessus a été abandonné : une exception a
            // interrompu la distribution. On le compte plutôt que de le taire.
            abandoned += depth - 1 - i;
            for (int j = i; j < depth; j++) {
                timedEvent[j] = null;
            }
            depth = i;
            return;
        }
    }

    /** Remet la pile à zéro sans toucher aux compteurs, après une anomalie. */
    public void resetNesting() {
        for (int i = 0; i < depth; i++) {
            timedEvent[i] = null;
        }
        depth = 0;
    }

    /** @return les compteurs par type, dans l'ordre d'apparition */
    public List<EventStats> snapshot() {
        synchronized (known) {
            return List.copyOf(known);
        }
    }

    /** @return le nombre d'événements observés, toutes natures confondues */
    public long dispatched() {
        return dispatched;
    }

    /** @return le nombre d'événements effectivement chronométrés */
    public long timed() {
        return timed;
    }

    /** @return le nombre de chronométrages perdus parce qu'une distribution a été interrompue */
    public long abandoned() {
        return abandoned;
    }

    /** @return le nombre de types d'événements vus passer */
    public int knownTypes() {
        synchronized (known) {
            return known.size();
        }
    }

    /** @return la profondeur d'imbrication courante, pour les diagnostics */
    int depth() {
        return depth;
    }

    /**
     * Compteurs d'un type d'événement (`rfx.events.dispatch_ns{event_type}`).
     *
     * <p>Champs mutés depuis le seul fil autoritatif : ni volatile, ni atomique. Les
     * lire depuis un autre fil peut rendre une valeur légèrement en retard, ce qui est
     * sans conséquence pour un affichage de diagnostic.
     */
    public static final class EventStats {

        private final String type;
        private long dispatched;
        private long timed;
        private long totalNs;
        private long maxNs;
        private long canceled;
        private long withResult;

        EventStats(String type) {
            this.type = type;
        }

        /** @return le nom complet de la classe d'événement */
        public String type() {
            return type;
        }

        /** @return le nombre de distributions observées */
        public long dispatched() {
            return dispatched;
        }

        /** @return le nombre de distributions chronométrées */
        public long timed() {
            return timed;
        }

        /** @return le temps cumulé des distributions chronométrées, en nanosecondes */
        public long totalNs() {
            return totalNs;
        }

        /** @return la distribution chronométrée la plus longue, en nanosecondes */
        public long maxNs() {
            return maxNs;
        }

        /** @return le nombre de distributions annulées (`rfx.events.cancels`) */
        public long canceled() {
            return canceled;
        }

        /** @return le nombre de distributions ayant produit un résultat non neutre */
        public long withResult() {
            return withResult;
        }

        /** @return la durée moyenne d'une distribution chronométrée, en nanosecondes */
        public long meanNs() {
            return timed == 0 ? 0 : totalNs / timed;
        }
    }
}
