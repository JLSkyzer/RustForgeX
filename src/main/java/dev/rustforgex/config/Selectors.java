package dev.rustforgex.config;

import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

/**
 * Lecture des listes de sélecteurs de {@code [overrides]} (PARTIE 28.2, ADR-067).
 *
 * <p>Composant : C-37, au service de C-33. Maturité : {@code STABLE}.
 *
 * <p>Un sélecteur vise un propriétaire, {@code modid}, ou une méthode,
 * {@code modid:paquet.Classe#methode}. C'est le seul endroit où l'utilisateur écrit un
 * nom de mod (R-550) ; le moteur natif ne fait que comparer ces noms à ceux des unités.
 * La grammaire est celle de {@code rfx_decision::Selector::parse}, que le natif applique
 * de nouveau.
 */
final class Selectors {

    private Selectors() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * @param text un sélecteur
     * @return {@code true} s'il est bien formé
     */
    static boolean valid(String text) {
        String selector = text.trim();
        int colon = selector.indexOf(':');
        String owner = colon < 0 ? selector : selector.substring(0, colon);
        if (owner.isEmpty() || owner.indexOf('#') >= 0) {
            return false;
        }
        if (colon < 0) {
            return true;
        }
        String rest = selector.substring(colon + 1);
        int hash = rest.indexOf('#');
        if (hash < 0) {
            return false;
        }
        String clazz = rest.substring(0, hash);
        String method = rest.substring(hash + 1);
        return !clazz.isEmpty() && !method.isEmpty() && method.indexOf('#') < 0;
    }

    /**
     * Lit un tableau TOML d'une ligne fait de chaînes entre guillemets.
     *
     * @param value la valeur brute, par exemple {@code ["a", "b:c.D#e"]}
     * @return les éléments, sans guillemets, ou rien si ce n'est pas un tel tableau
     */
    static Optional<List<String>> parseArray(String value) {
        String trimmed = value.trim();
        if (trimmed.length() < 2 || trimmed.charAt(0) != '['
                || trimmed.charAt(trimmed.length() - 1) != ']') {
            return Optional.empty();
        }
        String body = trimmed.substring(1, trimmed.length() - 1).trim();
        List<String> items = new ArrayList<>();
        if (body.isEmpty()) {
            return Optional.of(items);
        }
        for (String part : body.split(",", -1)) {
            String item = part.trim();
            if (item.length() < 2 || item.charAt(0) != '"' || item.charAt(item.length() - 1) != '"') {
                return Optional.empty();
            }
            String inner = item.substring(1, item.length() - 1);
            if (inner.indexOf('"') >= 0) {
                return Optional.empty();
            }
            items.add(inner);
        }
        return Optional.of(items);
    }
}
