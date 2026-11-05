package corpus;

import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.player.Inventory;

/**
 * Corpus T-204, famille 4 : tableaux et alias. Le domaine d'un élément est celui de
 * l'objet ou du statique d'où vient le tableau.
 */
public final class Arrays {

    static int[] table = new int[32];
    static int[][] grid = new int[4][4];
    static Entity[] entities = new Entity[8];
    static int[] published;

    private Arrays() {
    }

    @W.ModStatics
    static void writeTable(int i, int v) {
        table[i & 31] = v;
    }

    @W.ModStatics
    static void aliasTable() {
        int[] alias = table;
        alias[0] = 1;
    }

    @W.ModStatics
    static void writeGrid() {
        grid[1][2] = 3;
    }

    @W.ModStatics
    static void fillTable(int v) {
        for (int i = 0; i < table.length; i++) {
            table[i] = v;
        }
    }

    @W.ModStatics
    static void storeEntity(Entity e) {
        entities[0] = e;
    }

    @W.EntityState
    static void moveStored() {
        entities[1].x = 2.0;
    }

    @W.Unknown
    static void writeParam(int[] values) {
        values[0] = 42;
    }

    @W.Unknown
    static void swap(int[] values, int i, int j) {
        int t = values[i];
        values[i] = values[j];
        values[j] = t;
    }

    @W.Unknown
    static void aliasParam(int[] values) {
        int[] other = values;
        other[1] = 7;
    }

    @W.Unknown
    static void writeEither(int[] a, int[] b, boolean first) {
        int[] target = first ? a : b;
        target[0] = 0;
    }

    @W.EntityInventory
    static void inventoryAlias(Inventory inv) {
        int[] c = inv.counts;
        int[] d = c;
        d[3] = 64;
    }

    @W.EntityState
    static void entityParamArray(Entity[] list) {
        list[0].removed = true;
    }

    @W.Nothing
    static int readTable() {
        return table[0] + grid[0][0];
    }

    @W.Nothing
    static int localCopy() {
        int[] copy = new int[table.length];
        for (int i = 0; i < copy.length; i++) {
            copy[i] = table[i];
        }
        return copy[0];
    }

    @W.Nothing
    static int localAlias() {
        int[] a = new int[4];
        int[] b = a;
        b[2] = 5;
        return a[2];
    }

    @W.ModStatics
    static void publishLocal() {
        int[] fresh = new int[4];
        fresh[0] = 1;
        published = fresh;
    }

    @W.ModStatics
    static void writePublished() {
        published[0] = 2;
    }

    @W.Nothing
    static long sumParam(long[] values) {
        long s = 0;
        for (long v : values) {
            s += v;
        }
        return s;
    }

    @W.Unknown
    static void writeObjectArray(Object[] values, Object v) {
        values[0] = v;
    }

    @W.Nothing
    static int localObjects() {
        Object[] objects = new Object[2];
        objects[0] = objects;
        return objects.length;
    }
}
