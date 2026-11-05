package corpus;

import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.block.entity.BlockEntity;

/**
 * Corpus T-204, famille 6 : appels virtuels — classe abstraite à plusieurs
 * implémentations, méthode redéfinie, interface, méthode par défaut, appel `super`.
 */
public final class Poly {

    static int calls;

    private Poly() {
    }

    @W.EntityState
    @W.BlockEntityState
    static void runAction(Action a, Entity e, BlockEntity be) {
        a.apply(e, be);
    }

    @W.EntityState
    static void runMover(Mover m, Entity e) {
        m.move(e);
    }

    @W.EntityState
    static void runFastMover(FastMover m, Entity e) {
        m.move(e);
    }

    @W.Nothing
    static int shapeArea(Shape s) {
        return s.area();
    }

    @W.ModStatics
    static void countedShape(Shape s) {
        calls += s.area();
    }

    @W.EntityState
    static void viaInterface(Effect fx, Entity e) {
        fx.affect(e);
    }

    @W.EntityState
    static void viaDefault(Effect fx, Entity e) {
        fx.twice(e);
    }

    @W.ModStatics
    static void viaSuper(Logged l) {
        l.log();
    }

    @W.Nothing
    static int localPoly() {
        Shape s = new Rect(2, 3);
        return s.area();
    }

    @W.EntityState
    static void knownSubclass(Entity e) {
        Mover m = new Mover();
        m.move(e);
    }

    @W.EntityState
    static void chosenAtRuntime(Entity e, boolean fast) {
        Mover m = fast ? new FastMover() : new Mover();
        m.move(e);
    }

    @W.Unknown
    static void finalTarget(Sealed s) {
        s.touch();
    }

    @W.EntityState
    static void toStringFree(Entity e) {
        e.tick();
    }

    @W.Nothing
    static int abstractRead(Shape s) {
        return s instanceof Rect ? ((Rect) s).w : 0;
    }

    @W.EntityState
    @W.BlockEntityState
    static void allActions(Action[] actions, Entity e, BlockEntity be) {
        for (Action a : actions) {
            a.apply(e, be);
        }
    }

    /** Action abstraite à trois implémentations. */
    abstract static class Action {
        abstract void apply(Entity e, BlockEntity be);
    }

    static final class Push extends Action {
        @Override
        void apply(Entity e, BlockEntity be) {
            e.x += 1;
        }
    }

    static final class Charge extends Action {
        @Override
        void apply(Entity e, BlockEntity be) {
            be.progress += 2;
        }
    }

    static final class Idle extends Action {
        @Override
        void apply(Entity e, BlockEntity be) {
        }
    }

    /** Déplace une entité ; redéfini plus bas. */
    static class Mover {
        void move(Entity e) {
            e.y += 1;
        }
    }

    static final class FastMover extends Mover {
        @Override
        void move(Entity e) {
            e.y += 4;
        }
    }

    /** Forme : lecture seule. */
    abstract static class Shape {
        abstract int area();
    }

    static final class Rect extends Shape {
        final int w;
        final int h;

        Rect(int w, int h) {
            this.w = w;
            this.h = h;
        }

        @Override
        int area() {
            return w * h;
        }
    }

    static final class Square extends Shape {
        final int side;

        Square(int side) {
            this.side = side;
        }

        @Override
        int area() {
            return side * side;
        }
    }

    /** Interface du mod, une implémentation, une méthode par défaut. */
    interface Effect {
        void affect(Entity e);

        default void twice(Entity e) {
            affect(e);
            affect(e);
        }
    }

    static final class Burn implements Effect {
        @Override
        public void affect(Entity e) {
            e.age += 10;
        }
    }

    /** Appel `super` vers une méthode qui écrit un statique. */
    static class Base {
        void log() {
            calls++;
        }
    }

    static final class Logged extends Base {
        @Override
        void log() {
            super.log();
        }
    }

    /** Classe finale : une seule cible possible. */
    static final class Sealed {
        int touched;

        void touch() {
            touched++;
        }
    }
}
