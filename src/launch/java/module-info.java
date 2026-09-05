/**
 * Plugin de lancement de RUSTFORGE-X (ADR-017).
 *
 * <p>Ce descripteur n'est pas une formalité : dans la couche d'amorçage de
 * ModLauncher, un service n'est découvert que s'il est déclaré par {@code provides}.
 * Un fichier {@code META-INF/services} suffit sur le classpath, mais pas là — le
 * module serait automatique, et son {@code ILaunchPluginService} resterait invisible.
 * C'est ce qui distingue ce JAR de n'importe quelle bibliothèque : il doit être un
 * module nommé.
 *
 * <p>Le paquet est exporté pour que le mod, dans la couche du jeu, puisse armer le
 * transformateur. Le sens inverse n'existe pas : ce module ne voit rien du mod.
 */
module rustforgex.launch {

    requires cpw.mods.modlauncher;
    requires org.objectweb.asm;
    requires org.objectweb.asm.tree;
    requires org.slf4j;

    exports dev.rustforgex.launch;

    // Laissez-passer vers la couche de plugins : sans ce service, le JAR n'y est pas
    // promu, et le plugin ci-dessous n'est jamais cherché.
    provides cpw.mods.modlauncher.api.ITransformationService
            with dev.rustforgex.launch.RfxTransformationService;

    // Le transformateur lui-même : consulté pour chaque classe chargée.
    provides cpw.mods.modlauncher.serviceapi.ILaunchPluginService
            with dev.rustforgex.launch.RfxLaunchPlugin;
}
