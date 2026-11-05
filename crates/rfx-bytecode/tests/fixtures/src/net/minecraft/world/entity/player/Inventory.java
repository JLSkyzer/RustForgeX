package net.minecraft.world.entity.player;

/** Bouchon écrit pour ce projet (T-204, ADR-071). */
public class Inventory {
    public int selected;
    public int[] counts = new int[36];

    public int count(int slot) {
        return counts[slot];
    }
}
