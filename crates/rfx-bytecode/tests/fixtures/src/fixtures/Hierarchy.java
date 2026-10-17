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

/** Trois sous-classes d'une classe : polymorphe borne. */
abstract class Vehicle {
    abstract int wheels();
}

final class Bike extends Vehicle {
    @Override
    int wheels() {
        return 2;
    }
}

final class Car extends Vehicle {
    @Override
    int wheels() {
        return 4;
    }
}

final class Truck extends Vehicle {
    @Override
    int wheels() {
        return 6;
    }
}

/** Cinq sous-classes d'une classe : polymorphe non borne. */
abstract class Tool {
    abstract void use();
}

final class T1 extends Tool {
    @Override
    void use() {
    }
}

final class T2 extends Tool {
    @Override
    void use() {
    }
}

final class T3 extends Tool {
    @Override
    void use() {
    }
}

final class T4 extends Tool {
    @Override
    void use() {
    }
}

final class T5 extends Tool {
    @Override
    void use() {
    }
}

/** Methode par defaut atteinte par une classe : la seule sous-classe la herite. */
abstract class Speaker implements Greeter {
}

final class Loud extends Speaker {
}

/** Base sans `greet` : elle herite le defaut de `Greeter`. */
abstract class Quiet implements Greeter {
}

/** `super.greet()` : `Quiet` ne la declare pas, la JVM resout le defaut de `Greeter`. */
final class Courteous extends Quiet {
    @Override
    public String greet() {
        return "poliment, " + super.greet();
    }
}

/** Methode par defaut atteinte par une classe, a cote d'une interface du JDK. */
abstract class Labelled implements Named {
}

final class Tag extends Labelled implements Comparable<Tag> {
    @Override
    public int compareTo(Tag other) {
        return 0;
    }
}

/** Par une classe : la seule sous-classe herite sa methode de Thread, hors de l'index. */
abstract class Worker extends Thread {
}

final class Busy extends Worker {
}

/**
 * La methode executee est dans une classe intermediaire : la seule sous-classe concrete
 * n'en declare pas, et il faut remonter d'un cran pour la trouver.
 */
abstract class Top {
    abstract int level();
}

abstract class Middle extends Top {
    @Override
    int level() {
        return 2;
    }
}

final class Leaf extends Middle {
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
