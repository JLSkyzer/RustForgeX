package net.minecraft.world.level.block.entity;

/** Bouchon écrit pour ce projet (T-204, ADR-071). */
public class BlockEntity {
    public int progress;
    public boolean changed;

    public void setChanged() {
        changed = true;
    }
}
