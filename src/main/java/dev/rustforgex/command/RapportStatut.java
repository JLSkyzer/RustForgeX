package dev.rustforgex.command;

import dev.rustforgex.bootstrap.Bootstrap;
import dev.rustforgex.config.Configuration;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * C-38 : composition du rapport affiché par {@code /rfx status}.
 *
 * <p>Cahier des charges : PARTIE 5.36. Test : T-420. Maturité : {@code STABLE}.
 *
 * <p>La composition est séparée de l'enregistrement de la commande afin d'être
 * testable sans démarrer Minecraft. Elle ne fait aucun appel natif : elle met en forme
 * un statut déjà lu, ce qui garantit que l'affichage ne peut pas bloquer le thread
 * serveur (R-601).
 *
 * <p>Aucune valeur n'est inventée : un champ que la sonde n'a pas mesuré est affiché
 * comme « non mesuré », jamais comme zéro (R-660, contrat agent 3.2).
 */
public final class RapportStatut {

    private RapportStatut() {
        throw new AssertionError("classe utilitaire, non instanciable");
    }

    /**
     * Compose les lignes du rapport.
     *
     * @param rapport rapport de démarrage produit par C-02
     * @param configuration configuration effective
     * @param statutNatif statut décodé publié par le runtime natif, ou {@code null}
     *     si le runtime n'est pas actif
     * @param observationSeule {@code true} si C-01 a imposé l'observation seule
     * @return les lignes à afficher, jamais vides
     */
    public static List<String> composer(
            Bootstrap.Rapport rapport,
            Configuration configuration,
            Map<String, Object> statutNatif,
            boolean observationSeule) {

        List<String> lignes = new ArrayList<>();
        lignes.add("RUSTFORGE-X — jalon M0 (bootstrap)");
        lignes.add("État        : " + etatLisible(rapport, observationSeule));
        lignes.add("Mode        : " + configuration.mode());
        lignes.add("Démarrage   : " + rapport.dureeMs() + " ms");

        if (rapport.code() != null) {
            lignes.add("Cause       : " + rapport.code());
        }
        if (rapport.bibliotheque() != null) {
            lignes.add("Binaire     : " + rapport.bibliotheque().getFileName());
        }

        if (statutNatif == null) {
            lignes.add("Runtime natif inactif : le jeu tourne en Java pur, sans instrumentation.");
            return lignes;
        }

        lignes.add("ABI native  : " + entier(statutNatif, "abi_version")
                + "   version " + texte(statutNatif, "version_native"));
        lignes.add("Panics      : " + entier(statutNatif, "panics"));

        Map<String, Object> materiel = table(statutNatif, "materiel");
        Map<String, Object> couverture = table(statutNatif, "couverture_sonde");
        if (materiel != null) {
            lignes.add("Matériel    : " + decrireMateriel(materiel, couverture));
            lignes.add("Coût FFI    : " + decrireCouts(materiel, couverture));
        }

        Object composants = statutNatif.get("composants");
        if (composants instanceof List<?> liste && !liste.isEmpty()) {
            lignes.add("Composants  :");
            for (Object element : liste) {
                if (element instanceof Map<?, ?> composant) {
                    lignes.add("  " + decrireComposant(composant));
                }
            }
        }
        return lignes;
    }

    private static String etatLisible(Bootstrap.Rapport rapport, boolean observationSeule) {
        if (observationSeule) {
            return "OBSERVE_ONLY (version de Forge hors plage supportée)";
        }
        return switch (rapport.etat()) {
            case READY -> "READY — runtime natif actif, aucune transformation à ce jalon";
            case DEGRADED -> "DEGRADED — Java pur, le jeu n'est pas affecté";
            case DISABLED -> "DISABLED — désactivé, aucun appel natif";
            default -> rapport.etat().name();
        };
    }

    private static String decrireMateriel(Map<String, Object> materiel, Map<String, Object> couverture) {
        StringBuilder texte = new StringBuilder();
        texte.append(entier(materiel, "physical_cores")).append(" cœurs physiques / ")
                .append(entier(materiel, "logical_cores")).append(" logiques");

        if (mesure(couverture, "mem_total")) {
            long octets = entier(materiel, "mem_total_bytes");
            texte.append(", ").append(octets / (1024L * 1024L * 1024L)).append(" Gio");
        } else {
            texte.append(", mémoire non mesurée");
        }

        if (mesure(couverture, "l3")) {
            texte.append(", L3 ").append(entier(materiel, "l3_bytes") / 1024L).append(" Kio");
        }
        if (mesure(couverture, "numa")) {
            texte.append(", ").append(entier(materiel, "numa_nodes")).append(" nœud(s) NUMA");
        }

        Map<String, Object> simd = table(materiel, "simd");
        if (simd != null) {
            List<String> jeux = new ArrayList<>();
            for (String nom : List.of("sse2", "avx2", "avx512", "neon")) {
                if (Boolean.TRUE.equals(simd.get(nom))) {
                    jeux.add(nom.toUpperCase(Locale.ROOT));
                }
            }
            texte.append(", SIMD ").append(jeux.isEmpty() ? "aucun" : String.join(" ", jeux));
        }
        return texte.toString();
    }

    private static String decrireCouts(Map<String, Object> materiel, Map<String, Object> couverture) {
        String appel = mesure(couverture, "ffi_call")
                ? entier(materiel, "jni_call_ns") + " ns par aller-retour"
                : "non mesuré";
        String transfert = mesure(couverture, "ffi_transfer")
                ? entier(materiel, "ffi_batch_ns_per_kb") + " ns par Kio"
                : "non mesuré";
        return appel + ", " + transfert;
    }

    private static String decrireComposant(Map<?, ?> composant) {
        Object actif = composant.get("actif");
        return composant.get("id") + " " + composant.get("nom")
                + " [" + composant.get("maturite") + "] "
                + (Boolean.TRUE.equals(actif) ? "actif" : "inactif");
    }

    private static boolean mesure(Map<String, Object> couverture, String cle) {
        return couverture != null && Boolean.TRUE.equals(couverture.get(cle));
    }

    private static long entier(Map<String, Object> table, String cle) {
        Object v = table == null ? null : table.get(cle);
        return v instanceof Long n ? n : 0L;
    }

    private static String texte(Map<String, Object> table, String cle) {
        Object v = table == null ? null : table.get(cle);
        return v instanceof String s ? s : "?";
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> table(Map<String, Object> parent, String cle) {
        Object v = parent == null ? null : parent.get(cle);
        return v instanceof Map ? (Map<String, Object>) v : null;
    }
}
