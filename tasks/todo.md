# TODO — RUSTFORGE-X

**Jalon courant : M0 (Bootstrap) — non démarré.**
Source de vérité : `docs/spec/RUSTFORGE-X_Cahier_des_Charges_v1.0.md`.

---

## Étape 0 — Mise en place de l'espace de travail ✅ TERMINÉ (2026-09-04)

- [x] Lire le cahier des charges et en extraire l'architecture, les jalons et le contrat agent
- [x] Confronter le MDK Forge généré au CDC et relever les divergences structurantes
- [x] Arbitrer les divergences avec l'utilisateur (package, layout, versioning)
- [x] Migrer le package `fr.eriniumgroup.rustforgex` vers `dev.rustforgex` (CDC 3.6 / 3.7)
- [x] Supprimer le code d'exemple du MDK (`example_block`, `example_item`, `Config` de démo)
- [x] Corriger `rustforgex.mixins.json` (package `dev.rustforgex.mixin`, niveau `JAVA_17`)
- [x] Renseigner `mod_group_id` et `mod_description` dans `gradle.properties`
- [x] Créer `tasks/lessons.md` (interdits du contrat agent + leçons de session)
- [x] Créer `tasks/todo.md` (ce fichier)
- [x] Versionner la copie du CDC dans `docs/spec/`
- [x] Écrire `docs/AGENT.md` (reprise de projet sans contexte, PARTIE 27.2)
- [x] Écrire ADR-013 (layout du dépôt) et ADR-014 (numérotation des ADR)
- [x] Écrire `README.md`, `BUILDING.md`, `LICENSE`
- [x] Écrire `.gitignore`, initialiser le dépôt git, commit initial
- [x] Vérifier que `./gradlew compileJava` passe sur le squelette migré

---

## Étape 1 — M0 : Bootstrap

**Livrable** : un JAR Forge qui se charge, charge la bibliothèque native, sonde le
matériel, expose `/rfx status`, et **ne fait rien d'autre**. Le jeu doit tourner
exactement comme sans le mod, overhead mesuré proche de zéro.

**Lire avant de coder** : fiches C-01 (5.1), C-02 (5.2), C-03 (5.3), C-27 (5.27),
C-37 (5.35), C-45 (5.43), C-40 (5.38 / PARTIE 24-25).

### 1.1 Fondations du workspace Rust

- [ ] Créer `Cargo.toml` (workspace) et `rust-toolchain.toml` (version épinglée)
- [ ] Créer les crates du jalon : `rfx-core`, `rfx-ffi`, `rfx-model` (squelettes compilables)
- [ ] Brancher `:buildNative` / `:copyNative` / `:hashNative` dans `build.gradle` (CDC 23.2)
- [ ] Vérifier que `./gradlew build` produit un JAR contenant `natives/<os>-<arch>/`

### 1.2 Composants

- [ ] **C-01** Forge Integration — abstraction du loader, points d'accroche du cycle de vie
- [ ] **C-02** Bootstrap — séquence de démarrage, ordre d'initialisation, mode dégradé
- [ ] **C-03** Native Loader — extraction et chargement du natif, vérification `.sha256`, fallback JAVA_ONLY si absent
- [ ] **C-27** Rust Runtime Core — squelette : types communs, erreurs, aucune panic à travers la FFI
- [ ] **C-37** Configuration — `<gameDir>/rustforgex/config/rustforgex.toml`, valeurs par défaut, validation (CDC PARTIE 28)
- [ ] **C-45** Hardware Probe — détection matérielle, `HardwareClass` (DM-14)
- [ ] **C-40** Release System — build reproductible, CI, job `lint-no-fiction` (CDC PARTIES 23-25)
- [ ] `/rfx status` minimal (C-38 partiel) exposant état du runtime et maturité des composants

### 1.3 Definition of Done du jalon (CDC PARTIE 29)

- [ ] Tests **T-001..T-010** (fondations) verts
- [ ] Tests **T-100..T-103** (C-01), **T-110..T-114** (C-02/C-03), **T-120..T-123** verts
- [ ] Tests **T-480..T-482** verts
- [ ] Chaque composant expose ses métriques et son niveau de maturité (contrat 4.1)
- [ ] Chaque chemin natif a un fallback Java testé (contrat 4.2)
- [ ] JAR installable et jouable, client **et** serveur dédié (contrat 4.10)
- [ ] Retirer le JAR laisse le monde parfaitement jouable
- [ ] Overhead mesuré par le harnais, jamais estimé (contrat 3.2)

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
