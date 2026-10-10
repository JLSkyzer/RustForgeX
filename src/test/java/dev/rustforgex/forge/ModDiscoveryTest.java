package dev.rustforgex.forge;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Tests de C-41, Mod Discovery (T-440 à T-442).
 *
 * <p>Cahier des charges : PARTIE 5.39. Exigences : R-620, R-621.
 *
 * <p>Aucune instance de Forge ici : {@link ModDiscovery} lit une {@link ModSource}, et
 * c'est précisément pour cela que cette interface existe. Un composant qu'on ne peut
 * vérifier que dans une partie lancée est un composant qu'on ne vérifie pas.
 */
class ModDiscoveryTest {

    /** Source de mods décrite à la main, sans Forge. */
    private static ModSource source(ModSource.RawMod... mods) {
        List<ModSource.RawMod> list = List.of(mods);
        return () -> list;
    }

    private static ModSource.RawMod mod(String id, String moduleName, String... packages) {
        return new ModSource.RawMod(id, "1.0", null, moduleName, Set.of(packages));
    }

    /**
     * T-440 : l'attribution suit l'ordre de la PARTIE 5.39 — module, puis paquet, puis
     * {@code unknown}.
     */
    @Test
    @DisplayName("T-440 : le module tranche avant le paquet")
    void theModuleWinsOverThePackage() {
        // Deux mods revendiquent le même paquet : c'est possible, et c'est exactement
        // le cas où le paquet seul se trompe.
        ModDiscovery discovery = ModDiscovery.from(source(
                mod("premier", "module.premier", "a.b"),
                mod("second", "module.second", "a.b")));

        assertEquals("second",
                discovery.ownerOf("module.second", "a/b/Classe"),
                "le module connu doit primer sur le paquet partagé");
        assertEquals("premier",
                discovery.ownerOf("module.premier", "a/b/Classe"));
    }

    @Test
    @DisplayName("T-440 : sans module connu, le paquet attribue")
    void withoutAKnownModuleThePackageAttributes() {
        ModDiscovery discovery = ModDiscovery.from(source(mod("unmod", "module.unmod", "x.y")));

        assertEquals("unmod", discovery.ownerOf(null, "x/y/Classe"));
        assertEquals("unmod", discovery.ownerOf("module.inconnu", "x/y/Classe"),
                "un module inconnu ne doit pas empêcher l'attribution par paquet");
    }

    @Test
    @DisplayName("T-440 : un sous-paquet non déclaré revient au plus long préfixe connu")
    void anUndeclaredSubPackageFallsBackToTheLongestKnownPrefix() {
        ModDiscovery discovery = ModDiscovery.from(source(mod("unmod", null, "x.y")));

        assertEquals("unmod", discovery.ownerOf(null, "x/y/z/w/Classe"));
    }

    /**
     * UNKNOWN = CONSERVATIVE : une classe qu'on ne sait pas rattacher n'est jamais
     * attribuée au mod le plus probable. Un rapport trompeur est pire qu'un rapport
     * incomplet.
     */
    @Test
    @DisplayName("T-440 : ce qu'on ne sait pas rattacher est « unknown », jamais deviné")
    void whatCannotBeAttributedIsUnknownNeverGuessed() {
        ModDiscovery discovery = ModDiscovery.from(source(mod("unmod", "module.unmod", "x.y")));

        assertEquals(ModDiscovery.UNKNOWN, discovery.ownerOf(null, "autre/paquet/Classe"));
        assertEquals(ModDiscovery.UNKNOWN, discovery.ownerOf(null, "SansPaquet"));
        assertEquals(ModDiscovery.UNKNOWN, discovery.ownerOf(null, null));
        assertEquals(ModDiscovery.UNKNOWN, ModDiscovery.empty().ownerOf(null, "x/y/Classe"));
    }

    /**
     * Une installation abîmée peut présenter deux fois le même identifiant. Garder les
     * deux rendrait l'attribution dépendante de l'ordre d'énumération, donc instable
     * d'un démarrage à l'autre.
     */
    @Test
    @DisplayName("Un identifiant en double ne crée qu'une entrée")
    void aDuplicateIdentifierCreatesASingleEntry() {
        ModDiscovery discovery = ModDiscovery.from(source(
                mod("unmod", "module.a", "p.a"),
                mod("unmod", "module.b", "p.b")));

        assertEquals(1, discovery.modCount());
        assertEquals("unmod", discovery.ownerOf(null, "p/a/Classe"));
        assertEquals(ModDiscovery.UNKNOWN, discovery.ownerOf(null, "p/b/Classe"),
                "le second exemplaire est ignoré en entier, paquets compris");
    }

    @Test
    @DisplayName("Un mod sans identifiant est refusé plutôt qu'inventorié à tort")
    void aModWithoutAnIdentifierIsRejected() {
        assertThrows(IllegalArgumentException.class,
                () -> new ModSource.RawMod("", "1.0", null, null, Set.of()));
        assertThrows(IllegalArgumentException.class,
                () -> new ModSource.RawMod(null, "1.0", null, null, Set.of()));
    }

    @Test
    @DisplayName("Une version absente est dite inconnue, jamais inventée")
    void anAbsentVersionIsCalledUnknown() {
        ModDiscovery discovery = ModDiscovery.from(source(
                new ModSource.RawMod("unmod", null, null, null, Set.of())));

        assertEquals("inconnue", discovery.mod("unmod").version());
    }

    /**
     * T-441 : la découverte ne doit charger aucune classe qui ne le serait pas
     * autrement (R-620). On ne peut pas observer le chargeur de classes depuis un test
     * unitaire ; on vérifie donc ce qui est observable et qui en découle : la
     * découverte ne touche aucun fichier.
     */
    @Test
    @DisplayName("T-441 : la découverte ne calcule aucune empreinte, donc ne lit aucun JAR")
    void discoveryReadsNoArchive(@TempDir Path root) throws IOException {
        Path jar = root.resolve("un-mod.jar");
        Files.writeString(jar, "contenu", StandardCharsets.UTF_8);

        ModDiscovery discovery = ModDiscovery.from(source(
                new ModSource.RawMod("unmod", "1.0", jar, "module.unmod", Set.of("x.y"))));

        assertFalse(discovery.mod("unmod").hashComputed(),
                "R-620 : hacher les JAR à la découverte lirait plus d'un gibioctet");
    }

    /**
     * T-441 : la découverte ne charge aucune classe (R-620), observé de deux façons.
     *
     * <ul>
     *   <li>un mod synthétique déclare le paquet {@code rfxtest.discovery}, dont la
     *       sentinelle lève un fil-piège si elle est initialisée ;
     *   <li>le compteur de classes chargées de la JVM ne bouge pas pendant une
     *       découverte, une fois les classes de la découverte elle-même chargées par un
     *       premier appel — ce qui prend aussi un chargement sans initialisation.
     * </ul>
     */
    @Test
    @DisplayName("T-441 : la découverte ne charge aucune classe, même des paquets qu'elle inventorie")
    void discoveryLoadsNoClass() {
        ModDiscovery.from(source(
                new ModSource.RawMod("chauffe", "1.0", null, "module.chauffe", Set.of("a.b"))));
        java.lang.management.ClassLoadingMXBean classes =
                java.lang.management.ManagementFactory.getClassLoadingMXBean();
        long before = classes.getTotalLoadedClassCount();

        ModDiscovery discovery = ModDiscovery.from(source(new ModSource.RawMod(
                "sentinelle", "1.0", null, "module.sentinelle", Set.of("rfxtest.discovery"))));

        long loaded = classes.getTotalLoadedClassCount() - before;
        assertEquals(1, discovery.modCount());
        assertEquals(0, loaded, "R-620 : la découverte a chargé " + loaded + " classe(s)");
        assertFalse(rfxtest.discovery.Tripwire.tripped(),
                "R-620 : une classe d'un paquet inventorié a été initialisée");
    }

    /**
     * T-442 : la découverte tient sous 500 ms pour 250 mods (R-621).
     *
     * <p>La marge réelle est énorme parce que la découverte ne fait que réorganiser des
     * métadonnées déjà en mémoire. C'est le calcul d'empreinte qui coûterait — mesuré à
     * 1 248 ms pour 273 JAR — et il est justement sorti de ce chemin (ADR-023).
     */
    @Test
    @DisplayName("T-442 : 250 mods sont inventoriés en moins de 500 ms")
    void twoHundredFiftyModsAreInventoriedUnderFiveHundredMilliseconds() {
        List<ModSource.RawMod> many = new ArrayList<>(250);
        for (int i = 0; i < 250; i++) {
            // Une vingtaine de paquets par mod, ordre de grandeur d'un mod réel.
            List<String> packages = new ArrayList<>(20);
            for (int p = 0; p < 20; p++) {
                packages.add("mod" + i + ".sous" + p);
            }
            many.add(new ModSource.RawMod("mod" + i, "1.0", null, "module.mod" + i,
                    Set.copyOf(packages)));
        }
        ModSource source = () -> many;

        ModDiscovery discovery = ModDiscovery.from(source);

        assertEquals(250, discovery.modCount());
        assertEquals(250, discovery.knownModules());
        assertEquals(5_000, discovery.knownPackages());
        assertTrue(discovery.durationMs() < 500,
                "R-621 : " + discovery.durationMs() + " ms pour 250 mods");
    }

    /**
     * L'empreinte identifie une version de mod dans un rapport. Deux contenus
     * différents doivent donc donner deux empreintes différentes, et un même contenu la
     * même — sans quoi elle n'identifie rien.
     */
    @Test
    @DisplayName("L'empreinte dépend du contenu, et est retenue après le premier calcul")
    void theHashDependsOnTheContentAndIsRemembered(@TempDir Path root) throws IOException {
        Path premier = Files.writeString(root.resolve("a.jar"), "un", StandardCharsets.UTF_8);
        Path second = Files.writeString(root.resolve("b.jar"), "deux", StandardCharsets.UTF_8);
        Path copie = Files.writeString(root.resolve("c.jar"), "un", StandardCharsets.UTF_8);

        ModDiscovery discovery = ModDiscovery.from(source(
                new ModSource.RawMod("a", "1", premier, null, Set.of()),
                new ModSource.RawMod("b", "1", second, null, Set.of()),
                new ModSource.RawMod("c", "1", copie, null, Set.of())));

        String hashA = discovery.mod("a").ownerModHash();

        assertNotEquals(hashA, discovery.mod("b").ownerModHash());
        assertEquals(hashA, discovery.mod("c").ownerModHash(),
                "un même contenu doit donner une même empreinte");
        assertTrue(discovery.mod("a").hashComputed());
        assertEquals(hashA, discovery.mod("a").ownerModHash(), "l'empreinte doit être retenue");
        assertEquals(2, discovery.mod("a").sizeBytes());
    }

    /**
     * Un mod en développement est un répertoire, pas une archive. Hacher son chemin
     * produirait une empreinte qui ne dépend pas du contenu : autant dire qu'il n'y en
     * a pas.
     */
    @Test
    @DisplayName("Un mod sans archive n'a pas d'empreinte, et le dit")
    void aModWithoutAnArchiveHasNoHashAndSaysSo(@TempDir Path root) {
        ModDiscovery discovery = ModDiscovery.from(source(
                new ModSource.RawMod("repertoire", "1", root, null, Set.of()),
                new ModSource.RawMod("sansfichier", "1", null, null, Set.of())));

        assertEquals(ModEntry.NO_HASH, discovery.mod("repertoire").ownerModHash());
        assertEquals(ModEntry.NO_HASH, discovery.mod("sansfichier").ownerModHash());
        assertEquals(0, discovery.mod("sansfichier").sizeBytes());
    }

    @Test
    @DisplayName("Un mod absent de l'inventaire se dit absent, sans échouer")
    void anAbsentModIsReportedAsAbsent() {
        assertNull(ModDiscovery.from(source(mod("unmod", null))).mod("autre"));
    }
}
