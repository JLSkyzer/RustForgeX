package dev.rustforgex.command;

import dev.rustforgex.forge.ModDiscovery;
import dev.rustforgex.forge.ModEntry;
import net.minecraft.network.chat.Component;

import java.util.ArrayList;
import java.util.List;

/**
 * C-38 : composition du rapport affiché par {@code /rfx mods} (C-41).
 *
 * <p>Cahier des charges : PARTIE 5.36 et 5.39. Maturité : {@code STABLE}.
 *
 * <p>Séparé de l'enregistrement de la commande pour être testable sans démarrer
 * Minecraft, comme {@link StatusReport}. Aucun texte codé en dur, aucun appel natif,
 * aucune lecture de fichier : le rapport met en forme un inventaire déjà dressé.
 *
 * <p>En particulier, il n'appelle <strong>pas</strong> {@link ModEntry#ownerModHash()} :
 * ce calcul lit l'archive entière, ce qui n'a rien à faire sur le fil serveur
 * (INV-14). L'empreinte n'est affichée que si quelqu'un l'a déjà fait calculer.
 */
public final class ModsReport {

    /** Préfixe commun à toutes les clés de traduction du rapport. */
    public static final String KEY_PREFIX = "rustforgex.mods.";

    /**
     * Mods détaillés au plus dans la liste.
     *
     * <p>Un modpack en compte plusieurs centaines : les déverser dans le chat le
     * remplirait sans rien apprendre. Le rapport donne le compte exact et un extrait.
     */
    private static final int LISTED = 20;

    private ModsReport() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Compose les lignes du rapport.
     *
     * @param discovery inventaire dressé par C-41, ou {@code null} s'il n'a pas eu lieu
     * @return les lignes à afficher, jamais vides
     */
    public static List<Component> lines(ModDiscovery discovery) {
        if (discovery == null || discovery.modCount() == 0) {
            return List.of(Component.translatable(KEY_PREFIX + "none"));
        }

        List<Component> lines = new ArrayList<>();
        lines.add(Component.translatable(KEY_PREFIX + "summary",
                discovery.modCount(),
                discovery.knownModules(),
                discovery.knownPackages(),
                discovery.durationMs()));

        List<ModEntry> mods = discovery.mods();
        for (ModEntry mod : mods.subList(0, Math.min(LISTED, mods.size()))) {
            lines.add(Component.translatable(KEY_PREFIX + "entry",
                    Component.literal(mod.modId()),
                    Component.literal(mod.version()),
                    Component.literal(size(mod))));
        }
        if (mods.size() > LISTED) {
            lines.add(Component.translatable(KEY_PREFIX + "truncated", mods.size() - LISTED));
        }
        return List.copyOf(lines);
    }

    /**
     * Taille du fichier du mod, en kibioctets, ou une marque si elle est inconnue.
     *
     * <p>Une taille nulle n'est pas une taille : c'est un mod en développement, ou un
     * fichier illisible. L'écrire « 0 Kio » laisserait croire à une mesure (R-660).
     */
    private static String size(ModEntry mod) {
        long bytes = mod.sizeBytes();
        return bytes == 0L ? "—" : (bytes / 1024L) + " Kio";
    }
}
