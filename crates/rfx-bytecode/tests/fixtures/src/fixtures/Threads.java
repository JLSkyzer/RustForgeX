package fixtures;

/**
 * Classes de test de C-12 (ADR-057) : contraintes de fil et confinement derive. Les
 * classes dont on teste le confinement sont de premier niveau, hors de tout nid, sauf
 * {@link Threads.InnerPrivate} qui en appartient a un expres.
 */
public class Threads {

    static final ThreadLocal<Integer> LOCAL = ThreadLocal.withInitial(() -> 0);

    /** Champ prive ecrit seulement par le constructeur, mais dans un nid. */
    public static final class InnerPrivate {
        private int value;

        InnerPrivate(int v) {
            value = v;
        }

        int value() {
            return value;
        }
    }

    /** T-210 : lit une variable locale de fil. */
    static int readLocal() {
        return LOCAL.get();
    }

    /** Ecrit le champ d'un objet partage d'une classe non sure. */
    static void touchShared(Counter c) {
        c.value++;
    }

    /** Lit un objet partage d'une classe sure, sans rien ecrire. */
    static int readFrozen(Frozen f) {
        return f.value();
    }

    /** Methode synchronisee. */
    static synchronized void syncMethod() {
        LOCAL.hashCode();
    }

    /** Attend sur un moniteur. */
    static void waits(Object o) throws InterruptedException {
        synchronized (o) {
            o.wait();
        }
    }
}

/** Champ public mutable : non sur. */
class Counter {
    public int value;
}

/** Champ final : sur. */
final class Frozen {
    private final int value;

    Frozen(int v) {
        value = v;
    }

    int value() {
        return value;
    }
}

/** Champ prive ecrit hors du constructeur, sans synchronisation : non sur. */
final class Mutating {
    private int value;

    void bump() {
        value++;
    }
}

/** Champ prive ecrit seulement dans une methode synchronisee : sur. */
final class Guarded {
    private int value;

    synchronized void bump() {
        value++;
    }

    synchronized int get() {
        return value;
    }
}

/** Champ prive ecrit seulement par le constructeur, hors de tout nid : sur. */
final class CtorOnly {
    private int value;

    CtorOnly(int v) {
        value = v;
    }

    int get() {
        return value;
    }
}

/** Herite d'une classe non sure : non sure. */
final class CounterChild extends Counter {
    private final int extra = 1;
}
