package net.minecraft.world.entity;

import net.minecraft.world.level.Level;

/**
 * Bouchon écrit pour ce projet (T-204, ADR-071) : seul son nom compte, celui d'une
 * classe d'ancrage de la table des domaines. Aucun code de Minecraft n'est repris.
 */
public class Entity {
    public double x;
    public double y;
    public double z;
    public int age;
    public boolean removed;
    public Level level;
    public Entity target;

    public void setPos(double x, double y, double z) {
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public double getX() {
        return x;
    }

    public void tick() {
        age++;
    }

    public Level level() {
        return level;
    }
}
