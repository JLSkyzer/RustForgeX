/**
 * Transformateur de bytecode de RUSTFORGE-X (ADR-019).
 *
 * <p>Ce descripteur n'est pas une formalité : quand ce JAR se trouve sur le classpath
 * d'amorçage — ce qui est le cas en développement — un service n'y est découvert que
 * s'il est déclaré par {@code provides}. Un fichier {@code META-INF/services} suffit
 * ailleurs, mais pas là : le module serait automatique, et son service resterait
 * invisible.
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

    // Laissez-passer ET travail : ModLauncher promeut le JAR qui déclare ce service,
    // puis lui demande ses transformateurs. C'est la seule voie ouverte à un JAR déposé
    // dans mods (ADR-019).
    provides cpw.mods.modlauncher.api.ITransformationService
            with dev.rustforgex.launch.RfxTransformationService;

}
