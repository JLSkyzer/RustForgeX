package fixtures;

/**
 * Classes de test de l'analyse de flux de donnees de C-11 (ADR-055). Chaque methode
 * fait un acces dont l'analyse doit retrouver la base : parametre, receveur, objet
 * alloue, valeur lue d'un champ, jonction de deux chemins.
 */
public class Effects {

    static int counter;
    static Object shared;
    int value;
    Object ref;
    int[] numbers;
    Effects next;

    /** Aucun acces memoire. */
    static int pureAdd(int a, int b) {
        return a + b;
    }

    /** Ecriture d'un champ du receveur (T-201). */
    void setValue(int v) {
        this.value = v;
    }

    /** Ecriture d'un champ d'un parametre. */
    static void setOther(Effects other, int v) {
        other.value = v;
    }

    /** Jonction : la base est l'un ou l'autre parametre. */
    static void chooseAndSet(Effects a, Effects b, boolean c) {
        Effects target = c ? a : b;
        target.value = 1;
    }

    /** Alias : le tableau ecrit est celui lu dans un champ du receveur (T-203). */
    void aliasArray(int i) {
        int[] local = this.numbers;
        local[i] = 3;
    }

    /** Tableau alloue dans la methode, rendu. */
    static int[] fresh(int n) {
        int[] array = new int[n];
        array[0] = 1;
        return array;
    }

    /** Un parametre s'echappe dans un statique. */
    static void escape(Object o) {
        shared = o;
    }

    /** Ecriture par une chaine : la base est la valeur lue dans `next`. */
    void chain() {
        this.next.value = 2;
    }

    /** Lecture et ecriture d'un statique. */
    static void bump() {
        counter++;
    }

    /** Appel : le receveur est le parametre, la valeur rendue vient de l'appel. */
    static Object viaCall(Effects e) {
        return e.self();
    }

    /** Rend le receveur. */
    Effects self() {
        return this;
    }

    /** Verrou sur le receveur. */
    void locked() {
        synchronized (this) {
            value++;
        }
    }

    /** Exception allouee et levee hors de tout gestionnaire. */
    static void thrower(Object o) {
        if (o == null) {
            throw new IllegalStateException();
        }
    }

    /** L'exception attrapee est ecrite dans un champ. */
    void caught() {
        try {
            value = 1;
        } catch (RuntimeException e) {
            ref = e;
        }
    }

    /**
     * Variable locale modifiee dans le bloc protege : le gestionnaire peut la voir avec
     * l'une ou l'autre valeur.
     */
    static void storeInTry(Effects a, Effects b) {
        Effects target = a;
        try {
            target = b;
            b.self();
        } catch (RuntimeException e) {
            target.value = 3;
        }
    }

    /** Un gestionnaire d'un autre type ne retient pas l'exception levee. */
    static void rethrowNarrow() {
        try {
            throw new IllegalStateException();
        } catch (IllegalArgumentException e) {
            counter = 0;
        }
    }

    /** Deux retours : la valeur rendue est l'un ou l'autre parametre. */
    static Object pick(Object a, Object b, boolean c) {
        if (c) {
            return a;
        }
        return b;
    }

    /** Boucle : la valeur rendue est un element du tableau, ou null. */
    static Object last(Object[] items) {
        Object last = null;
        for (Object item : items) {
            last = item;
        }
        return last;
    }
}
