package dev.rustforgex.command;

import dev.rustforgex.bootstrap.Bootstrap;
import dev.rustforgex.config.Configuration;
import dev.rustforgex.diag.CodeErreur;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Test T-420 : {@code /rfx status} rend compte de l'état réel, sans rien inventer. */
class RapportStatutTest {

    private static Bootstrap.Rapport rapportPret() {
        return new Bootstrap.Rapport(
                Bootstrap.Etat.READY, null, "Runtime natif prêt (rfx_native.dll).",
                0x5246_5800_0000_0001L, Path.of("native", "rfx_native.dll"), 42, List.of());
    }

    private static Map<String, Object> statutNatif(boolean coutsMesures, boolean l3Mesure) {
        Map<String, Object> simd = new LinkedHashMap<>();
        simd.put("sse2", Boolean.TRUE);
        simd.put("avx2", Boolean.TRUE);
        simd.put("avx512", Boolean.FALSE);
        simd.put("neon", Boolean.FALSE);

        Map<String, Object> materiel = new LinkedHashMap<>();
        materiel.put("physical_cores", 8L);
        materiel.put("logical_cores", 16L);
        materiel.put("l3_bytes", l3Mesure ? 33_554_432L : 0L);
        materiel.put("numa_nodes", 1L);
        materiel.put("mem_total_bytes", 34_359_738_368L);
        materiel.put("jni_call_ns", coutsMesures ? 250L : 0L);
        materiel.put("ffi_batch_ns_per_kb", coutsMesures ? 40L : 0L);
        materiel.put("simd", simd);

        Map<String, Object> couverture = new LinkedHashMap<>();
        couverture.put("cores", Boolean.TRUE);
        couverture.put("topology", Boolean.TRUE);
        couverture.put("l3", l3Mesure);
        couverture.put("numa", Boolean.TRUE);
        couverture.put("simd", Boolean.TRUE);
        couverture.put("mem_total", Boolean.TRUE);
        couverture.put("ffi_call", coutsMesures);
        couverture.put("ffi_transfer", coutsMesures);

        Map<String, Object> composant = new LinkedHashMap<>();
        composant.put("id", "C-27");
        composant.put("nom", "Rust Runtime Core");
        composant.put("maturite", "Stable");
        composant.put("actif", Boolean.TRUE);

        Map<String, Object> statut = new LinkedHashMap<>();
        statut.put("schema", 1L);
        statut.put("abi_version", 1L);
        statut.put("version_native", "0.1.0");
        statut.put("etat", "RUNNING");
        statut.put("panics", 0L);
        statut.put("materiel", materiel);
        statut.put("couverture_sonde", couverture);
        statut.put("composants", List.of(composant));
        return statut;
    }

    private static String joint(List<String> lignes) {
        return String.join("\n", lignes);
    }

    @Test
    @DisplayName("T-420 : le statut nominal expose l'état, le matériel et les composants")
    void statutNominal() {
        String texte = joint(RapportStatut.composer(
                rapportPret(), Configuration.parDefaut(), statutNatif(true, true), false));

        assertTrue(texte.contains("READY"), texte);
        assertTrue(texte.contains("balanced"), texte);
        assertTrue(texte.contains("42 ms"), texte);
        assertTrue(texte.contains("8 cœurs physiques / 16 logiques"), texte);
        assertTrue(texte.contains("32 Gio"), texte);
        assertTrue(texte.contains("L3 32768 Kio"), texte);
        assertTrue(texte.contains("SIMD SSE2 AVX2"), texte);
        assertTrue(texte.contains("250 ns par aller-retour"), texte);
        assertTrue(texte.contains("40 ns par Kio"), texte);
        assertTrue(texte.contains("C-27 Rust Runtime Core [Stable] actif"), texte);
    }

    @Test
    @DisplayName("R-660 : un coût non mesuré est affiché comme tel, jamais comme zéro")
    void coutsNonMesuresAffichesCommeTels() {
        String texte = joint(RapportStatut.composer(
                rapportPret(), Configuration.parDefaut(), statutNatif(false, false), false));

        assertTrue(texte.contains("Coût FFI    : non mesuré, non mesuré"), texte);
        assertFalse(texte.contains("0 ns"), "un zéro ne doit jamais passer pour une mesure : " + texte);
        assertFalse(texte.contains("L3 0"), texte);
        assertFalse(texte.contains("L3 "), "un L3 non sondé ne doit pas être affiché : " + texte);
    }

    @Test
    @DisplayName("Le mode dégradé dit explicitement que le jeu n'est pas affecté")
    void statutDegrade() {
        Bootstrap.Rapport degrade = new Bootstrap.Rapport(
                Bootstrap.Etat.DEGRADED, CodeErreur.NATIF_ABSENT,
                "Aucun binaire natif embarqué pour linux-aarch64.",
                0, null, 7, List.of());

        String texte = joint(RapportStatut.composer(
                degrade, Configuration.parDefaut(), null, false));

        assertTrue(texte.contains("DEGRADED"), texte);
        assertTrue(texte.contains("le jeu n'est pas affecté"), texte);
        assertTrue(texte.contains("E-1005"), texte);
        assertTrue(texte.contains("Java pur"), texte);
    }

    @Test
    @DisplayName("FM-01 : l'observation seule est signalée avec sa cause")
    void statutObservationSeule() {
        Bootstrap.Rapport horsPlage = new Bootstrap.Rapport(
                Bootstrap.Etat.DEGRADED, CodeErreur.FORGE_HORS_PLAGE,
                "Version de Forge « 48.0.1 » hors de la plage supportée [47,48).",
                0, null, 1, List.of());

        String texte = joint(RapportStatut.composer(
                horsPlage, Configuration.parDefaut(), null, true));

        assertTrue(texte.contains("OBSERVE_ONLY"), texte);
        assertTrue(texte.contains("E-1001"), texte);
    }

    @Test
    @DisplayName("Un statut natif illisible ne fait pas échouer la commande")
    void statutNatifAbsentTolere() {
        List<String> lignes = RapportStatut.composer(
                rapportPret(), Configuration.parDefaut(), null, false);

        assertFalse(lignes.isEmpty());
        assertTrue(joint(lignes).contains("Runtime natif inactif"), joint(lignes));
    }
}
