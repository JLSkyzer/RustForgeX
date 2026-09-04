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

### Étape A — Fondations du workspace Rust (C-27 squelette) — T-001

- [ ] `Cargo.toml` (workspace) + `rust-toolchain.toml` (toolchain épinglée)
- [ ] `crates/rfx-core` : `RuntimeState`, codes d'erreur `E-xxxx` (annexe A.2), `Runtime` (R-520/R-521)
- [ ] `crates/rfx-model` : DM-14 `HardwareClass`, structure de configuration, sérialisation CBOR (R-704)
- [ ] `crates/rfx-ffi` : `cdylib` `rfx_native`, `rfx_abi_version` / `rfx_init` / `rfx_shutdown` (IF-01)
- [ ] `panic = "unwind"` obligatoire (R-522) + `catch_unwind` sur **chaque** point d'entrée (R-523)
- [ ] Handle opaque validé par génération, jamais de pointeur brut (R-521)

### Étape B — Build natif intégré à Gradle (C-40 partiel) — T-002

- [ ] Tâches `buildNative` / `copyNative` / `hashNative` dans `build.gradle` (CDC 23.2)
- [ ] Sélection de cible par plateforme hôte, `.sha256` généré à côté du binaire
- [ ] `processResources` dépend de `hashNative` ; vérifier le JAR produit

### Étape C — C-03 Native Loader — T-120..T-123

- [ ] Extraction vers `<gameDir>/rustforgex/native/<sha256>/` (R-301, idempotent)
- [ ] Vérification SHA-256 **avant** chargement, refus sinon (R-300, `E-1003`)
- [ ] Repli `java.io.tmpdir` si `noexec` ou lecture seule (R-302)
- [ ] Écriture atomique (fichier temporaire puis renommage)

### Étape D — C-37 Configuration — T-007

- [ ] Schéma complet de la PARTIE 28.2 : défaut, plage et description pour **chaque** clé (R-590)
- [ ] Lecture/écriture de `<gameDir>/rustforgex/config/rustforgex.toml`, créé au premier lancement
- [ ] Clé inconnue conservée et signalée, jamais supprimée (R-591)
- [ ] Valeur hors plage rejetée avec message précis et remplacée par le défaut (28.5)
- [ ] Surcharges `-Drustforgex.<section>.<clé>` prioritaires sur le fichier (28.4)
- [ ] Clés structurelles marquées « redémarrage requis » (R-592)

### Étape E — C-02 Bootstrap — T-110..T-114

- [ ] Machine à états `INIT → PROBE → LOAD_NATIVE → HANDSHAKE → CONFIGURE → READY | DEGRADED | DISABLED`
- [ ] Séquence normative en 10 étapes (5.2), aucune exception non capturée ne remonte
- [ ] Handshake ABI : `rfx_abi_version()` avant tout autre appel (R-702), écart ⇒ `DISABLED` + `E-1002`
- [ ] `DEGRADED` parfaitement jouable : aucune instrumentation, aucun thread supplémentaire
- [ ] Double initialisation impossible (T-114, `E-1004`)

### Étape F — C-45 Hardware Probe — T-480..T-482

- [ ] Topologie (cœurs physiques/logiques), capacités SIMD, mémoire totale
- [ ] Coût d'un aller-retour FFI mesuré sur 10 000 appels, débit de copie Java→natif
- [ ] Durée totale de sonde < 150 ms (T-482), stabilité inter-exécutions < 20 % (T-480)
- [ ] Aucune constante de coût codée en dur (R-660) ; tout champ non sondable est déclaré comme tel

### Étape G — C-01 Forge Integration — T-100..T-103

- [ ] `PlatformAdapter` (IF-10) : isole tout le reste du système de l'API Forge (P-11)
- [ ] Attache aux deux bus, priorité HIGHEST en PRE et LOWEST en POST
- [ ] `try/catch` obligatoire autour de chaque hook, désactivation après 5 échecs (FM-02)
- [ ] Version de Forge hors plage ⇒ `OBSERVE_ONLY` + `E-1001` (FM-01)
- [ ] Tick sans POST ⇒ fermeture implicite au `tick_begin` suivant (R-706, FM-03)
- [ ] Aucun hook au-dessus de 50 µs en observation seule (critère d'acceptation)

### Étape H — `/rfx status` (C-38 partiel) — T-420..T-422

- [ ] Commande enregistrée via `RegisterCommandsEvent`, permission niveau 3 sur serveur (R-602)
- [ ] Affiche état du runtime, mode, état de boot, classe matérielle, maturité des composants
- [ ] Ne bloque pas le thread serveur plus de 5 ms (R-601)

### Étape I — Qualité, CI et documentation (C-40) — T-003..T-010

- [ ] Tests de fondations T-003 (lints), T-006 (aucune fiction en STABLE), T-007, T-008, T-009, T-010
- [ ] CI : `fmt`, `lint`, `lint-no-fiction`, `build`, `test-unit`, `artifact-verify` (CDC 24.2)
- [ ] `ARCHITECTURE.md`, `CONFIGURATION.md`, `SECURITY.md`, `INSTALLATION.md`, `RELEASING.md`, `CHANGELOG.md`
- [ ] ADR des décisions prises pendant le jalon (à partir d'ADR-015)

### Definition of Done du jalon (CDC PARTIE 29.3)

- [ ] Tests **T-001..T-010**, **T-100..T-103**, **T-110..T-114**, **T-120..T-123**, **T-480..T-482** verts
- [ ] Chaque composant expose ses métriques et son niveau de maturité (contrat 4.1)
- [ ] Chaque chemin natif a un fallback Java testé (contrat 4.2)
- [ ] JAR installable et jouable, client **et** serveur dédié (contrat 4.10)
- [ ] Retirer le JAR laisse le monde parfaitement jouable
- [ ] Aucun chiffre de performance qui ne vienne d'une mesure (contrat 3.2)

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
