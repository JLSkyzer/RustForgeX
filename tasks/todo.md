# TODO — RUSTFORGE-X

**Jalon courant : M1 (Observation).** M0 est livré : le mod se charge, mesure la
machine, expose `/rfx status`, et le client rejoint le serveur de développement.
Source de vérité : `docs/spec/RUSTFORGE-X_Cahier_des_Charges_v1.0.md`.

---

## M0 — Bootstrap ✅ LIVRÉ (2026-09-04)

C-27, C-45, C-03, C-02, C-37, C-01, C-38 (`/rfx status`), C-40 (build natif, CI,
artifact-verify). 57 tests Java, 47 tests Rust. Vérifié en jeu : démarrage, sonde,
arrêt propre, connexion client/serveur.

Reste à faire à la première release : `INSTALLATION.md`, `RELEASING.md`.

---

## M1 — Observation

**Livrable** : « je sais ce que fait ce modpack ». Profilage adaptatif complet,
dashboard textuel, rapports, overhead mesuré et affiché sous 2 %.

**Composants** : C-04 (Instrumentation), C-05 (Profiler), C-06 (Event Observer),
C-31 (Memory Manager), C-34 (Telemetry), C-35 (Diagnostics), C-38 (CLI complet),
C-41 (Mod Discovery), C-36 (Benchmark Harness).

**Lire avant de coder** : fiches 5.4, 5.5, 5.6, 5.28, 5.32, 5.33, 5.34, 5.36, 5.39 ;
interfaces IF-02 et IF-03 ; modèles DM-01, DM-02, DM-04 ; PARTIE 12 (budgets).

### Étape A — Cycle de tick (IF-02) et modèle de mesure — T-1xx

- [x] DM-01 `WorkId` : `xxh3_64` sur (owner, classe, méthode, descripteur, contexte, side)
- [x] DM-04 `WorkloadDynamics` : EWMA, histogrammes à buckets log2, `Heat`, `VarianceClass`
- [x] `rfx_tick_begin` / `rfx_phase` / `rfx_tick_end` (IF-02)
- [x] Fermeture implicite d'un tick non terminé, compteur `rfx.tick.unbalanced` (R-706)
- [x] Aucun hook au-dessus de `tick.max_hook_ns` (défaut 500 µs, R-707)
- [x] Hooks de tick côté Java : PRE en priorité HIGHEST, POST en LOWEST

### Étape B — C-31 Memory Manager — T-370..T-373

- [x] Ring buffers de profilage possédés par le natif, taille fixe, écrasement borné
- [x] Budget mémoire natif appliqué (`memory.max_native_mb`, R-533)
- [x] Structures partagées alignées sur 64 octets (R-531)
- [ ] Aucune fuite sur un cycle long (T-370) — demande le test long T-700

### Étape C — IF-03 : flux de profilage

- [ ] `rfx_probe_buffer_acquire` / `rfx_probe_buffer_flush`
- [ ] `ProbeRecord` de 32 octets, little-endian, aligné 8
- [ ] Le buffer appartient au natif ; Java ne le libère jamais (R-708)
- [ ] Un flush n'alloue pas et ne bloque pas ; saturation ⇒ perte comptée (R-709)
- [ ] Une seule traversée FFI par tick et par thread

### Étape D — C-04 JVM Instrumentation — T-130..T-134

- [ ] Transformateur enregistré en dernier dans la chaîne ModLauncher
- [ ] `enter`/`exit` sous `try/finally` : une exception ne fausse jamais un compteur
- [ ] Niveaux `COUNTER` / `TIMED` / `DEEP` avec leurs coûts cibles
- [ ] Refus de sonder : méthodes trop courtes, `<clinit>`, natives, classes du bootstrap (R-311, R-312)
- [ ] Sémantique observable inchangée (R-310)
- [ ] Retrait de sonde si régression JIT au-delà du seuil (R-313)

### Étape E — C-05 Profiler — T-140..T-144

- [ ] Agrégation côté Rust : EWMA, histogrammes, chaleur (DM-04)
- [ ] Adaptation `OFF → LIGHT → NORMAL → DEEP → THROTTLED`
- [ ] Auto-mesure de l'overhead, réduction de niveau au dépassement, `E-1201`
- [ ] Aucune allocation dans le chemin chaud (R-320, T-142)
- [ ] Éviction LRU au-delà de `max_workloads` (R-321)
- [ ] Fonctionne sans aucune sonde, par échantillonnage seul (R-322)

### Étape F — C-06 Event Observer — T-150..T-154

- [ ] Énumération des listeners au `LOAD_COMPLETE`, proxy préservant ordre et priorité
- [ ] L'ordre observable ne change jamais (R-330)
- [ ] Annulation et `Event.Result` respectés (R-331, R-332)
- [ ] Désenveloppement automatique si un mod inspecte le listener (FM-14)

### Étape G — C-41 Mod Discovery — T-440..T-442

- [ ] Inventaire des conteneurs de mods, `owner_mod_hash`, table chargeur → modid
- [ ] Indexation paresseuse : aucune classe chargée qui ne le serait pas (R-620)
- [ ] Moins de 500 ms pour 250 mods (R-621)

### Étape H — C-34 Telemetry, C-35 Diagnostics, C-38 complet

- [ ] Métriques typées à cardinalité bornée, export JSON, coût < 0,2 % du MSPT
- [ ] Aucune socket ouverte par RUSTFORGE-X (T-400)
- [ ] `/rfx why`, `/rfx top`, `/rfx mods`, `/rfx workload`, `/rfx report`
- [ ] Fiche d'explication au format normatif de la PARTIE 5.33, en moins de 50 ms
- [ ] Anonymisation des chemins dans les rapports exportables (R-571)

### Étape I — C-36 Benchmark Harness

- [ ] Micro-benchmarks Rust (criterion)
- [ ] Macro-benchmarks in-game scriptés, résultats versionnés en JSON
- [ ] Aucun chiffre publié qui ne vienne d'un fichier de résultats généré (R-580)
- [ ] Aucun benchmark sans intervalle de confiance ni nombre de répétitions (R-581)

### Definition of Done du jalon (CDC PARTIE 29.3)

- [ ] T-130..T-134, T-140..T-144, T-150..T-154, T-370..T-373, T-400..T-402,
      T-410..T-412, T-420..T-422, T-440..T-442 verts
- [ ] Overhead **mesuré** et affiché sous 2 % sur un profil de charge
- [ ] Aucun test de gameplay ne diverge de la référence non instrumentée
- [ ] JAR installable et jouable, client et serveur dédié

---

## Dette documentaire suivie

Documents exigés par la PARTIE 27.1, à rédiger au jalon où leur contenu devient réel :

| Document | Rédiger à |
|---|---|
| `ARCHITECTURE.md` | M0 (une fois C-01/C-02/C-03 en place) |
| `INSTALLATION.md` | M0 (première release) |
| `CONFIGURATION.md` | M0 (avec C-37) |
| `SECURITY.md` | M0 (modèle de menace, `unsafe`, absence de réseau) |
| `TROUBLESHOOTING.md` | M1 (avec C-35 Diagnostics) |
| `BENCHMARKS.md` | M1 (avec C-36, dès qu'un résultat existe) |
| `COMPATIBILITY.md` | M3 (avec C-33) |
| `RELEASING.md` | M0 (avec C-40) |
| `RUST_MOD_SDK.md` | M8 (avec C-39) |
| `CHANGELOG.md`, `NOTICE` | M0 (première release) |

## ADR obligatoires restant à rédiger (CDC PARTIE 27.3)

`ADR-001` Forge comme plateforme initiale · `ADR-002` Java reste l'état autoritatif ·
`ADR-003` command buffer + commit ordonné · `ADR-004` JNI + DirectByteBuffer plutôt que
Panama · `ADR-005` work stealing Chase-Lev · `ADR-006` domaines abstraits pour les
read/write sets · `ADR-007` fingerprint complet comme clé de cache · `ADR-008` RF-IR
restreint · `ADR-009` Cranelift en backend optionnel · `ADR-010` choix de licence ·
`ADR-011` apprentissage inspectable uniquement · `ADR-012` aucune télémétrie réseau.

Les ADR issus de décisions de l'agent continuent à partir d'`ADR-015` (cf. ADR-014).

---

## Points en attente d'arbitrage utilisateur

- **Licence définitive** (ADR-010) : `LICENSE` est aujourd'hui « tous droits réservés »,
  cohérent avec `mod_license`. Hors périmètre de décision de l'agent (contrat 5).
- **Remote git** : le dépôt est local, aucun remote configuré, aucun push effectué.
