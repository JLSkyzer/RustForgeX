package dev.rustforgex.instrument;

import net.minecraftforge.fml.ModList;
import net.minecraftforge.forgespi.language.IModFileInfo;
import net.minecraftforge.forgespi.language.IModInfo;

import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Attribution d'une classe au mod qui la fournit (DM-01, champ {@code owner_id}).
 *
 * <p>Composant : C-04. Cahier des charges : PARTIE 4.1. Invariant : INV-12.
 * Maturité : {@code STABLE}.
 *
 * <p>La correspondance est construite à partir de la liste des mods de Forge, jamais
 * d'une règle écrite à la main. C'est ce qui la garde compatible avec INV-12 : aucun
 * nom de mod n'apparaît dans ce code, et {@code minecraft} comme {@code forge} sont
 * découverts par le même chemin que n'importe quel autre mod. Le résultat sert
 * uniquement d'étiquette dans le {@code WorkId} — aucune décision du moteur n'en
 * dépend.
 *
 * <p>Une classe dont le paquet n'est rattaché à aucun mod connu est attribuée à
 * {@link #UNKNOWN}. C'est le principe UNKNOWN = CONSERVATIVE : mieux vaut une unité de
 * travail non attribuée qu'une unité attribuée au mauvais mod, qui rendrait tout
 * rapport trompeur.
 */
public final class ModOwnerResolver {

    /** Étiquette des classes qu'on ne sait rattacher à aucun mod. */
    public static final String UNKNOWN = "unknown";

    /**
     * Correspondance paquet vers identifiant de mod, ou {@code null} tant qu'elle n'a
     * pas été construite.
     *
     * <p>{@code volatile} : la construction a lieu sur le premier fil qui charge une
     * classe sondable, les lectures sur tous les autres.
     */
    private volatile Map<String, String> packageToMod;

    /** Construit un résolveur qui bâtira sa table au premier usage. */
    public ModOwnerResolver() {
        // La liste des mods n'existe pas encore à la construction : la table est
        // bâtie paresseusement, au premier appel utile.
    }

    /**
     * Détermine le mod propriétaire d'une classe.
     *
     * @param classInternalName nom interne, par exemple {@code net/minecraft/world/X}
     * @return l'identifiant du mod, ou {@link #UNKNOWN}
     */
    public String ownerOf(String classInternalName) {
        int lastSlash = classInternalName.lastIndexOf('/');
        if (lastSlash <= 0) {
            return UNKNOWN;
        }
        String packageName = classInternalName.substring(0, lastSlash).replace('/', '.');

        Map<String, String> table = table();
        String owner = table.get(packageName);
        if (owner != null) {
            return owner;
        }

        // Les mods déclarent leurs paquets feuille par feuille : une classe d'un
        // sous-paquet non listé se rattache au plus long préfixe connu.
        for (int cut = packageName.lastIndexOf('.'); cut > 0; cut = packageName.lastIndexOf('.', cut - 1)) {
            owner = table.get(packageName.substring(0, cut));
            if (owner != null) {
                return owner;
            }
        }
        return UNKNOWN;
    }

    /** @return le nombre de paquets connus, ou {@code 0} si la table n'est pas bâtie */
    public int knownPackages() {
        Map<String, String> table = packageToMod;
        return table == null ? 0 : table.size();
    }

    /** Table des paquets, construite au premier appel. */
    private Map<String, String> table() {
        Map<String, String> table = packageToMod;
        if (table != null) {
            return table;
        }
        // Deux fils peuvent la bâtir en même temps : ils obtiendront le même contenu,
        // et le second écrasera le premier sans conséquence. Un verrou coûterait plus
        // que cette redondance improbable.
        table = build();
        packageToMod = table;
        return table;
    }

    /** Construit la table à partir de la liste des mods de Forge. */
    private static Map<String, String> build() {
        Map<String, String> table = new HashMap<>();
        ModList mods = ModList.get();
        if (mods == null) {
            // Appelé avant que Forge n'ait dressé sa liste : tout sera `unknown`, et
            // la table sera rebâtie au prochain appel.
            return Map.of();
        }

        for (IModFileInfo fileInfo : mods.getModFiles()) {
            List<IModInfo> declared = fileInfo.getMods();
            if (declared.isEmpty()) {
                continue;
            }
            // Un fichier peut déclarer plusieurs mods ; le premier fait foi, faute
            // d'un moyen de départager les paquets entre eux.
            String modId = declared.get(0).getModId();
            for (String packageName : fileInfo.getFile().getSecureJar().getPackages()) {
                table.putIfAbsent(packageName, modId);
            }
        }
        return Map.copyOf(table);
    }
}
