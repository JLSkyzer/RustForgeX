package dev.rustforgex.forge;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * C-41 : inventaire des mods et attribution des classes à leur propriétaire.
 *
 * <p>Cahier des charges : PARTIE 5.39. Exigences : R-620, R-621. Tests : T-440 à T-442.
 * Métriques : {@code rfx.discovery.mods}, {@code rfx.discovery.duration_ms},
 * {@code rfx.discovery.unknown_owner_ratio}. Maturité : {@code STABLE}.
 *
 * <h2>Ce que la découverte ne fait pas</h2>
 *
 * <p>R-620 est la contrainte qui commande tout : la découverte <strong>ne doit charger
 * aucune classe qui ne le serait pas autrement</strong>. Elle ne parcourt donc pas les
 * archives, n'ouvre aucun {@code .class}, ne résout aucun type. Elle lit ce que le
 * chargeur de mods a déjà en mémoire — identifiants, versions, chemins, paquets
 * déclarés — et rien d'autre.
 *
 * <p>L'indexation des classes est paresseuse par construction : une classe entre dans
 * l'inventaire quand elle est effectivement chargée et sondée, pas avant.
 *
 * <h2>Ordre d'attribution</h2>
 *
 * <p>La PARTIE 5.39 demande « par chargeur, puis par paquet, puis {@code unknown} ».
 * Sous ModLauncher, les mods partagent un unique chargeur de classes mais vivent dans
 * des <strong>modules</strong> distincts : le module joue ici le rôle que le chargeur
 * jouait avant Java 9. L'ordre appliqué est donc module, puis paquet, puis
 * {@code unknown}.
 *
 * <p>Le module est plus sûr que le paquet : deux mods peuvent déclarer le même préfixe
 * de paquet, jamais le même module. Quand il est connu, il tranche.
 *
 * <h2>UNKNOWN = CONSERVATIVE</h2>
 *
 * <p>Une classe qu'on ne sait rattacher est attribuée à {@link #UNKNOWN}, jamais au mod
 * le plus probable. Une attribution fausse rend tout rapport trompeur, et un rapport
 * trompeur est pire qu'un rapport incomplet.
 *
 * <p>Aucun nom de mod n'apparaît dans ce code (INV-12) : tout vient du chargeur.
 */
public final class ModDiscovery {

    /** Étiquette des classes qu'on ne sait rattacher à aucun mod. */
    public static final String UNKNOWN = "unknown";

    private final List<ModEntry> mods;
    private final Map<String, ModEntry> byModId;
    private final Map<String, String> moduleToMod;
    private final Map<String, String> packageToMod;
    private final long durationNanos;

    private ModDiscovery(List<ModEntry> mods, Map<String, ModEntry> byModId,
            Map<String, String> moduleToMod, Map<String, String> packageToMod,
            long durationNanos) {
        this.mods = List.copyOf(mods);
        this.byModId = Map.copyOf(byModId);
        this.moduleToMod = Map.copyOf(moduleToMod);
        this.packageToMod = Map.copyOf(packageToMod);
        this.durationNanos = durationNanos;
    }

    /**
     * Dresse l'inventaire à partir d'une source de mods.
     *
     * @param source d'où viennent les mods, injectable pour les tests
     * @return l'inventaire, jamais {@code null}, éventuellement vide
     */
    public static ModDiscovery from(ModSource source) {
        long start = System.nanoTime();

        List<ModSource.RawMod> raw = source.mods();
        List<ModEntry> entries = new ArrayList<>(raw.size());
        Map<String, ModEntry> byModId = new HashMap<>(raw.size() * 2);
        Map<String, String> modules = new HashMap<>(raw.size() * 2);
        Map<String, String> packages = new HashMap<>();

        for (ModSource.RawMod mod : raw) {
            ModEntry entry = new ModEntry(mod);
            // Un identifiant en double vient d'une installation abîmée. Garder le
            // premier et ignorer le second est le choix conservateur : deux entrées
            // pour un même identifiant rendraient l'attribution non déterministe.
            if (byModId.putIfAbsent(entry.modId(), entry) != null) {
                continue;
            }
            entries.add(entry);
            if (entry.moduleName() != null && !entry.moduleName().isBlank()) {
                modules.putIfAbsent(entry.moduleName(), entry.modId());
            }
            for (String packageName : entry.packages()) {
                // `putIfAbsent` : un paquet revendiqué par deux mods reste au premier,
                // et l'attribution ne dépend pas de l'ordre d'énumération suivant.
                packages.putIfAbsent(packageName, entry.modId());
            }
        }

        return new ModDiscovery(entries, byModId, modules, packages,
                System.nanoTime() - start);
    }

    /** @return un inventaire vide, pour quand le chargeur n'a rien à dire */
    public static ModDiscovery empty() {
        return new ModDiscovery(List.of(), Map.of(), Map.of(), Map.of(), 0L);
    }

    /**
     * Attribue une classe à son mod, par son module puis par son paquet.
     *
     * @param moduleName nom du module de la classe, ou {@code null} s'il est inconnu
     * @param classInternalName nom interne, par exemple {@code a/b/C}
     * @return l'identifiant du mod, ou {@link #UNKNOWN}
     */
    public String ownerOf(String moduleName, String classInternalName) {
        if (moduleName != null) {
            String byModule = moduleToMod.get(moduleName);
            if (byModule != null) {
                return byModule;
            }
        }
        return ownerOfPackage(classInternalName);
    }

    /**
     * Attribue une classe à son mod par son seul paquet.
     *
     * @param classInternalName nom interne, par exemple {@code a/b/C}
     * @return l'identifiant du mod, ou {@link #UNKNOWN}
     */
    public String ownerOfPackage(String classInternalName) {
        if (classInternalName == null) {
            return UNKNOWN;
        }
        int lastSlash = classInternalName.lastIndexOf('/');
        if (lastSlash <= 0) {
            return UNKNOWN;
        }
        String packageName = classInternalName.substring(0, lastSlash).replace('/', '.');

        String owner = packageToMod.get(packageName);
        if (owner != null) {
            return owner;
        }
        // Les mods déclarent leurs paquets feuille par feuille : une classe d'un
        // sous-paquet non listé se rattache au plus long préfixe connu.
        for (int cut = packageName.lastIndexOf('.'); cut > 0;
                cut = packageName.lastIndexOf('.', cut - 1)) {
            owner = packageToMod.get(packageName.substring(0, cut));
            if (owner != null) {
                return owner;
            }
        }
        return UNKNOWN;
    }

    /**
     * Retrouve un mod inventorié.
     *
     * @param modId identifiant cherché
     * @return l'entrée, ou {@code null} si ce mod n'est pas inventorié
     */
    public ModEntry mod(String modId) {
        return byModId.get(modId);
    }

    /** @return les mods inventoriés, dans l'ordre où le chargeur les a donnés */
    public List<ModEntry> mods() {
        return mods;
    }

    /** @return le nombre de mods inventoriés ({@code rfx.discovery.mods}) */
    public int modCount() {
        return mods.size();
    }

    /** @return le nombre de modules connus */
    public int knownModules() {
        return moduleToMod.size();
    }

    /** @return le nombre de paquets connus */
    public int knownPackages() {
        return packageToMod.size();
    }

    /**
     * Durée de la découverte, en millisecondes ({@code rfx.discovery.duration_ms}).
     *
     * <p>R-621 la borne à 500 ms pour 250 mods. Elle ne compte aucun calcul
     * d'empreinte : ceux-ci ont lieu à la demande, hors découverte (ADR-023).
     *
     * @return la durée mesurée, arrondie à la milliseconde inférieure
     */
    public long durationMs() {
        return durationNanos / 1_000_000L;
    }

    /** @return la durée exacte de la découverte, en nanosecondes */
    public long durationNanos() {
        return durationNanos;
    }
}
