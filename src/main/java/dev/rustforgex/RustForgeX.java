package dev.rustforgex;

import com.mojang.logging.LogUtils;
import net.minecraftforge.fml.common.Mod;
import org.slf4j.Logger;

/**
 * Point d'entrée Forge de RUSTFORGE-X.
 *
 * <p>Composant : C-01 (Forge Integration) — squelette. Jalon : M0.
 *
 * <p>À ce stade, le mod se charge et n'altère strictement rien du comportement du jeu,
 * conformément au livrable M0 du cahier des charges (PARTIE 30) : « le jeu tourne
 * exactement comme sans le mod ».
 *
 * <p>Le contenu réel de C-01 (abstraction du loader, hooks de tick, bus d'événements)
 * et de C-02 (Bootstrap) sera implémenté dans les sous-paquets {@code forge} et
 * {@code bootstrap}.
 */
@Mod(RustForgeX.MODID)
public class RustForgeX {

    /** Identifiant du mod. DOIT correspondre à {@code mod_id} dans gradle.properties. */
    public static final String MODID = "rustforgex";

    private static final Logger LOGGER = LogUtils.getLogger();

    public RustForgeX() {
        LOGGER.info("RUSTFORGE-X chargé (squelette M0, aucune transformation active)");
    }
}
