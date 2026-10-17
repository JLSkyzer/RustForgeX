package fixtures;

/**
 * Classes de test du decodeur d'instructions de C-11 (ADR-055). Chaque methode fait
 * emettre a {@code javac} une famille d'instructions dont l'effet sur la pile doit etre
 * exact : operations de pile sur des valeurs de categorie 2, gestionnaires
 * d'exceptions, moniteurs, tableaux, constantes, conversions.
 */
public class Flow {

    static int counter;
    static long total;
    int value;
    long wide;

    /** dup2 (indice de tableau), laload, ladd, lastore. */
    static void addToLong(long[] array, int i) {
        array[i] += 5L;
    }

    /** dup2_x2 : affectation chainee d'un element de tableau long. */
    static long chainLongArray(long[] array, int i, long v) {
        long x = array[i] = v;
        return x;
    }

    /** dup_x1 : affectation chainee d'un champ int. */
    int chainField(int v) {
        int y = this.value = v;
        return y;
    }

    /** dup_x2 : affectation chainee d'un element de tableau int. */
    static int chainIntArray(int[] array, int i, int v) {
        int z = array[i] = v;
        return z;
    }

    /** dup2_x1 : affectation chainee d'un champ long. */
    long chainWideField(long v) {
        long w = this.wide = v;
        return w;
    }

    /** pop2 : valeur longue ignoree ; pop : valeur int ignoree. */
    static void discard(Long boxed, Integer small) {
        boxed.longValue();
        small.intValue();
    }

    /** Champs statiques, dont un long ; iinc. */
    static long statics(int n) {
        for (int i = 0; i < n; i++) {
            counter++;
            total += i;
        }
        return total;
    }

    /** try / catch / finally : plusieurs gestionnaires, dont un sans type. */
    static int guarded(String text) {
        int result;
        try {
            result = Integer.parseInt(text);
        } catch (NumberFormatException | NullPointerException e) {
            result = -1;
        } finally {
            counter++;
        }
        return result;
    }

    /** monitorenter, monitorexit, gestionnaire implicite. */
    void locked(Object lock) {
        synchronized (lock) {
            value++;
        }
    }

    /** newarray, anewarray, multianewarray, arraylength. */
    static int arrays(int n) {
        int[] ints = new int[n];
        String[] names = new String[n];
        long[][] grid = new long[n][n];
        return ints.length + names.length + grid.length;
    }

    /** ldc d'un entier, d'un flottant, d'une chaine, d'une classe ; ldc2_w. */
    static Object constants(int choice) {
        switch (choice) {
            case 0:
                return 123_456;
            case 1:
                return 1.5f;
            case 2:
                return "texte";
            case 3:
                return Flow.class;
            case 4:
                return 9_876_543_210L;
            default:
                return 2.25;
        }
    }

    /** lookupswitch, instanceof, checkcast, athrow. */
    static int dispatch(Object o, int key) {
        switch (key) {
            case 10:
                return 1;
            case 1000:
                return 2;
            case 100_000:
                if (o instanceof String) {
                    return ((String) o).length();
                }
                throw new IllegalArgumentException();
            default:
                return 0;
        }
    }

    /** Conversions et comparaisons, valeurs longues aux jonctions. */
    static double conversions(int i, long l, float f, double d) {
        long a = i;
        double b = l;
        int c = (int) d;
        long e = (long) f;
        float g = (float) d;
        long joined = l > a ? l : a;
        boolean less = d < b;
        return a + b + c + e + g + joined + (less ? 1 : 0) + (short) i + (byte) i + (char) i;
    }

    /** Plus de 256 emplacements locaux : prefixe wide sur les chargements et iinc. */
    static int manyLocals(int seed) {
        int v0 = seed, v1 = v0 + 1, v2 = v1 + 1, v3 = v2 + 1, v4 = v3 + 1;
        int[] spill = new int[300];
        long sum = 0;
        for (int k = 0; k < spill.length; k++) {
            sum += spill[k];
        }
        int a000 = 0, a001 = 0, a002 = 0, a003 = 0, a004 = 0, a005 = 0, a006 = 0, a007 = 0;
        int a010 = 0, a011 = 0, a012 = 0, a013 = 0, a014 = 0, a015 = 0, a016 = 0, a017 = 0;
        int a020 = 0, a021 = 0, a022 = 0, a023 = 0, a024 = 0, a025 = 0, a026 = 0, a027 = 0;
        int a030 = 0, a031 = 0, a032 = 0, a033 = 0, a034 = 0, a035 = 0, a036 = 0, a037 = 0;
        long b000 = 0, b001 = 0, b002 = 0, b003 = 0, b004 = 0, b005 = 0, b006 = 0, b007 = 0;
        long b010 = 0, b011 = 0, b012 = 0, b013 = 0, b014 = 0, b015 = 0, b016 = 0, b017 = 0;
        long b020 = 0, b021 = 0, b022 = 0, b023 = 0, b024 = 0, b025 = 0, b026 = 0, b027 = 0;
        long b030 = 0, b031 = 0, b032 = 0, b033 = 0, b034 = 0, b035 = 0, b036 = 0, b037 = 0;
        long b040 = 0, b041 = 0, b042 = 0, b043 = 0, b044 = 0, b045 = 0, b046 = 0, b047 = 0;
        long b050 = 0, b051 = 0, b052 = 0, b053 = 0, b054 = 0, b055 = 0, b056 = 0, b057 = 0;
        long b060 = 0, b061 = 0, b062 = 0, b063 = 0, b064 = 0, b065 = 0, b066 = 0, b067 = 0;
        long b070 = 0, b071 = 0, b072 = 0, b073 = 0, b074 = 0, b075 = 0, b076 = 0, b077 = 0;
        long b100 = 0, b101 = 0, b102 = 0, b103 = 0, b104 = 0, b105 = 0, b106 = 0, b107 = 0;
        long b110 = 0, b111 = 0, b112 = 0, b113 = 0, b114 = 0, b115 = 0, b116 = 0, b117 = 0;
        long b120 = 0, b121 = 0, b122 = 0, b123 = 0, b124 = 0, b125 = 0, b126 = 0, b127 = 0;
        long b130 = 0, b131 = 0, b132 = 0, b133 = 0, b134 = 0, b135 = 0, b136 = 0, b137 = 0;
        long b140 = 0, b141 = 0, b142 = 0, b143 = 0, b144 = 0, b145 = 0, b146 = 0, b147 = 0;
        long b150 = 0, b151 = 0, b152 = 0, b153 = 0, b154 = 0, b155 = 0, b156 = 0, b157 = 0;
        int far = v4 + (int) sum;
        far += 1000;
        double farDouble = far;
        Object farRef = names(far);
        return far + (int) farDouble + farRef.hashCode()
                + a000 + a037 + (int) b000 + (int) b157;
    }

    private static Object names(int n) {
        return Integer.toString(n);
    }
}
