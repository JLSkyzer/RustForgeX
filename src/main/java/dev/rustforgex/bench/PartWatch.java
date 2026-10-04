package dev.rustforgex.bench;

/**
 * C-36 : compte, tick après tick, les changements d'état de chaque partie d'un ouvrage
 * de test ({@link BenchFixture}).
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Ces compteurs prouvent que chaque partie a tourné : une égalité stricte entre trois
 * ouvrages morts ne prouverait rien. Ils ne servent qu'au journal et au fichier ; le
 * verdict, lui, porte sur les empreintes.
 */
final class PartWatch {

    private final String[] names;
    private final long[] last;
    private final long[] changes;
    private boolean primed;

    PartWatch(String... names) {
        this.names = names.clone();
        this.last = new long[names.length];
        this.changes = new long[names.length];
    }

    /**
     * Enregistre l'état des parties, dans l'ordre des noms, et compte celles qui ont
     * changé depuis l'appel précédent.
     */
    void record(long... now) {
        if (now.length != names.length) {
            throw new IllegalArgumentException(
                    names.length + " parties attendues, " + now.length + " relevées");
        }
        for (int i = 0; i < now.length; i++) {
            if (primed && now[i] != last[i]) {
                changes[i]++;
            }
            last[i] = now[i];
        }
        primed = true;
    }

    /** Les compteurs, par exemple {@code "anneau 812, piston-anneau 28"}. */
    String summary() {
        StringBuilder text = new StringBuilder();
        for (int i = 0; i < names.length; i++) {
            text.append(i == 0 ? "" : ", ").append(names[i]).append(' ').append(changes[i]);
        }
        return text.toString();
    }

    /** {@code true} si chaque partie a changé au moins une fois. */
    boolean everyPartMoved() {
        for (long count : changes) {
            if (count == 0) {
                return false;
            }
        }
        return true;
    }
}
