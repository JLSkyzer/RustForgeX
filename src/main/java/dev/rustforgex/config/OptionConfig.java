package dev.rustforgex.config;

import java.util.List;

/**
 * Description normative d'une option de configuration.
 *
 * <p>Composant : C-37. Cahier des charges : PARTIE 28. Exigence : R-590 — toute option
 * DOIT avoir un défaut sûr, une plage validée et une description. Test : T-007.
 * Maturité : {@code STABLE}.
 *
 * @param section section du fichier TOML, sans crochets
 * @param key nom de l'option dans sa section
 * @param type type de la valeur
 * @param defaultValue valeur par défaut, toujours sûre
 * @param min borne inférieure incluse, pour un entier ; ignorée sinon
 * @param max borne supérieure incluse, pour un entier ; ignorée sinon
 * @param allowedValues libellés acceptés, pour un texte énuméré ; vide sinon
 * @param description phrase reprise en commentaire dans le fichier généré
 * @param hotReloadable {@code true} si {@code /rfx set} peut la modifier en cours de
 *     partie ; {@code false} si elle exige un redémarrage (R-592)
 */
public record OptionConfig(
        String section,
        String key,
        ValueType type,
        Object defaultValue,
        long min,
        long max,
        List<String> allowedValues,
        String description,
        boolean hotReloadable) {

    /** Types de valeurs représentables dans le fichier de configuration. */
    public enum ValueType {
        /** {@code true} ou {@code false}. */
        BOOLEAN,
        /** Entier signé, borné par {@link OptionConfig#min} et {@link OptionConfig#max}. */
        INTEGER,
        /** Chaîne, éventuellement restreinte à {@link OptionConfig#allowedValues}. */
        TEXT
    }

    /** @return le chemin complet de l'option, par exemple {@code general.mode}. */
    public String path() {
        return section + "." + key;
    }

    /**
     * Crée une option booléenne.
     *
     * @param section section du fichier
     * @param key nom de l'option
     * @param defaultValue valeur par défaut
     * @param description description reprise en commentaire
     * @param hotReloadable modifiable sans redémarrage
     * @return l'option décrite
     */
    public static OptionConfig booleanOption(
            String section, String key, boolean defaultValue, String description, boolean hotReloadable) {
        return new OptionConfig(
                section, key, ValueType.BOOLEAN, defaultValue, 0, 0, List.of(), description, hotReloadable);
    }

    /**
     * Crée une option entière bornée.
     *
     * @param section section du fichier
     * @param key nom de l'option
     * @param defaultValue valeur par défaut
     * @param min borne inférieure incluse
     * @param max borne supérieure incluse
     * @param description description reprise en commentaire
     * @param hotReloadable modifiable sans redémarrage
     * @return l'option décrite
     */
    public static OptionConfig integerOption(
            String section,
            String key,
            long defaultValue,
            long min,
            long max,
            String description,
            boolean hotReloadable) {
        return new OptionConfig(
                section, key, ValueType.INTEGER, defaultValue, min, max, List.of(), description, hotReloadable);
    }

    /**
     * Crée une option textuelle restreinte à un ensemble de libellés.
     *
     * @param section section du fichier
     * @param key nom de l'option
     * @param defaultValue valeur par défaut, qui doit figurer parmi les libellés admis
     * @param allowedValues libellés acceptés
     * @param description description reprise en commentaire
     * @param hotReloadable modifiable sans redémarrage
     * @return l'option décrite
     */
    public static OptionConfig enumeration(
            String section,
            String key,
            String defaultValue,
            List<String> allowedValues,
            String description,
            boolean hotReloadable) {
        return new OptionConfig(
                section,
                key,
                ValueType.TEXT,
                defaultValue,
                0,
                0,
                List.copyOf(allowedValues),
                description,
                hotReloadable);
    }

    /** @return la plage admise, sous forme lisible, ou une chaîne vide s'il n'y en a pas. */
    public String readableRange() {
        return switch (type) {
            case BOOLEAN -> "true | false";
            case INTEGER -> min + " .. " + max;
            case TEXT -> allowedValues.isEmpty() ? "" : String.join(" | ", allowedValues);
        };
    }
}
