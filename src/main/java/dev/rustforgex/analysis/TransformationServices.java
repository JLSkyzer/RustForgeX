package dev.rustforgex.analysis;

import java.lang.module.ModuleDescriptor;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;

/**
 * Services de ModLauncher qui transforment des classes, et ceux de la plateforme dont
 * l'effet est connu de l'analyse.
 *
 * <p>Composant : C-07 (ADR-059). Maturité : {@code STABLE}.
 *
 * <p>Un service de transformation ({@code ITransformationService}) ou un greffon de
 * lancement ({@code ILaunchPluginService}) peut réécrire n'importe quelle classe au
 * chargement : le bytecode lu dans l'archive ne dit alors plus ce qui s'exécute. Ceux
 * de Forge, de Mixin et de RUSTFORGE-X sont connus, et leur effet est pris en compte
 * (cibles déclarées absentes, ou parade de l'ADR-059) ; tout autre fournisseur rend
 * les transformations inconnues, et l'analyse ne démarre pas. Les noms ci-dessous sont
 * ceux de la plateforme, jamais d'un mod (INV-12), relevés dans les archives de Forge
 * 47.4.23, de Mixin 0.8.5, d'eventbus 6.2.33 et d'accesstransformers 8.0.4 : leurs
 * {@code module-info} et leurs fichiers {@code META-INF/services}, qui diffèrent chez
 * Mixin (variantes {@code Legacy} pour un chargement hors module).
 */
final class TransformationServices {

    /** Service de transformation de ModLauncher. */
    static final String TRANSFORMATION_SERVICE = "cpw.mods.modlauncher.api.ITransformationService";

    /** Greffon de lancement de ModLauncher. */
    static final String LAUNCH_PLUGIN = "cpw.mods.modlauncher.serviceapi.ILaunchPluginService";

    /** Fournisseurs de la plateforme, dont l'effet est connu. */
    static final Set<String> KNOWN = Set.of(
            "net.minecraftforge.fml.loading.FMLServiceProvider",
            "org.spongepowered.asm.launch.MixinTransformationService",
            "org.spongepowered.asm.launch.MixinTransformationServiceLegacy",
            "dev.rustforgex.launch.RfxTransformationService",
            "net.minecraftforge.accesstransformer.service.AccessTransformerService",
            "net.minecraftforge.eventbus.service.ModLauncherService",
            "net.minecraftforge.fml.loading.log4j.SLF4JFixerLaunchPluginService",
            "org.spongepowered.asm.launch.MixinLaunchPlugin",
            "org.spongepowered.asm.launch.MixinLaunchPluginLegacy",
            "net.minecraftforge.fml.common.asm.CapabilityTokenSubclass",
            "net.minecraftforge.fml.common.asm.ObjectHolderDefinalize",
            "net.minecraftforge.fml.common.asm.RuntimeEnumExtender",
            "net.minecraftforge.fml.loading.RuntimeDistCleaner");

    private TransformationServices() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * @param module descripteur d'un module ; ceux des modules automatiques reprennent
     *     les fichiers {@code META-INF/services}
     * @return les fournisseurs de transformation de ce module dont l'effet est inconnu
     */
    static List<String> unknownProviders(ModuleDescriptor module) {
        List<String> unknown = new ArrayList<>();
        for (ModuleDescriptor.Provides provides : module.provides()) {
            String service = provides.service();
            if (!service.equals(TRANSFORMATION_SERVICE) && !service.equals(LAUNCH_PLUGIN)) {
                continue;
            }
            for (String provider : provides.providers()) {
                if (!KNOWN.contains(provider)) {
                    unknown.add(provider);
                }
            }
        }
        return unknown;
    }
}
