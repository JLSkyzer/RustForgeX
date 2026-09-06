package rfxbench.synth;

/**
 * Une unité de travail synthétique : du calcul, des allocations, un peu d'état.
 *
 * <p>PARTIE 22. Aucune dépendance à Minecraft ni à Forge : le cœur de la charge se teste
 * seul, et ce qui se teste seul se vérifie sans démarrer un serveur.
 *
 * <h2>Pourquoi ces méthodes sont longues</h2>
 *
 * <p>ADR-021 a porté le seuil de sondage à soixante-quatre instructions réelles. Une
 * unité de travail synthétique plus courte que ce seuil ne serait <strong>jamais
 * sondée</strong>, et le mod synthétique ne mesurerait alors rien du coût de
 * l'instrumentation — ce pour quoi il existe. Les corps sont donc délibérément assez
 * longs pour être retenus.
 *
 * <h2>Pourquoi ce n'est pas un mod nommé</h2>
 *
 * <p>R-871 interdit qu'un profil soit conçu pour optimiser un mod nommé. Une charge
 * synthétique paramétrable est le contraire : elle décrit une <em>forme</em> de travail
 * — coût CPU, taux d'allocation, non-déterminisme — sans imiter personne.
 */
public final class SynthWorkload {

    /** Identifiant de l'unité, qui la distingue des autres dans le profiler. */
    private final int id;

    /** État accumulé, pour que le calcul ne soit pas éliminé par le compilateur. */
    private long accumulator;

    private long ticks;
    private long allocatedBytes;

    /**
     * @param id identifiant de l'unité
     */
    public SynthWorkload(int id) {
        this.id = id;
        this.accumulator = id * 2_654_435_761L;
    }

    /**
     * Exécute un tick de cette unité.
     *
     * <p>Le résultat est conservé dans l'accumulateur : sans cela, le JIT supprimerait
     * purement et simplement la boucle, et la charge n'existerait que dans le code
     * source.
     *
     * @param iterations itérations de calcul
     * @param allocationBytes octets à allouer, arrondis au multiple de huit
     * @param nondeterministic {@code true} pour mêler une source non reproductible
     * @return la valeur accumulée, à conserver par l'appelant
     */
    public long tick(int iterations, int allocationBytes, boolean nondeterministic) {
        ticks++;
        long value = accumulator;

        // Calcul pur : un mélange multiplicatif, assez irrégulier pour ne pas être
        // réduit à une forme close par le compilateur.
        for (int i = 0; i < iterations; i++) {
            value ^= value >>> 33;
            value *= 0xff51afd7ed558ccdL;
            value ^= value >>> 29;
            value += id + i;
        }

        if (allocationBytes > 0) {
            // Pression sur le ramasse-miettes. Le tableau est lu pour qu'il ne soit pas
            // éliminé par l'analyse d'échappement.
            long[] scratch = new long[Math.max(1, allocationBytes / Long.BYTES)];
            for (int i = 0; i < scratch.length; i++) {
                scratch[i] = value + i;
            }
            value += scratch[scratch.length - 1];
            allocatedBytes += (long) scratch.length * Long.BYTES;
        }

        if (nondeterministic) {
            // R-870 : une part non reproductible, pour vérifier que RUSTFORGE-X la
            // refuse ou la sérialise plutôt que de la casser.
            value += System.nanoTime() & 0xffL;
        }

        accumulator = value;
        return value;
    }

    /** @return l'identifiant de l'unité */
    public int id() {
        return id;
    }

    /** @return le nombre de ticks exécutés */
    public long ticks() {
        return ticks;
    }

    /** @return les octets alloués depuis le démarrage */
    public long allocatedBytes() {
        return allocatedBytes;
    }

    /** @return la valeur accumulée, qui empêche l'élimination du calcul */
    public long accumulator() {
        return accumulator;
    }
}
