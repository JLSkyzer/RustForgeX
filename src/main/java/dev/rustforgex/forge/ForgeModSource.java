package dev.rustforgex.forge;

import net.minecraftforge.fml.ModList;
import net.minecraftforge.forgespi.language.IModFileInfo;
import net.minecraftforge.forgespi.language.IModInfo;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;

/**
 * Lecture de l'inventaire des mods auprès de Forge (C-41).
 *
 * <p>Cahier des charges : PARTIE 5.39. Exigence : R-620. Maturité : {@code STABLE}.
 *
 * <p>Toutes les références à l'API de mods de Forge sont rassemblées ici, et nulle part
 * ailleurs : {@link ModDiscovery} se teste donc sans instance de jeu.
 *
 * <p>Rien de ce qui est lu ici ne charge une classe (R-620). {@code getMods()},
 * {@code getPackages()} et le chemin du fichier sont des métadonnées que le chargeur a
 * déjà en mémoire depuis son propre balayage. Notamment, la classe principale du mod
 * n'est <strong>pas</strong> relevée : l'obtenir supposerait de toucher au conteneur
 * construit, et la PARTIE 5.39 ne s'en sert pour rien.
 */
public final class ForgeModSource implements ModSource {

    /**
     * Nom du module portant un fichier de mods, ou {@code null}.
     *
     * <p>Sous ModLauncher, tous les mods partagent un chargeur de classes mais vivent
     * dans des modules distincts. Le module est donc l'équivalent moderne de
     * l'attribution « par chargeur » que demande la PARTIE 5.39, et il est plus sûr que
     * le paquet : deux mods peuvent revendiquer le même préfixe de paquet, jamais le
     * même module.
     */
    private static String moduleNameOf(IModFileInfo fileInfo) {
        try {
            return fileInfo.moduleName();
        } catch (RuntimeException | LinkageError e) {
            // Un fichier de mods peut ne pas être porté par un module nommé — un mod
            // en développement, par exemple. L'attribution retombe alors sur le paquet.
            return null;
        }
    }

    /** Chemin du fichier de mods, ou {@code null} si le chargeur n'en expose pas. */
    private static Path fileOf(IModFileInfo fileInfo) {
        try {
            return fileInfo.getFile().getFilePath();
        } catch (RuntimeException | LinkageError e) {
            return null;
        }
    }

    @Override
    public List<RawMod> mods() {
        ModList list = ModList.get();
        if (list == null) {
            // Appelé avant que Forge n'ait dressé sa liste. L'inventaire sera vide, et
            // toute classe sera attribuée à `unknown` — le comportement conservateur.
            return List.of();
        }

        List<RawMod> found = new ArrayList<>();
        for (IModFileInfo fileInfo : list.getModFiles()) {
            List<IModInfo> declared = fileInfo.getMods();
            if (declared.isEmpty()) {
                // Un fichier sans mod déclaré est une bibliothèque embarquée : elle n'a
                // pas de propriétaire à elle, ses classes retomberont sur `unknown`.
                continue;
            }
            String moduleName = moduleNameOf(fileInfo);
            Path file = fileOf(fileInfo);
            Set<String> packages = packagesOf(fileInfo);

            // Un fichier peut déclarer plusieurs mods. Chacun entre dans l'inventaire
            // avec sa propre version ; les paquets, eux, appartiennent au fichier et
            // reviennent donc au premier déclarant, faute d'un moyen de les départager.
            boolean first = true;
            for (IModInfo mod : declared) {
                found.add(new RawMod(
                        mod.getModId(),
                        mod.getVersion() == null ? null : mod.getVersion().toString(),
                        file,
                        first ? moduleName : null,
                        first ? packages : Set.of()));
                first = false;
            }
        }
        return found;
    }

    /** Paquets déclarés par le fichier, tels que le chargeur les a indexés. */
    private static Set<String> packagesOf(IModFileInfo fileInfo) {
        try {
            return fileInfo.getFile().getSecureJar().getPackages();
        } catch (RuntimeException | LinkageError e) {
            return Set.of();
        }
    }
}
