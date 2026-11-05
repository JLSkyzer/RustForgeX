package corpus;

import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.entity.BlockEntity;

/**
 * Corpus T-204, famille 3 : état du jeu, par des classes dont le nom est une ancre de la
 * table des domaines (bouchons de ce dépôt).
 */
public final class Game {

    static Entity tracked;

    private Game() {
    }

    @W.EntityState
    static void move(Entity e) {
        e.x += 1.0;
    }

    @W.EntityState
    static void teleport(Entity e, double x, double y, double z) {
        e.setPos(x, y, z);
    }

    @W.EntityState
    static void age(Entity e) {
        e.tick();
    }

    @W.EntityState
    static void kill(Entity e) {
        e.removed = true;
    }

    @W.EntityState
    static void heal(LivingEntity e) {
        e.heal(2.0f);
    }

    @W.EntityState
    static void damage(LivingEntity e, float amount) {
        e.hurt(amount);
    }

    @W.EntityState
    static void setTarget(Entity e, Entity other) {
        e.target = other;
    }

    @W.EntityState
    static void moveTarget(Entity e) {
        e.target.y = 64.0;
    }

    @W.EntityState
    static void moveTracked() {
        tracked.z = 0.0;
    }

    @W.ModStatics
    static void track(Entity e) {
        tracked = e;
    }

    @W.EntityState
    static void moveBoth(Entity a, LivingEntity b) {
        a.x = 0.0;
        b.health = 20.0f;
    }

    @W.Nothing
    static double readPosition(Entity e) {
        return e.getX() + e.y + e.z;
    }

    @W.Nothing
    static float readHealth(LivingEntity e) {
        return e.getHealth();
    }

    @W.EntityInventory
    static void select(Inventory inv, int slot) {
        inv.selected = slot;
    }

    @W.EntityInventory
    static void addItems(Inventory inv, int slot, int amount) {
        inv.counts[slot] += amount;
    }

    @W.EntityInventory
    static void clearInventory(Inventory inv) {
        int[] counts = inv.counts;
        for (int i = 0; i < counts.length; i++) {
            counts[i] = 0;
        }
    }

    @W.Nothing
    static int readInventory(Inventory inv) {
        return inv.count(inv.selected);
    }

    @W.BlockEntityState
    static void progress(BlockEntity be) {
        be.progress++;
        be.setChanged();
    }

    @W.BlockEntityState
    static void markChanged(BlockEntity be) {
        be.setChanged();
    }

    @W.Nothing
    static boolean isChanged(BlockEntity be) {
        return be.changed;
    }

    @W.LevelGlobals
    static void advanceTime(Level level) {
        level.dayTime += 20;
    }

    @W.LevelGlobals
    static void stopRain(Entity e) {
        e.level.raining = false;
    }

    @W.LevelGlobals
    static void advanceThroughGetter(Entity e) {
        e.level().dayTime++;
    }

    @W.Nothing
    static long readTime(Level level) {
        return level.getDayTime();
    }

    @W.EntityState
    @W.BlockEntityState
    static void linked(Entity e, BlockEntity be) {
        be.progress = (int) e.x;
        e.age = be.progress;
    }
}
