package corpus;

import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;

/**
 * Corpus T-204, famille 5 : effets portés par les appelés — aides, constructeurs,
 * fabriques, récursion, échappement par un appelé.
 */
public final class Calls {

    static int hits;
    static Node head;

    private Calls() {
    }

    private static int square(int v) {
        return v * v;
    }

    private static void hit() {
        hits++;
    }

    private static void moveBy(Entity e, double dx) {
        e.x += dx;
    }

    private static Node make(int v) {
        Node n = new Node();
        n.value = v;
        return n;
    }

    private static void fill(Node n, int v) {
        n.value = v;
    }

    private static Node identity(Node n) {
        return n;
    }

    @W.Nothing
    static int callsPure(int v) {
        return square(v) + square(v + 1);
    }

    @W.ModStatics
    static void callsHit() {
        hit();
    }

    @W.ModStatics
    static void callsHitTwice(boolean b) {
        hit();
        if (b) {
            hit();
        }
    }

    @W.EntityState
    static void callsMove(Entity e) {
        moveBy(e, 0.5);
    }

    @W.EntityState
    static void callsMoveOnLiving(LivingEntity e) {
        moveBy(e, 1.5);
    }

    @W.Nothing
    static int factoryLocal() {
        Node n = make(3);
        n.value++;
        return n.value;
    }

    @W.Nothing
    static int fillLocal() {
        Node n = new Node();
        fill(n, 4);
        return n.value;
    }

    @W.Unknown
    static void fillParam(Node n) {
        fill(n, 5);
    }

    @W.ModStatics
    static void pushFresh(int v) {
        Node n = make(v);
        n.next = head;
        head = n;
    }

    @W.Unknown
    static void writeHead() {
        head.value = 9;
    }

    @W.Unknown
    static void writeThroughIdentity(Node n) {
        identity(n).value = 1;
    }

    @W.Nothing
    static int identityLocal() {
        Node n = identity(new Node());
        n.value = 2;
        return n.value;
    }

    @W.Nothing
    static int recursiveSum(int n) {
        return n <= 0 ? 0 : n + recursiveSum(n - 1);
    }

    @W.ModStatics
    static void recursiveHits(int n) {
        if (n > 0) {
            hits++;
            recursiveHits(n - 1);
        }
    }

    @W.Nothing
    static boolean isEven(int n) {
        return n == 0 || isOdd(n - 1);
    }

    @W.Nothing
    static boolean isOdd(int n) {
        return n != 0 && isEven(n - 1);
    }

    @W.Unknown
    static void walk(Node n) {
        while (n != null) {
            n.value = 0;
            n = n.next;
        }
    }

    @W.Nothing
    static int ctorWritesItself() {
        Counter c = new Counter(5);
        return c.count;
    }

    @W.ModStatics
    static int ctorWritesStatic() {
        Registered r = new Registered();
        return r.id;
    }

    @W.Unknown
    static void instanceCall(Counter c) {
        c.add(2);
    }

    @W.Nothing
    static int instanceLocal() {
        Counter c = new Counter(0);
        c.add(3);
        return c.count;
    }

    @W.EntityState
    static void deepChain(Entity e) {
        level1(e);
    }

    private static void level1(Entity e) {
        level2(e);
    }

    private static void level2(Entity e) {
        level3(e);
    }

    private static void level3(Entity e) {
        e.age = 0;
    }

    @W.EntityState
    @W.ModStatics
    static void mixed(Entity e) {
        moveBy(e, 1.0);
        hit();
    }

    @W.Unknown
    static void escapesViaCallee(Node n) {
        Node fresh = new Node();
        link(n, fresh);
        fresh.value = 3;
    }

    private static void link(Node from, Node to) {
        from.next = to;
    }

    @W.Nothing
    static int staticInitRead() {
        return Counter.START;
    }

    /** Noeud du corpus, sans domaine de jeu. */
    static final class Node {
        int value;
        Node next;
    }

    /** Compteur dont le constructeur écrit son propre objet. */
    static final class Counter {
        static final int START = 1;
        int count;

        Counter(int start) {
            this.count = start;
        }

        void add(int v) {
            count += v;
        }
    }

    /** Objet dont le constructeur écrit un statique. */
    static final class Registered {
        static int nextId;
        final int id;

        Registered() {
            this.id = nextId++;
        }
    }
}
