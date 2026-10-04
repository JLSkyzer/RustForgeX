package dev.rustforgex.bench;

import net.minecraft.core.NonNullList;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.TransientCraftingContainer;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.crafting.CraftingRecipe;
import net.minecraft.world.item.crafting.Ingredient;
import net.minecraft.world.item.crafting.RecipeType;
import net.minecraftforge.common.crafting.IShapedRecipe;
import net.minecraftforge.registries.ForgeRegistries;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;

/**
 * C-36 : crafting du test G-12 — chaque recette d'atelier du modpack, posée sur sa propre
 * grille et jugée.
 *
 * <p>Cahier des charges : PARTIE 20.3.4. Maturité : {@code STABLE}.
 *
 * <p>Pour chaque recette, dans l'ordre de son identifiant : sa grille est remplie avec le
 * premier objet de chaque ingrédient, puis on relève si la recette se reconnaît
 * ({@code matches}), ce qu'elle produit ({@code assemble}) et ce qu'elle rend
 * ({@code getRemainingItems}). C'est le code de recettes de tous les mods, appelé tel
 * que le jeu l'appelle, sans joueur.
 *
 * <p>La recette est appelée directement, pas cherchée par le gestionnaire de recettes :
 * chercher la recette d'une grille parcourt toutes les recettes, et le faire pour chacune
 * coûterait un produit de leurs nombres. Le travail est étalé sur les ticks
 * ({@value #PER_TICK} recettes par tick) pour rester loin du chien de garde du serveur.
 *
 * <p>Le résultat d'une recette se compare d'une exécution à l'autre : objet par son
 * identifiant de registre, nombre, et données par leur empreinte — jamais par l'identité
 * d'un objet Java, qui change à chaque lancement.
 */
final class CraftingSweep {

    private static final Logger LOGGER = LoggerFactory.getLogger("rustforgex-bench");

    /** Recettes jugées par tick. */
    static final int PER_TICK = 200;

    private final List<CraftingRecipe> recipes = new ArrayList<>();
    private final List<String> ids = new ArrayList<>();
    private final List<String> hashes = new ArrayList<>();
    private int matched;
    private int errors;
    private int next;
    private boolean reported;

    /** Relève les recettes d'atelier chargées, triées par identifiant. */
    void start(ServerLevel level) {
        recipes.addAll(level.getRecipeManager().getAllRecipesFor(RecipeType.CRAFTING));
        recipes.sort(Comparator.comparing(recipe -> recipe.getId().toString()));
        LOGGER.info("G-12 : {} recettes d'atelier à juger.", recipes.size());
    }

    /**
     * Juge les recettes suivantes, au plus {@value #PER_TICK}.
     *
     * @return {@code true} quand toutes l'ont été
     */
    boolean step(ServerLevel level) {
        int end = Math.min(recipes.size(), next + PER_TICK);
        for (; next < end; next++) {
            CraftingRecipe recipe = recipes.get(next);
            ids.add(recipe.getId().toString());
            hashes.add(String.format(Locale.ROOT, "%016x", fnv(judge(recipe, level))));
        }
        boolean finished = next >= recipes.size();
        if (finished && !reported) {
            reported = true;
            LOGGER.info("G-12 : {} recettes jugées, {} reconnues par leur propre grille, "
                    + "{} en erreur.", recipes.size(), matched, errors);
        }
        return finished;
    }

    /** Description stable du verdict d'une recette sur sa propre grille. */
    private String judge(CraftingRecipe recipe, ServerLevel level) {
        try {
            TransientCraftingContainer grid = new TransientCraftingContainer(new NoMenu(), 3, 3);
            String layout = lay(recipe, grid);
            if (layout != null) {
                return layout;
            }
            boolean matches = recipe.matches(grid, level);
            if (matches) {
                matched++;
            }
            StringBuilder text = new StringBuilder(matches ? "reconnue|" : "refusée|");
            describe(text, recipe.assemble(grid, level.registryAccess()));
            text.append('|');
            NonNullList<ItemStack> remaining = recipe.getRemainingItems(grid);
            for (ItemStack stack : remaining) {
                describe(text, stack);
                text.append(';');
            }
            return text.toString();
        } catch (RuntimeException | LinkageError e) {
            // Une recette de mod qui lève ne doit pas arrêter le test : l'erreur est son
            // verdict, et elle se compare d'une exécution à l'autre comme un autre.
            errors++;
            return "erreur|" + e.getClass().getName();
        }
    }

    /**
     * Pose les ingrédients sur la grille : une recette à forme en haut à gauche, une
     * recette sans forme dans l'ordre de ses ingrédients.
     *
     * @return {@code null} si la grille est remplie, sinon le verdict à retenir
     */
    private static String lay(CraftingRecipe recipe, TransientCraftingContainer grid) {
        List<Ingredient> ingredients = recipe.getIngredients();
        int width = 3;
        if (recipe instanceof IShapedRecipe<?> shaped) {
            width = shaped.getRecipeWidth();
            if (width > 3 || shaped.getRecipeHeight() > 3) {
                return "trop-grande";
            }
        } else if (ingredients.size() > 9) {
            return "trop-grande";
        }
        for (int i = 0; i < ingredients.size(); i++) {
            ItemStack[] items = ingredients.get(i).getItems();
            if (items.length == 0 || width == 0) {
                continue;
            }
            grid.setItem((i / width) * 3 + i % width, items[0].copy());
        }
        return null;
    }

    private static void describe(StringBuilder text, ItemStack stack) {
        if (stack.isEmpty()) {
            text.append("vide");
            return;
        }
        text.append(ForgeRegistries.ITEMS.getKey(stack.getItem())).append('x')
                .append(stack.getCount()).append('#')
                .append(stack.getTag() == null ? 0 : stack.getTag().hashCode());
    }

    /** FNV-1a sur 64 bits des caractères : stable d'un lancement de la JVM à l'autre. */
    private static long fnv(String text) {
        long hash = 0xcbf29ce484222325L;
        for (int i = 0; i < text.length(); i++) {
            hash ^= text.charAt(i);
            hash *= 0x100000001b3L;
        }
        return hash;
    }

    int expected() {
        return recipes.size();
    }

    int judged() {
        return ids.size();
    }

    boolean finished() {
        return next >= recipes.size();
    }

    /** Table JSON : une ligne par recette, son empreinte comme unique composante. */
    String recipesJson() {
        StringBuilder table = new StringBuilder(ids.size() * 64);
        for (int i = 0; i < ids.size(); i++) {
            table.append(i == 0 ? "\n" : ",\n").append("    \"")
                    .append(ids.get(i).replace("\\", "\\\\").replace("\"", "\\\""))
                    .append("\": [\"").append(hashes.get(i)).append("\"]");
        }
        return table.toString();
    }

    /** Menu fictif : la grille d'atelier en exige un, aucun joueur ne l'ouvre. */
    private static final class NoMenu extends AbstractContainerMenu {

        NoMenu() {
            super(null, 0);
        }

        @Override
        public ItemStack quickMoveStack(Player player, int index) {
            return ItemStack.EMPTY;
        }

        @Override
        public boolean stillValid(Player player) {
            return false;
        }
    }
}
