package fixtures;

/** Hierarchies de test de C-07 (ADR-050). */
final class Hierarchy {
    private Hierarchy() {
    }
}

/** Une seule implementation chargee : l'appel est monomorphe. */
abstract class Animal {
    abstract String sound();
}

class Dog extends Animal {
    @Override
    String sound() {
        return bark();
    }

    private String bark() {
        return "ouaf";
    }
}

/** Trois implementations : polymorphe borne. */
interface Shape {
    double area();
}

final class Circle implements Shape {
    @Override
    public double area() {
        return 3.14;
    }
}

final class Square implements Shape {
    @Override
    public double area() {
        return 1.0;
    }
}

final class Triangle implements Shape {
    @Override
    public double area() {
        return 0.5;
    }
}

/** Cinq implementations : polymorphe non borne. */
interface Handler {
    void handle();
}

final class H1 implements Handler {
    @Override
    public void handle() {
    }
}

final class H2 implements Handler {
    @Override
    public void handle() {
    }
}

final class H3 implements Handler {
    @Override
    public void handle() {
    }
}

final class H4 implements Handler {
    @Override
    public void handle() {
    }
}

final class H5 implements Handler {
    @Override
    public void handle() {
    }
}

/** Methode par defaut, heritee par la seule implementation. */
interface Greeter {
    default String greet() {
        return "bonjour";
    }
}

final class Polite implements Greeter {
}

/**
 * Seule implementation chargee, mais sa methode vient d'une classe du JDK absente de
 * l'index : l'appel ne doit pas etre declare resolu.
 */
interface Job {
    void run();
}

final class ThreadJob extends Thread implements Job {
}

/**
 * Methode par defaut connue, mais la classe implemente aussi une interface du JDK
 * absente de l'index, qui pourrait en fournir une plus specifique : rien n'est affirme.
 */
interface Named {
    default String label() {
        return "nom";
    }
}

final class Item implements Named, Comparable<Item> {
    @Override
    public int compareTo(Item other) {
        return 0;
    }
}

/** Appel a la super-classe : invokespecial vers la methode heritee. */
class Base {
    int value() {
        return 1;
    }
}

final class Derived extends Base {
    @Override
    int value() {
        return super.value() + 1;
    }
}
