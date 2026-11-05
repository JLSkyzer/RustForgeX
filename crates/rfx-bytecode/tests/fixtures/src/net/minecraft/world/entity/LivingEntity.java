package net.minecraft.world.entity;

/** Bouchon écrit pour ce projet (T-204, ADR-071). */
public class LivingEntity extends Entity {
    public float health;

    public void heal(float amount) {
        health += amount;
    }

    public void hurt(float amount) {
        health -= amount;
        if (health <= 0) {
            removed = true;
        }
    }

    public float getHealth() {
        return health;
    }
}
