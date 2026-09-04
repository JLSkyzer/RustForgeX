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
 * @param cle nom de l'option dans sa section
 * @param type type de la valeur
 * @param defaut valeur par défaut, toujours sûre
 * @param min borne inférieure incluse, pour un entier ; ignorée sinon
 * @param max borne supérieure incluse, pour un entier ; ignorée sinon
 * @param valeursAdmises libellés acceptés, pour un texte énuméré ; vide sinon
 * @param description phrase reprise en commentaire dans le fichier généré
 * @param rechargeableAChaud {@code true} si {@code /rfx set} peut la modifier en
 *     cours de partie ; {@code false} si elle exige un redémarrage (R-592)
 */
public record OptionConfig(
        String section,
        String cle,
        TypeValeur type,
        Object defaut,
        long min,
        long max,
        List<String> valeursAdmises,
        String description,
        boolean rechargeableAChaud) {

    /** Types de valeurs représentables dans le fichier de configuration. */
    public enum TypeValeur {
        /** {@code true} ou {@code false}. */
        BOOLEEN,
        /** Entier signé, borné par {@link OptionConfig#min} et {@link OptionConfig#max}. */
        ENTIER,
        /** Chaîne, éventuellement restreinte à {@link OptionConfig#valeursAdmises}. */
        TEXTE
    }

    /** @return le chemin complet de l'option, par exemple {@code general.mode}. */
    public String chemin() {
        return section + "." + cle;
    }

    /**
     * Crée une option booléenne.
     *
     * @param section section du fichier
     * @param cle nom de l'option
     * @param defaut valeur par défaut
     * @param description description reprise en commentaire
     * @param rechargeableAChaud modifiable sans redémarrage
     * @return l'option décrite
     */
    public static OptionConfig booleen(
            String section, String cle, boolean defaut, String description, boolean rechargeableAChaud) {
        return new OptionConfig(
                section, cle, TypeValeur.BOOLEEN, defaut, 0, 0, List.of(), description, rechargeableAChaud);
    }

    /**
     * Crée une option entière bornée.
     *
     * @param section section du fichier
     * @param cle nom de l'option
     * @param defaut valeur par défaut
     * @param min borne inférieure incluse
     * @param max borne supérieure incluse
     * @param description description reprise en commentaire
     * @param rechargeableAChaud modifiable sans redémarrage
     * @return l'option décrite
     */
    public static OptionConfig entier(
            String section,
            String cle,
            long defaut,
            long min,
            long max,
            String description,
            boolean rechargeableAChaud) {
        return new OptionConfig(
                section, cle, TypeValeur.ENTIER, defaut, min, max, List.of(), description, rechargeableAChaud);
    }

    /**
     * Crée une option textuelle restreinte à un ensemble de libellés.
     *
     * @param section section du fichier
     * @param cle nom de l'option
     * @param defaut valeur par défaut, qui doit figurer parmi les libellés admis
     * @param valeursAdmises libellés acceptés
     * @param description description reprise en commentaire
     * @param rechargeableAChaud modifiable sans redémarrage
     * @return l'option décrite
     */
    public static OptionConfig enumeration(
            String section,
            String cle,
            String defaut,
            List<String> valeursAdmises,
            String description,
            boolean rechargeableAChaud) {
        return new OptionConfig(
                section,
                cle,
                TypeValeur.TEXTE,
                defaut,
                0,
                0,
                List.copyOf(valeursAdmises),
                description,
                rechargeableAChaud);
    }

    /** @return la plage admise, sous forme lisible, ou une chaîne vide s'il n'y en a pas. */
    public String plageLisible() {
        return switch (type) {
            case BOOLEEN -> "true | false";
            case ENTIER -> min + " .. " + max;
            case TEXTE -> valeursAdmises.isEmpty() ? "" : String.join(" | ", valeursAdmises);
        };
    }
}
