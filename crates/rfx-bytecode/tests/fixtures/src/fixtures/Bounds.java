package fixtures;

/** Bornes de l'exploration (R-340) : une longue chaine, et un large eventail. */
public final class Bounds {

    private Bounds() {
    }

    // Chaine de vingt appels : au-dela de max_call_depth (12).
    public static int c0() { return c1() + 1; }
    static int c1() { return c2() + 1; }
    static int c2() { return c3() + 1; }
    static int c3() { return c4() + 1; }
    static int c4() { return c5() + 1; }
    static int c5() { return c6() + 1; }
    static int c6() { return c7() + 1; }
    static int c7() { return c8() + 1; }
    static int c8() { return c9() + 1; }
    static int c9() { return c10() + 1; }
    static int c10() { return c11() + 1; }
    static int c11() { return c12() + 1; }
    static int c12() { return c13() + 1; }
    static int c13() { return c14() + 1; }
    static int c14() { return c15() + 1; }
    static int c15() { return c16() + 1; }
    static int c16() { return c17() + 1; }
    static int c17() { return c18() + 1; }
    static int c18() { return c19() + 1; }
    static int c19() { return 0; }

    // Eventail : une racine, huit appelees, chacune vers huit feuilles.
    public static int fan() { return f0() + f1() + f2() + f3() + f4() + f5() + f6() + f7(); }
    static int f0() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int f1() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int f2() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int f3() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int f4() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int f5() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int f6() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int f7() { return l0() + l1() + l2() + l3() + l4() + l5() + l6() + l7(); }
    static int l0() { return 0; }
    static int l1() { return 1; }
    static int l2() { return 2; }
    static int l3() { return 3; }
    static int l4() { return 4; }
    static int l5() { return 5; }
    static int l6() { return 6; }
    static int l7() { return 7; }

    /** Appel vers une classe absente de l'index : arete non resolue (FM-15). */
    public static String outside() {
        return System.getProperty("user.home");
    }
}
