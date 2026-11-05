package corpus;

/**
 * Corpus T-204, famille 1 : calculs qui n'écrivent rien hors de leurs objets locaux.
 * Lectures de champs et de statiques permises : seules les écritures sont annotées.
 */
public final class Pure {

    static final int LIMIT = 64;
    static int shared = 7;

    private Pure() {
    }

    @W.Nothing
    static int add(int a, int b) {
        return a + b;
    }

    @W.Nothing
    static long mulLong(long a, long b) {
        return a * b;
    }

    @W.Nothing
    static double lerp(double a, double b, double t) {
        return a + (b - a) * t;
    }

    @W.Nothing
    static int clamp(int v, int lo, int hi) {
        return v < lo ? lo : (v > hi ? hi : v);
    }

    @W.Nothing
    static int abs(int v) {
        return v < 0 ? -v : v;
    }

    @W.Nothing
    static int sumTo(int n) {
        int total = 0;
        for (int i = 1; i <= n; i++) {
            total += i;
        }
        return total;
    }

    @W.Nothing
    static int factorial(int n) {
        int r = 1;
        while (n > 1) {
            r *= n--;
        }
        return r;
    }

    @W.Nothing
    static boolean isPowerOfTwo(int v) {
        return v > 0 && (v & (v - 1)) == 0;
    }

    @W.Nothing
    static int bits(int v) {
        int count = 0;
        while (v != 0) {
            count += v & 1;
            v >>>= 1;
        }
        return count;
    }

    @W.Nothing
    static int readsStatic() {
        return shared * 2 + LIMIT;
    }

    @W.Nothing
    static int switchOf(int kind) {
        switch (kind) {
            case 0:
                return 10;
            case 1:
                return 20;
            case 7:
                return 70;
            default:
                return -1;
        }
    }

    @W.Nothing
    static int sparseSwitch(int key) {
        switch (key) {
            case 3:
                return 1;
            case 1000:
                return 2;
            case 99999:
                return 3;
            default:
                return 0;
        }
    }

    @W.Nothing
    static int localArraySum() {
        int[] values = new int[8];
        for (int i = 0; i < values.length; i++) {
            values[i] = i * i;
        }
        int total = 0;
        for (int v : values) {
            total += v;
        }
        return total;
    }

    @W.Nothing
    static int localMatrix() {
        int[][] m = new int[3][3];
        m[1][2] = 5;
        return m[1][2];
    }

    @W.Nothing
    static int localObject() {
        Point p = new Point(1, 2);
        p.x = 4;
        return p.x + p.y;
    }

    @W.Nothing
    static int localChain() {
        Point a = new Point(0, 0);
        Point b = new Point(1, 1);
        a.next = b;
        b.x = 9;
        return a.next.x;
    }

    @W.Nothing
    static double distance(double ax, double ay, double bx, double by) {
        double dx = ax - bx;
        double dy = ay - by;
        return dx * dx + dy * dy;
    }

    @W.Nothing
    static int maxOf(int[] values) {
        int best = Integer.MIN_VALUE;
        for (int v : values) {
            if (v > best) {
                best = v;
            }
        }
        return best;
    }

    @W.Nothing
    static int readsField(Point p) {
        return p.x * p.y;
    }

    @W.Nothing
    static int ternaryChain(int a, int b, int c) {
        return a > b ? (a > c ? a : c) : (b > c ? b : c);
    }

    @W.Nothing
    static long shifts(long v) {
        return (v << 3) ^ (v >> 2) ^ (v >>> 5);
    }

    @W.Nothing
    static float floats(float a, float b) {
        return a / (b == 0f ? 1f : b);
    }

    @W.Nothing
    static String concat(int value) {
        return "valeur=" + value;
    }

    @W.Nothing
    static int caughtLocally(int v) {
        try {
            if (v < 0) {
                throw new IllegalStateException();
            }
            return v;
        } catch (IllegalStateException e) {
            return 0;
        }
    }

    @W.Nothing
    static int nestedLoops(int n) {
        int count = 0;
        for (int i = 0; i < n; i++) {
            for (int j = i; j < n; j++) {
                if ((i + j) % 3 == 0) {
                    count++;
                }
            }
        }
        return count;
    }

    /** Objet du corpus, sans domaine de jeu. */
    static final class Point {
        int x;
        int y;
        Point next;

        Point(int x, int y) {
            this.x = x;
            this.y = y;
        }
    }
}
