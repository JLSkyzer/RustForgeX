package fixtures;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.lang.reflect.Method;
import java.util.function.Function;

/**
 * Classes de test de C-07 (ADR-050). Chaque methode exerce un cas de la fiche 5.7.
 * Compilees par {@code javac --release 17} : voir {@code ../README.md}.
 */
public class Calls {

    // --- appels resolus statiquement --------------------------------------------

    static int helper(int x) {
        return x + 1;
    }

    private int secret() {
        return 7;
    }

    public final int sealed() {
        return 3;
    }

    /** invokestatic, invokespecial (prive, constructeur), invokevirtual sur final. */
    public int direct() {
        Calls other = new Calls();
        return helper(1) + secret() + other.sealed();
    }

    // --- monomorphe : une seule implementation chargee ----------------------------

    /** {@code Animal.sound} n'a qu'une implementation chargee : {@code Dog.sound}. */
    public static String monomorphic(Animal animal) {
        return animal.sound();
    }

    // --- polymorphe borne : trois implementations ---------------------------------

    public static double bounded(Shape shape) {
        return shape.area();
    }

    // --- polymorphe non borne : cinq implementations ------------------------------

    public static void unbounded(Handler handler) {
        handler.handle();
    }

    // --- lambda et reference de methode -------------------------------------------

    public static int lambda() {
        Function<Integer, Integer> f = x -> helper(x);
        Function<String, Integer> parse = Integer::parseInt;
        return f.apply(2) + parse.apply("3");
    }

    /** References de methode virtuelles : la cible depend du receveur. */
    public static int methodReferences(Dog dog) {
        Function<Dog, String> sound = Dog::sound;
        Function<String, Integer> length = String::length;
        return length.apply(sound.apply(dog));
    }

    // --- memes cas, par une classe : la hierarchie chargee peut trancher ----------

    public static int vehicle(Vehicle vehicle) {
        return vehicle.wheels();
    }

    /** Par `Object` : toute classe est un receveur. */
    public static int objectHash(Object value) {
        return value.hashCode();
    }

    /** Deux hierarchies de classes dans le meme graphe : chacune a ses sous-classes. */
    public static int twoHierarchies(Vehicle vehicle, Tool tool) {
        tool(tool);
        return vehicle(vehicle);
    }

    public static void tool(Tool tool) {
        tool.use();
    }

    public static String speak(Speaker speaker) {
        return speaker.greet();
    }

    public static String labelled(Labelled labelled) {
        return labelled.label();
    }

    public static void work(Worker worker) {
        worker.run();
    }

    public static int inherited(Top top) {
        return top.level();
    }

    /** Methode par defaut, mais une interface du JDK pourrait en fournir une autre. */
    public static String label(Named named) {
        return named.label();
    }

    /** L'implementation unique herite sa methode d'une classe du JDK. */
    public static void runJob(Job job) {
        job.run();
    }

    // --- concatenation : invokedynamic qui n'est pas une lambda --------------------

    public static String concat(String a, int b) {
        return a + b;
    }

    // --- reflexion ----------------------------------------------------------------

    public static Object reflective() throws Throwable {
        Method method = Calls.class.getDeclaredMethod("helper", int.class);
        Object viaReflection = method.invoke(null, 1);
        MethodHandle handle = MethodHandles.lookup()
                .findStatic(Calls.class, "helper", MethodType.methodType(int.class, int.class));
        int viaHandle = (int) handle.invokeExact(2);
        return viaReflection.toString() + viaHandle;
    }

    // --- recursion ----------------------------------------------------------------

    public static long factorial(int n) {
        return n <= 1 ? 1 : n * factorial(n - 1);
    }

    public static boolean isEven(int n) {
        return n == 0 || isOdd(n - 1);
    }

    public static boolean isOdd(int n) {
        return n != 0 && isEven(n - 1);
    }

    // --- interface : methode par defaut -------------------------------------------

    public static String greet(Greeter greeter) {
        return greeter.greet();
    }

    // --- instructions a longueur variable -----------------------------------------

    /** tableswitch (dense), lookupswitch (clairseme), iinc large (wide). */
    public static int switches(int key) {
        int total = 0;
        switch (key) {
            case 0: total = helper(0); break;
            case 1: total = helper(1); break;
            case 2: total = helper(2); break;
            case 3: total = helper(3); break;
            default: total = -1;
        }
        switch (key) {
            case 10: total += 1; break;
            case 1_000: total += 2; break;
            case 100_000: total += 3; break;
            default: break;
        }
        total += 1_000;
        return (int) factorial(total);
    }

    /**
     * Les memes instructions, decalees : le tableswitch commence a une position non
     * alignee, et des octets de remplissage le precedent (verifie par javap).
     */
    public static int padded(int key) {
        int shifted = key + 1;
        switch (shifted) {
            case 0: return helper(10);
            case 1: return helper(11);
            case 2: return helper(12);
            default: return sparse(shifted);
        }
    }

    /** Le lookupswitch decale, de meme. */
    static int sparse(int key) {
        int shifted = key * 3;
        switch (shifted) {
            case 7: return helper(1);
            case 70_000: return helper(2);
            default: return 0;
        }
    }
}
