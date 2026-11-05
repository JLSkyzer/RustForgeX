package corpus;

/**
 * Corpus T-204, famille 2 : état propre au mod — statiques, objets du mod reçus en
 * paramètre ou tenus dans un statique, receveur partagé.
 */
public final class ModState {

    static int counter;
    static long total;
    static boolean dirty;
    static Config config = new Config();
    static Config spare;
    static int[] histogram = new int[16];
    static Config[] slots = new Config[4];

    int value;
    Config own;

    private ModState() {
    }

    @W.ModStatics
    static void increment() {
        counter++;
    }

    @W.ModStatics
    static void addTotal(long amount) {
        total += amount;
    }

    @W.ModStatics
    static void markDirty() {
        dirty = true;
    }

    @W.ModStatics
    static void resetAll() {
        counter = 0;
        total = 0;
        dirty = false;
    }

    @W.ModStatics
    static void conditional(int v) {
        if (v > 10) {
            counter = v;
        }
    }

    @W.ModStatics
    static void inLoop(int n) {
        for (int i = 0; i < n; i++) {
            counter += i;
        }
    }

    @W.ModStatics
    static void bucket(int v) {
        histogram[v & 15]++;
    }

    @W.ModStatics
    static void publishNew() {
        spare = new Config();
    }

    @W.ModStatics
    static void publishBuilt() {
        Config c = new Config();
        c.level = 3;
        spare = c;
    }

    @W.ModStatics
    static void storeSlot(int i) {
        slots[i & 3] = new Config();
    }

    @W.Unknown
    static void configThroughStatic() {
        config.level = 5;
    }

    @W.Unknown
    static void configFlag() {
        config.enabled = !config.enabled;
    }

    @W.Unknown
    static void setOnParam(Config c) {
        c.level = 1;
    }

    @W.Unknown
    static void setOnParamCond(Config c, boolean b) {
        if (b) {
            c.enabled = true;
        }
    }

    @W.Unknown
    static void linkParams(Config a, Config b) {
        a.parent = b;
    }

    @W.Unknown
    static void deepWrite(Config c) {
        c.parent.level = 2;
    }

    @W.Unknown
    static void slotWrite(int i) {
        slots[i & 3].level = 4;
    }

    @W.Nothing
    static int readsOnly(Config c) {
        return c.level + (c.enabled ? 1 : 0);
    }

    @W.Nothing
    static int localConfig() {
        Config c = new Config();
        c.level = 8;
        c.enabled = true;
        return c.level;
    }

    @W.Unknown
    void setValue(int v) {
        this.value = v;
    }

    @W.Unknown
    void bump() {
        value++;
    }

    @W.Unknown
    void writeOwned() {
        own.level = 6;
    }

    @W.Unknown
    void replaceOwned() {
        own = new Config();
    }

    @W.Unknown
    @W.ModStatics
    void both() {
        value = counter;
        counter = 0;
    }

    @W.Nothing
    int readThis() {
        return value * 2;
    }

    /** Objet du mod, sans domaine de jeu. */
    static final class Config {
        int level;
        boolean enabled;
        Config parent;
    }
}
