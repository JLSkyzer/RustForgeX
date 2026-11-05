package corpus;

import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.function.IntUnaryOperator;

import net.minecraft.world.entity.Entity;

/**
 * Corpus T-204, famille 7 : ce que l'analyse statique ne peut pas suivre entièrement —
 * lambdas, réflexion, collections et méthodes du JDK, code natif, verrous, exceptions
 * propagées. La vérité reste ce que la méthode écrit réellement.
 */
public final class Hard {

    static int counter;
    static final Object LOCK = new Object();
    static List<Entity> registry = new ArrayList<>();
    static Map<String, Integer> scores = new HashMap<>();

    private Hard() {
    }

    @W.ModStatics
    static void viaLambda() {
        Runnable r = () -> counter++;
        r.run();
    }

    @W.Nothing
    static int pureLambda(int v) {
        IntUnaryOperator twice = x -> x * 2;
        return twice.applyAsInt(v);
    }

    @W.EntityState
    static void viaMethodReference(Entity e) {
        Runnable r = e::tick;
        r.run();
    }

    @W.EntityState
    static void viaReflection(Entity e) throws ReflectiveOperationException {
        Field f = Entity.class.getField("age");
        f.setInt(e, 3);
    }

    @W.Unknown
    static void addToList(List<Entity> list, Entity e) {
        list.add(e);
    }

    @W.Unknown
    static void addToRegistry(Entity e) {
        registry.add(e);
    }

    @W.Unknown
    static void putScore(String name) {
        scores.put(name, 1);
    }

    @W.Nothing
    static int jdkMath(int a, int b) {
        return Math.max(a, b) + Math.abs(a - b);
    }

    @W.Nothing
    static int localList() {
        List<Integer> values = new ArrayList<>();
        values.add(1);
        values.add(2);
        return values.size();
    }

    @W.ModStatics
    static void synchronizedWrite() {
        synchronized (LOCK) {
            counter++;
        }
    }

    @W.ModStatics
    static synchronized void synchronizedMethod() {
        counter--;
    }

    @W.Nothing
    static int nativeHash(Object o) {
        return System.identityHashCode(o);
    }

    @W.ModStatics
    static void throwsAfterWrite(int v) {
        counter = v;
        if (v < 0) {
            throw new IllegalArgumentException("négatif");
        }
    }

    @W.EntityState
    static void iterate(List<Entity> entities) {
        for (Entity e : entities) {
            e.removed = false;
        }
    }

    @W.Nothing
    static String format(int v) {
        return String.valueOf(v);
    }
}
