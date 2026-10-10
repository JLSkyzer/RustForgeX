package dev.rustforgex.config;

import dev.rustforgex.bridge.Cbor;
import dev.rustforgex.config.OptionConfig.ValueType;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.function.UnaryOperator;

/**
 * C-37 : configuration de RUSTFORGE-X.
 *
 * <p>Cahier des charges : PARTIE 28. Exigences : R-590 (défaut, plage et description
 * pour chaque option), R-591 (une clé inconnue est conservée et signalée, jamais
 * supprimée), R-592 (rechargement à chaud interdit pour les options structurelles).
 * Test : T-007. Maturité : {@code STABLE}.
 *
 * <p>Fichier : {@code <gameDir>/rustforgex/config/rustforgex.toml}, créé avec les
 * valeurs par défaut et leurs commentaires au premier lancement.
 *
 * <p>Priorité des sources, du plus fort au plus faible (PARTIE 28.4) :
 *
 * <ol>
 *   <li>propriétés système {@code -Drustforgex.<section>.<clé>=<valeur>}
 *   <li>fichier {@code rustforgex.toml}
 *   <li>valeurs par défaut
 * </ol>
 *
 * <p>Les surcharges par {@code /rfx set}, de priorité intermédiaire, seront ajoutées
 * avec la commande correspondante (C-38).
 *
 * <p>Le schéma ne déclare que les options réellement lues par du code implémenté. Les
 * autres sections de la PARTIE 28.2 seront ajoutées au jalon qui les mobilise : une
 * option qui ne pilote rien tromperait l'utilisateur (contrat agent 3.1).
 */
public final class Configuration {

    /** Version du schéma de configuration (clé {@code schema} du fichier). */
    public static final int SCHEMA = 1;

    /** Préfixe des propriétés système de surcharge (PARTIE 28.4). */
    public static final String OVERRIDE_PREFIX = "rustforgex.";

    /** Chemin du fichier, relatif à la racine de travail du mod. */
    public static final String FILE_PATH = "config/rustforgex.toml";

    /** Modes de fonctionnement (PARTIE 28.3). */
    public static final List<String> MODES =
            List.of("safe", "balanced", "performance", "experimental", "debug");

    /** Schéma normatif : toute option a un défaut, une plage et une description. */
    private static final List<OptionConfig> SCHEMA_OPTIONS = List.of(
            OptionConfig.booleanOption("general", "enabled", true,
                    "false = RUSTFORGE-X ne fait rien du tout", false),
            OptionConfig.enumeration("general", "mode", "balanced", MODES,
                    "Profil de risque et d'agressivité des décisions", true),
            OptionConfig.booleanOption("general", "side_client", true,
                    "Activer le runtime sur le client", false),
            OptionConfig.booleanOption("general", "side_server", true,
                    "Activer le runtime sur le serveur dédié", false),
            OptionConfig.integerOption("memory", "max_native_mb", 512, 16, 16384,
                    "Plafond de mémoire native, en mébioctets", false),
            OptionConfig.booleanOption("telemetry", "enabled", true,
                    "Collecte locale des métriques. Aucune donnée ne quitte la machine", true),
            OptionConfig.integerOption("runtime", "panic_threshold", 3, 1, 100,
                    "Panics tolérées pour un sous-système avant sa désactivation", true),
            OptionConfig.integerOption("profiler", "max_workloads", 20_000, 1_000, 200_000,
                    "Unités de travail suivies simultanément ; au-delà, la plus froide "
                            + "est évincée", false),
            OptionConfig.integerOption("profiler", "cpu_budget_pct", 2, 1, 50,
                    "Part d'un cœur accordée au profilage, en pourcent ; au-delà, la "
                            + "profondeur de sondage descend", false),
            OptionConfig.integerOption("profiler", "baseline_period_ticks", 2_048, 20, 6_000,
                    "Ticks entre deux mises en pause de mesure du coût (PARTIE 12.4)",
                    false),
            OptionConfig.integerOption("profiler", "baseline_pause_ticks", 512, 5, 512,
                    "Durée d'une pause de mesure, en ticks. Fixe aussi la taille des "
                            + "deux fenêtres comparées : trop courte, la mesure ne "
                            + "distingue pas le coût du bruit", false),
            OptionConfig.integerOption("profiler", "baseline_cycles", 30, 3, 64,
                    "Cycles agrégés avant qu'une mesure de coût soit rendue. Une mesure "
                            + "coûte cycles × (période + pause) ticks", false),
            OptionConfig.booleanOption("diagnostics", "report_on_incident", true,
                    "Consigner chaque incident dans crash/rfx-crash-<ts>.json : accroche "
                            + "désactivée, panic native. Fichier local, chemins anonymisés",
                    false),
            OptionConfig.booleanOption("instrumentation", "early_arm", false,
                    "Armer le sondage dès la construction du mod plutôt qu'au setup "
                            + "commun. Multiplie par cinq la surface observée — et par "
                            + "cinq le coût, mesuré à 105 ns par sonde armée et par tick "
                            + "(ADR-026). Hors budget par défaut", false));

    private final Map<String, Object> values;
    private final List<String> warnings;
    private final Map<String, String> unknownKeys;

    private Configuration(
            Map<String, Object> values, List<String> warnings, Map<String, String> unknownKeys) {
        this.values = Map.copyOf(values);
        this.warnings = List.copyOf(warnings);
        this.unknownKeys = Map.copyOf(unknownKeys);
    }

    /** @return le schéma normatif complet. */
    public static List<OptionConfig> schema() {
        return SCHEMA_OPTIONS;
    }

    /**
     * @param path chemin complet de l'option
     * @return la description de l'option, si elle appartient au schéma
     */
    public static Optional<OptionConfig> option(String path) {
        return SCHEMA_OPTIONS.stream().filter(o -> o.path().equals(path)).findFirst();
    }

    /** @return une configuration composée uniquement des valeurs par défaut. */
    public static Configuration defaults() {
        Map<String, Object> values = new LinkedHashMap<>();
        for (OptionConfig o : SCHEMA_OPTIONS) {
            values.put(o.path(), o.defaultValue());
        }
        return new Configuration(values, List.of(), Map.of());
    }

    /**
     * Charge la configuration depuis un fichier, en appliquant les surcharges système.
     *
     * <p>Le fichier est créé avec les valeurs par défaut s'il n'existe pas. Toute
     * valeur hors plage ou de type incorrect est rejetée avec un message précis et
     * remplacée par le défaut, jamais par un comportement indéfini (PARTIE 28.5).
     *
     * @param file chemin du fichier {@code rustforgex.toml}
     * @param systemProperties accès aux propriétés système, injectable pour les tests
     * @return la configuration effective, porteuse de ses avertissements
     */
    public static Configuration load(Path file, UnaryOperator<String> systemProperties) {
        List<String> warnings = new ArrayList<>();
        Map<String, String> raw = new LinkedHashMap<>();

        if (Files.isRegularFile(file)) {
            try {
                raw.putAll(FlatToml.read(Files.readString(file, StandardCharsets.UTF_8)));
            } catch (IOException e) {
                warnings.add(
                        "Fichier de configuration illisible (" + e.getMessage()
                                + ") : les valeurs par défaut s'appliquent.");
            }
        } else {
            try {
                writeDefaults(file);
            } catch (IOException e) {
                warnings.add(
                        "Création du fichier de configuration impossible (" + e.getMessage()
                                + ") : les valeurs par défaut s'appliquent, sans persistance.");
            }
        }

        Map<String, Object> values = new LinkedHashMap<>();
        for (OptionConfig o : SCHEMA_OPTIONS) {
            String path = o.path();
            Object value = o.defaultValue();

            String fromFile = raw.remove(path);
            if (fromFile != null) {
                value = parse(o, fromFile, "fichier", warnings).orElse(o.defaultValue());
            }

            // Priorité la plus forte : la propriété système (PARTIE 28.4).
            String override = systemProperties.apply(OVERRIDE_PREFIX + path);
            if (override != null) {
                value = parse(o, override, "propriété système", warnings).orElse(value);
            }

            values.put(path, value);
        }

        // R-591 : ce qui reste dans `raw` est inconnu du schéma. La valeur est
        // conservée pour être réécrite telle quelle, et signalée.
        raw.remove("schema");
        for (String key : raw.keySet()) {
            warnings.add(
                    "Option inconnue conservée sans effet : « " + key
                            + " ». Elle appartient peut-être à un jalon ultérieur.");
        }

        return new Configuration(values, warnings, raw);
    }

    /** Analyse une valeur textuelle selon le type et la plage de l'option. */
    private static Optional<Object> parse(
            OptionConfig o, String text, String origin, List<String> warnings) {

        String value = text.trim();
        switch (o.type()) {
            case BOOLEAN -> {
                if (value.equals("true")) {
                    return Optional.of(Boolean.TRUE);
                }
                if (value.equals("false")) {
                    return Optional.of(Boolean.FALSE);
                }
                warnings.add(rejection(o, value, origin, "attendu true ou false"));
                return Optional.empty();
            }
            case INTEGER -> {
                long number;
                try {
                    number = Long.parseLong(value);
                } catch (NumberFormatException e) {
                    warnings.add(rejection(o, value, origin, "attendu un entier"));
                    return Optional.empty();
                }
                if (number < o.min() || number > o.max()) {
                    warnings.add(
                            rejection(o, value, origin, "hors de la plage " + o.readableRange()));
                    return Optional.empty();
                }
                return Optional.of(number);
            }
            case TEXT -> {
                String unquoted = unquote(value);
                if (!o.allowedValues().isEmpty() && !o.allowedValues().contains(unquoted)) {
                    warnings.add(
                            rejection(o, unquoted, origin, "valeurs admises : " + o.readableRange()));
                    return Optional.empty();
                }
                return Optional.of(unquoted);
            }
            default -> {
                return Optional.empty();
            }
        }
    }

    private static String rejection(OptionConfig o, String value, String origin, String reason) {
        return "Valeur rejetée pour « " + o.path() + " » (" + origin + ") : « " + value
                + " » — " + reason + ". Valeur par défaut appliquée : " + o.defaultValue() + ".";
    }

    private static String unquote(String value) {
        if (value.length() >= 2 && value.startsWith("\"") && value.endsWith("\"")) {
            return value.substring(1, value.length() - 1);
        }
        return value;
    }

    /**
     * Écrit le fichier de configuration commenté, avec toutes les valeurs par défaut.
     *
     * @param file chemin du fichier à créer
     * @throws IOException si le fichier ne peut pas être écrit
     */
    public static void writeDefaults(Path file) throws IOException {
        Path parent = file.getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.writeString(file, renderToml(defaults()), StandardCharsets.UTF_8);
    }

    /**
     * Rend la configuration au format TOML commenté.
     *
     * @param configuration configuration à sérialiser
     * @return le contenu du fichier
     */
    public static String renderToml(Configuration configuration) {
        StringBuilder out = new StringBuilder();
        out.append("# Configuration de RUSTFORGE-X\n")
                .append("# Cahier des charges, PARTIE 28. Toute valeur hors plage est rejetee,\n")
                .append("# signalee dans les journaux et remplacee par le defaut.\n")
                .append("# Surcharge possible au lancement : -D").append(OVERRIDE_PREFIX)
                .append("<section>.<cle>=<valeur>\n\n")
                .append("schema = ").append(SCHEMA).append('\n');

        String currentSection = null;
        for (OptionConfig o : SCHEMA_OPTIONS) {
            if (!o.section().equals(currentSection)) {
                currentSection = o.section();
                out.append("\n[").append(currentSection).append("]\n");
            }
            out.append("# ").append(o.description()).append('\n');
            out.append("# valeurs : ").append(o.readableRange());
            if (!o.hotReloadable()) {
                out.append("  (redemarrage requis)");
            }
            out.append('\n');
            out.append(o.key()).append(" = ")
                    .append(format(configuration.values.get(o.path())))
                    .append('\n');
        }

        if (!configuration.unknownKeys.isEmpty()) {
            out.append("\n# Options inconnues de cette version, conservees telles quelles (R-591).\n");
            out.append("[inconnues]\n");
            for (Map.Entry<String, String> e : configuration.unknownKeys.entrySet()) {
                out.append("# ").append(e.getKey()).append(" = ").append(e.getValue()).append('\n');
            }
        }
        return out.toString();
    }

    private static String format(Object value) {
        return value instanceof String text ? '"' + text + '"' : String.valueOf(value);
    }

    /**
     * @param path chemin complet de l'option, par exemple {@code general.enabled}
     * @return la valeur booléenne de l'option
     * @throws IllegalArgumentException si l'option n'existe pas ou n'est pas booléenne
     */
    public boolean getBoolean(String path) {
        return (Boolean) typedValue(path, ValueType.BOOLEAN);
    }

    /**
     * @param path chemin complet de l'option
     * @return la valeur entière de l'option
     * @throws IllegalArgumentException si l'option n'existe pas ou n'est pas entière
     */
    public long getLong(String path) {
        return (Long) typedValue(path, ValueType.INTEGER);
    }

    /**
     * @param path chemin complet de l'option
     * @return la valeur textuelle de l'option
     * @throws IllegalArgumentException si l'option n'existe pas ou n'est pas textuelle
     */
    public String getString(String path) {
        return (String) typedValue(path, ValueType.TEXT);
    }

    private Object typedValue(String path, ValueType expected) {
        OptionConfig o = option(path).orElseThrow(
                () -> new IllegalArgumentException("option absente du schéma : " + path));
        if (o.type() != expected) {
            throw new IllegalArgumentException(
                    "option « " + path + " » de type " + o.type() + ", lue comme " + expected);
        }
        Object value = values.get(path);
        // Une valeur manquante signalerait une incohérence interne du schéma, jamais
        // une saisie utilisateur : celle-ci est toujours remplacée par le défaut.
        return value != null ? value : o.defaultValue();
    }

    /** @return les messages produits au chargement : valeurs rejetées, clés inconnues. */
    public List<String> warnings() {
        return warnings;
    }

    /** @return les clés inconnues du schéma, conservées telles quelles (R-591). */
    public Map<String, String> unknownKeys() {
        return unknownKeys;
    }

    /**
     * @param clientSide {@code true} pour le côté client
     * @return {@code true} si RUSTFORGE-X doit s'activer sur ce côté
     */
    public boolean enabledOn(boolean clientSide) {
        if (!getBoolean("general.enabled")) {
            return false;
        }
        return clientSide ? getBoolean("general.side_client") : getBoolean("general.side_server");
    }

    /** @return le mode de fonctionnement, en minuscules (PARTIE 28.3). */
    public String mode() {
        return getString("general.mode").toLowerCase(Locale.ROOT);
    }

    /**
     * Sérialise en CBOR le sous-ensemble de la configuration destiné au runtime natif.
     *
     * <p>Les clés et les libellés produits correspondent exactement à
     * {@code rfx_model::RuntimeConfig} (R-704). Le mode est transmis sous la forme du
     * nom de variante Rust, seule forme que le décodeur natif reconnaît.
     *
     * @return le blob à passer à {@code rfx_init}
     */
    public byte[] toNativeCbor() {
        Map<String, Object> table = Cbor.map();
        table.put("schema", SCHEMA);
        table.put("enabled", getBoolean("general.enabled"));
        table.put("mode", nativeMode(mode()));
        table.put("max_native_mb", getLong("memory.max_native_mb"));
        table.put("telemetry_enabled", getBoolean("telemetry.enabled"));
        table.put("panic_threshold", getLong("runtime.panic_threshold"));
        table.put("profiler_baseline_cycles", getLong("profiler.baseline_cycles"));
        table.put("profiler_baseline_period_ticks", getLong("profiler.baseline_period_ticks"));
        table.put("profiler_baseline_pause_ticks", getLong("profiler.baseline_pause_ticks"));
        table.put("profiler_max_workloads", getLong("profiler.max_workloads"));
        table.put("profiler_cpu_budget_pct", getLong("profiler.cpu_budget_pct"));
        return Cbor.encode(table);
    }

    /** Convertit un libellé de mode TOML vers le nom de variante attendu par le natif. */
    private static String nativeMode(String mode) {
        return switch (mode) {
            case "safe" -> "Safe";
            case "performance" -> "Performance";
            case "experimental" -> "Experimental";
            case "debug" -> "Debug";
            default -> "Balanced";
        };
    }
}
