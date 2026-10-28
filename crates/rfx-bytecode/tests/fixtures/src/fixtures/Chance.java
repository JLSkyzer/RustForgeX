package fixtures;

import java.io.File;
import java.lang.reflect.Method;
import java.util.HashMap;
import java.util.Random;
import java.util.concurrent.locks.Lock;
import java.util.function.IntUnaryOperator;

/**
 * Classes de test de C-13 (ADR-058) : une methode par source de non-determinisme, et des
 * methodes sans source, entieres ou flottantes, en lecture seule ou qui ecrivent.
 */
public class Chance {

    static int last;

    /** Generateur pseudo-aleatoire. */
    static int roll(Random random) {
        return random.nextInt();
    }

    /** Horloge. */
    static long now() {
        return System.nanoTime();
    }

    /** Hachage d'identite. */
    static int identity(Object o) {
        return System.identityHashCode(o);
    }

    /** Iteration d'une table de hachage. */
    static int keys(HashMap<String, Integer> map) {
        return map.keySet().size();
    }

    /** Entree-sortie. */
    static boolean exists(File file) {
        return file.exists();
    }

    /** Verrou. */
    static void locked(Lock lock) {
        lock.lock();
    }

    /** Reflexion. */
    static Object reflect(Method method) throws Exception {
        return method.invoke(null);
    }

    /** Methode native. */
    static native int nativeCall();

    /** Appelle une methode native. */
    static int callsNative() {
        return nativeCall() + 1;
    }

    /** Aucune source, arithmetique entiere, lecture seule. */
    static int pure(int a, int b) {
        return a * b + helper(a);
    }

    private static int helper(int a) {
        return a - 1;
    }

    /** Aucune source, arithmetique flottante. */
    static double sum(double a, double b) {
        return a + b;
    }

    /** Aucune source, ecrit un statique. */
    static void store(int value) {
        last = value;
    }

    /** Statique, mais un tableau est un objet : il peut porter le flux (ADR-060). */
    static int first(int[] values) {
        return values[0];
    }

    /** Statique a parametre primitif, mais transmise comme rappel par sa classe. */
    static int doubled(int value) {
        return value * 2;
    }

    /** Reference de methode vers `doubled` (ADR-060). */
    static IntUnaryOperator twice() {
        return Chance::doubled;
    }
}
