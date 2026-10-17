package fixtures;

import java.util.List;

/**
 * Classes de test de la phase interprocedurale de C-11 (ADR-055). Chaque methode
 * statique sans parametre est une racine dont l'analyse doit retrouver les effets, en
 * suivant ses appels.
 */
public class Interproc {

    static Object shared;
    int value;
    Object ref;

    Interproc() {
    }

    Interproc(int v) {
        this.value = v;
    }

    /** Objet construit et lu sur place : rien ne s'echappe, aucun domaine touche. */
    static int localObject() {
        Interproc p = new Interproc(5);
        return p.value;
    }

    /** Fabrique : rend un objet frais. */
    static Interproc factory() {
        return new Interproc(1);
    }

    /** L'objet de la fabrique reste local a l'appelant. */
    static int viaFactory() {
        Interproc p = factory();
        p.value = 2;
        return p.value;
    }

    /** L'objet construit est publie dans un statique. */
    static void publish() {
        Interproc p = new Interproc(3);
        shared = p;
    }

    /** Ecrit un champ de l'objet passe. */
    static void setOn(Interproc other) {
        other.value = 1;
    }

    /** L'ecriture de l'appele porte sur un objet local : aucun domaine. */
    static int setOnLocal() {
        Interproc p = new Interproc();
        setOn(p);
        return p.value;
    }

    /** L'ecriture de l'appele porte sur un objet partage. */
    static void setOnShared() {
        setOn((Interproc) shared);
    }

    /** Publie l'objet passe. */
    static void store(Object o) {
        shared = o;
    }

    /** L'objet s'echappe par l'appele : l'ecriture qui suit porte sur un objet partage. */
    static void escapesViaCallee() {
        Interproc p = new Interproc();
        store(p);
        p.value = 4;
    }

    /** Rend son receveur. */
    Interproc self() {
        return this;
    }

    /** L'objet rendu est le receveur local : il reste local. */
    static int selfLocal() {
        Interproc p = new Interproc();
        p.self().value = 5;
        return p.value;
    }

    /** Recursion : converge vers ses effets reels, aucun. */
    static int countdown(int n) {
        return n <= 0 ? 0 : countdown(n - 1);
    }

    /** Racine de la recursion. */
    static int recursive() {
        return countdown(3);
    }

    /** Range l'objet passe dans un objet neuf, rendu. */
    static Interproc wrap(Object o) {
        Interproc holder = new Interproc();
        holder.ref = o;
        return holder;
    }

    /** L'objet interieur est atteignable par l'emballage publie : il n'est plus local. */
    static void wrapLeak() {
        Interproc inner = new Interproc();
        Interproc holder = wrap(inner);
        shared = holder;
        inner.value = 6;
    }

    /** Une lambda creee et appelee par une interface : appel non resolu. */
    static void lambda() {
        Runnable r = () -> { };
        r.run();
    }

    /** Lambda creee seulement : aucun effet. */
    static Runnable lambdaOnly() {
        return () -> { };
    }

    /** Concatenation de primitives et de chaines : aucun effet. */
    static String concat() {
        int a = 1;
        String b = "b";
        return "x" + a + b;
    }

    /** Concatenation d'un objet : son toString est arbitraire. */
    static String concatObject() {
        return "x" + shared;
    }

    /** Transmet son parametre a un appele qui y ecrit. */
    static void forward(Interproc o) {
        setOn(o);
    }

    /** Lambda dont le corps ecrit un statique : sa creation seule n'a pas d'effet. */
    static Runnable lambdaWrites() {
        return () -> {
            shared = null;
        };
    }

    /** Transmet son parametre a `second`, qui le publie. */
    static void first(Object o) {
        second(o);
    }

    /** Publie son parametre. */
    static void second(Object o) {
        shared = o;
    }

    /**
     * `first` et `second` sont a la meme profondeur, `first` traite avant `second` : le
     * point fixe doit repasser sur `first` quand le resume de `second` change.
     */
    static void twoCallers() {
        Interproc p = new Interproc();
        first(p);
        second(null);
        p.value = 1;
    }

    /** Appel par une interface ouverte : non resolu. */
    static void openInterface() {
        List<Object> list = List.of();
        list.size();
    }
}
