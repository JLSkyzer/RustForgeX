package dev.rustforgex.config;

import dev.rustforgex.bridge.Cbor;
import dev.rustforgex.config.OptionConfig.TypeValeur;

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
    public static final String PREFIXE_SURCHARGE = "rustforgex.";

    /** Chemin du fichier, relatif à la racine de travail du mod. */
    public static final String CHEMIN_FICHIER = "config/rustforgex.toml";

    /** Modes de fonctionnement (PARTIE 28.3). */
    public static final List<String> MODES =
            List.of("safe", "balanced", "performance", "experimental", "debug");

    /** Schéma normatif : toute option a un défaut, une plage et une description. */
    private static final List<OptionConfig> SCHEMA_OPTIONS = List.of(
            OptionConfig.booleen("general", "enabled", true,
                    "false = RUSTFORGE-X ne fait rien du tout", false),
            OptionConfig.enumeration("general", "mode", "balanced", MODES,
                    "Profil de risque et d'agressivité des décisions", true),
            OptionConfig.booleen("general", "side_client", true,
                    "Activer le runtime sur le client", false),
            OptionConfig.booleen("general", "side_server", true,
                    "Activer le runtime sur le serveur dédié", false),
            OptionConfig.entier("memory", "max_native_mb", 512, 16, 16384,
                    "Plafond de mémoire native, en mébioctets", false),
            OptionConfig.booleen("telemetry", "enabled", true,
                    "Collecte locale des métriques. Aucune donnée ne quitte la machine", true),
            OptionConfig.entier("runtime", "panic_threshold", 3, 1, 100,
                    "Panics tolérées pour un sous-système avant sa désactivation", true));

    private final Map<String, Object> valeurs;
    private final List<String> avertissements;
    private final Map<String, String> clesInconnues;

    private Configuration(
            Map<String, Object> valeurs, List<String> avertissements, Map<String, String> clesInconnues) {
        this.valeurs = Map.copyOf(valeurs);
        this.avertissements = List.copyOf(avertissements);
        this.clesInconnues = Map.copyOf(clesInconnues);
    }

    /** @return le schéma normatif complet. */
    public static List<OptionConfig> schema() {
        return SCHEMA_OPTIONS;
    }

    /** @return la description d'une option, si elle appartient au schéma. */
    public static Optional<OptionConfig> option(String chemin) {
        return SCHEMA_OPTIONS.stream().filter(o -> o.chemin().equals(chemin)).findFirst();
    }

    /** @return une configuration composée uniquement des valeurs par défaut. */
    public static Configuration parDefaut() {
        Map<String, Object> valeurs = new LinkedHashMap<>();
        for (OptionConfig o : SCHEMA_OPTIONS) {
            valeurs.put(o.chemin(), o.defaut());
        }
        return new Configuration(valeurs, List.of(), Map.of());
    }

    /**
     * Charge la configuration depuis un fichier, en appliquant les surcharges système.
     *
     * <p>Le fichier est créé avec les valeurs par défaut s'il n'existe pas. Toute
     * valeur hors plage ou de type incorrect est rejetée avec un message précis et
     * remplacée par le défaut, jamais par un comportement indéfini (PARTIE 28.5).
     *
     * @param fichier chemin du fichier {@code rustforgex.toml}
     * @param proprietesSysteme accès aux propriétés système, injectable pour les tests
     * @return la configuration effective, porteuse de ses avertissements
     */
    public static Configuration charger(Path fichier, UnaryOperator<String> proprietesSysteme) {
        List<String> avertissements = new ArrayList<>();
        Map<String, String> brut = new LinkedHashMap<>();

        if (Files.isRegularFile(fichier)) {
            try {
                brut.putAll(TomlPlat.lire(Files.readString(fichier, StandardCharsets.UTF_8)));
            } catch (IOException e) {
                avertissements.add(
                        "Fichier de configuration illisible (" + e.getMessage()
                                + ") : les valeurs par défaut s'appliquent.");
            }
        } else {
            try {
                ecrireParDefaut(fichier);
            } catch (IOException e) {
                avertissements.add(
                        "Création du fichier de configuration impossible (" + e.getMessage()
                                + ") : les valeurs par défaut s'appliquent, sans persistance.");
            }
        }

        Map<String, Object> valeurs = new LinkedHashMap<>();
        for (OptionConfig o : SCHEMA_OPTIONS) {
            String chemin = o.chemin();
            Object valeur = o.defaut();

            String duFichier = brut.remove(chemin);
            if (duFichier != null) {
                valeur = analyser(o, duFichier, "fichier", avertissements).orElse(o.defaut());
            }

            // Priorité la plus forte : la propriété système (PARTIE 28.4).
            String surcharge = proprietesSysteme.apply(PREFIXE_SURCHARGE + chemin);
            if (surcharge != null) {
                valeur = analyser(o, surcharge, "propriété système", avertissements).orElse(valeur);
            }

            valeurs.put(chemin, valeur);
        }

        // R-591 : ce qui reste dans `brut` est inconnu du schéma. La valeur est
        // conservée pour être réécrite telle quelle, et signalée.
        brut.remove("schema");
        for (String cle : brut.keySet()) {
            avertissements.add(
                    "Option inconnue conservée sans effet : « " + cle
                            + " ». Elle appartient peut-être à un jalon ultérieur.");
        }

        return new Configuration(valeurs, avertissements, brut);
    }

    /** Analyse une valeur textuelle selon le type et la plage de l'option. */
    private static Optional<Object> analyser(
            OptionConfig o, String texte, String origine, List<String> avertissements) {

        String valeur = texte.trim();
        switch (o.type()) {
            case BOOLEEN -> {
                if (valeur.equals("true")) {
                    return Optional.of(Boolean.TRUE);
                }
                if (valeur.equals("false")) {
                    return Optional.of(Boolean.FALSE);
                }
                avertissements.add(rejet(o, valeur, origine, "attendu true ou false"));
                return Optional.empty();
            }
            case ENTIER -> {
                long nombre;
                try {
                    nombre = Long.parseLong(valeur);
                } catch (NumberFormatException e) {
                    avertissements.add(rejet(o, valeur, origine, "attendu un entier"));
                    return Optional.empty();
                }
                if (nombre < o.min() || nombre > o.max()) {
                    avertissements.add(
                            rejet(o, valeur, origine, "hors de la plage " + o.plageLisible()));
                    return Optional.empty();
                }
                return Optional.of(nombre);
            }
            case TEXTE -> {
                String sansGuillemets = deguillemeter(valeur);
                if (!o.valeursAdmises().isEmpty() && !o.valeursAdmises().contains(sansGuillemets)) {
                    avertissements.add(
                            rejet(o, sansGuillemets, origine, "valeurs admises : " + o.plageLisible()));
                    return Optional.empty();
                }
                return Optional.of(sansGuillemets);
            }
            default -> {
                return Optional.empty();
            }
        }
    }

    private static String rejet(OptionConfig o, String valeur, String origine, String raison) {
        return "Valeur rejetée pour « " + o.chemin() + " » (" + origine + ") : « " + valeur
                + " » — " + raison + ". Valeur par défaut appliquée : " + o.defaut() + ".";
    }

    private static String deguillemeter(String valeur) {
        if (valeur.length() >= 2 && valeur.startsWith("\"") && valeur.endsWith("\"")) {
            return valeur.substring(1, valeur.length() - 1);
        }
        return valeur;
    }

    /** Écrit le fichier de configuration commenté, avec toutes les valeurs par défaut. */
    public static void ecrireParDefaut(Path fichier) throws IOException {
        Path parent = fichier.getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.writeString(fichier, rendreToml(parDefaut()), StandardCharsets.UTF_8);
    }

    /**
     * Rend la configuration au format TOML commenté.
     *
     * @param configuration configuration à sérialiser
     * @return le contenu du fichier
     */
    public static String rendreToml(Configuration configuration) {
        StringBuilder sortie = new StringBuilder();
        sortie.append("# Configuration de RUSTFORGE-X\n")
                .append("# Cahier des charges, PARTIE 28. Toute valeur hors plage est rejetee,\n")
                .append("# signalee dans les journaux et remplacee par le defaut.\n")
                .append("# Surcharge possible au lancement : -D").append(PREFIXE_SURCHARGE)
                .append("<section>.<cle>=<valeur>\n\n")
                .append("schema = ").append(SCHEMA).append('\n');

        String sectionCourante = null;
        for (OptionConfig o : SCHEMA_OPTIONS) {
            if (!o.section().equals(sectionCourante)) {
                sectionCourante = o.section();
                sortie.append("\n[").append(sectionCourante).append("]\n");
            }
            sortie.append("# ").append(o.description()).append('\n');
            sortie.append("# valeurs : ").append(o.plageLisible());
            if (!o.rechargeableAChaud()) {
                sortie.append("  (redemarrage requis)");
            }
            sortie.append('\n');
            sortie.append(o.cle()).append(" = ")
                    .append(formater(configuration.valeurs.get(o.chemin())))
                    .append('\n');
        }

        if (!configuration.clesInconnues.isEmpty()) {
            sortie.append("\n# Options inconnues de cette version, conservees telles quelles (R-591).\n");
            sortie.append("[inconnues]\n");
            for (Map.Entry<String, String> e : configuration.clesInconnues.entrySet()) {
                sortie.append("# ").append(e.getKey()).append(" = ").append(e.getValue()).append('\n');
            }
        }
        return sortie.toString();
    }

    private static String formater(Object valeur) {
        return valeur instanceof String texte ? '"' + texte + '"' : String.valueOf(valeur);
    }

    /**
     * @param chemin chemin complet de l'option, par exemple {@code general.enabled}
     * @return la valeur booléenne de l'option
     * @throws IllegalArgumentException si l'option n'existe pas ou n'est pas booléenne
     */
    public boolean booleen(String chemin) {
        return (Boolean) valeurTypee(chemin, TypeValeur.BOOLEEN);
    }

    /**
     * @param chemin chemin complet de l'option
     * @return la valeur entière de l'option
     * @throws IllegalArgumentException si l'option n'existe pas ou n'est pas entière
     */
    public long entier(String chemin) {
        return (Long) valeurTypee(chemin, TypeValeur.ENTIER);
    }

    /**
     * @param chemin chemin complet de l'option
     * @return la valeur textuelle de l'option
     * @throws IllegalArgumentException si l'option n'existe pas ou n'est pas textuelle
     */
    public String texte(String chemin) {
        return (String) valeurTypee(chemin, TypeValeur.TEXTE);
    }

    private Object valeurTypee(String chemin, TypeValeur attendu) {
        OptionConfig o = option(chemin).orElseThrow(
                () -> new IllegalArgumentException("option absente du schéma : " + chemin));
        if (o.type() != attendu) {
            throw new IllegalArgumentException(
                    "option « " + chemin + " » de type " + o.type() + ", lue comme " + attendu);
        }
        Object valeur = valeurs.get(chemin);
        // Une valeur manquante signalerait une incohérence interne du schéma, jamais
        // une saisie utilisateur : celle-ci est toujours remplacée par le défaut.
        return valeur != null ? valeur : o.defaut();
    }

    /** @return les messages produits au chargement : valeurs rejetées, clés inconnues. */
    public List<String> avertissements() {
        return avertissements;
    }

    /** @return les clés inconnues du schéma, conservées telles quelles (R-591). */
    public Map<String, String> clesInconnues() {
        return clesInconnues;
    }

    /** @return {@code true} si RUSTFORGE-X doit s'activer sur ce côté. */
    public boolean activeSur(boolean coteClient) {
        if (!booleen("general.enabled")) {
            return false;
        }
        return coteClient ? booleen("general.side_client") : booleen("general.side_server");
    }

    /** @return le mode de fonctionnement, en minuscules (PARTIE 28.3). */
    public String mode() {
        return texte("general.mode").toLowerCase(Locale.ROOT);
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
    public byte[] versCborNatif() {
        Map<String, Object> table = Cbor.table();
        table.put("schema", SCHEMA);
        table.put("enabled", booleen("general.enabled"));
        table.put("mode", modeNatif(mode()));
        table.put("max_native_mb", entier("memory.max_native_mb"));
        table.put("telemetry_enabled", booleen("telemetry.enabled"));
        table.put("panic_threshold", entier("runtime.panic_threshold"));
        return Cbor.encoder(table);
    }

    /** Convertit un libellé de mode TOML vers le nom de variante attendu par le natif. */
    private static String modeNatif(String mode) {
        return switch (mode) {
            case "safe" -> "Safe";
            case "performance" -> "Performance";
            case "experimental" -> "Experimental";
            case "debug" -> "Debug";
            default -> "Balanced";
        };
    }
}
