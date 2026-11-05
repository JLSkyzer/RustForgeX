package net.minecraft.world.level;

/** Bouchon écrit pour ce projet (T-204, ADR-071). */
public class Level {
    public long dayTime;
    public boolean raining;

    public long getDayTime() {
        return dayTime;
    }
}
