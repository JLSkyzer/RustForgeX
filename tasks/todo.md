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

- [x] `rfx_probe_buffer_acquire` / `rfx_probe_buffer_flush`
- [x] `ProbeRecord` de 32 octets, little-endian, aligné 8
- [x] Le buffer appartient au natif ; Java ne le libère jamais (R-708)
- [x] Un flush n'alloue pas et ne bloque pas ; saturation ⇒ perte comptée (R-709)
- [x] Une seule traversée FFI par tick et par thread

### Étape D — C-04 JVM Instrumentation — T-130..T-134

- [x] `enter`/`exit` sous `try/finally` : une exception ne fausse jamais un compteur
- [x] Niveaux `COUNTER` / `TIMED` / `DEEP`, pilotés à l'exécution (ADR-016)
- [x] Refus de sonder : méthodes trop courtes, `<clinit>`, natives, classes du bootstrap (R-311, R-312)
- [x] Sémantique observable inchangée (R-310), vérifiée en exécutant les classes transformées
- [ ] Transformateur enregistré en dernier dans la chaîne ModLauncher
- [ ] Coûts par niveau mesurés sous leur seuil (T-131) — demande le harnais C-36
- [ ] Retrait de sonde si régression JIT au-delà du seuil (R-313) — demande C-05

### Étape E — C-05 Profiler — T-140..T-144

- [x] Agrégation côté Rust : EWMA, histogrammes, chaleur (DM-04)
- [x] Adaptation `OFF → LIGHT → NORMAL → DEEP → THROTTLED`
- [x] Auto-mesure de l'overhead, réduction de niveau au dépassement, `E-1201`
- [x] Aucune allocation dans le chemin chaud (R-320, T-142) — prouvé par allocateur
      compteur, pas par relecture du code
- [x] Éviction LRU au-delà de `max_workloads` (R-321)
- [x] Fonctionne sans aucune sonde : le temps attribué par échantillonnage prend le
      relais du temps mesuré (R-322)
- [x] Points d'entrée `rfx_workload_register`, `rfx_probe_levels`, `rfx_profiler_start`
- [x] `/rfx status` expose niveau, unités suivies, coût mesuré et anomalies
- [ ] Le producteur d'échantillons de pile (thread à 100 Hz) reste à écrire côté Java :
      l'agrégation les accepte déjà, personne n'en produit encore
- [ ] `profiler.max_workloads` et `profiler.cpu_budget_pct` dans `rustforgex.toml` —
      le code lit aujourd'hui les valeurs par défaut du cahier des charges

### Étape E bis — brancher les sondes (reste de C-04)

C-05 sait désormais attribuer un identifiant de sonde et piloter les niveaux : la
condition posée pour brancher le transformateur est levée.

- [x] Voie d'enregistrement établie et consignée : ADR-017 — `ILaunchPluginService`
      dans un second JAR promu à la couche de plugins de ModLauncher
- [x] Jeu de sources `src/launch/` produisant `rustforgex-launch.jar`, module nommé
      (`module-info.java`) — un `META-INF/services` seul reste invisible dans la couche
      d'amorçage
- [x] `RfxLaunchPlugin` : inerte tant que le mod ne l'a pas armé (couche parente)
- [x] `ProbeRegistry` côté Java : décrire l'unité de travail, obtenir son identifiant
      du natif (`rfx_workload_register`)
- [x] `ModOwnerResolver` : attribution d'une classe à son mod par la liste de Forge,
      sans aucun nom de mod en dur (INV-12)
- [x] Détecter et signaler l'absence du JAR de lancement, **et** le cas où il est
      présent sans avoir été installé par ModLauncher — armer un plugin jamais
      instancié réussit sans rien produire
- [x] Rafraîchir la table des niveaux en fin de tick (`rfx_probe_levels`,
      `RfxProbes.setLevels`)
- [x] Faire parvenir le JAR à la couche d'amorçage en développement :
      `minecraftArtifacts.from(launchJar)` sur les tâches de run — c'est de cette
      collection, et non de `runtimeClasspathArtifacts`, que ForgeGradle compose le
      fichier lu par BootstrapLauncher
- [x] **Vérifié en jeu** : 1800 ticks à 20 TPS, 1366 classes vues, 3679 méthodes
      sondées, 0 échec, 11 mises à jour de niveaux
- [x] Vérifié en production — et **ADR-017 ne fonctionne pas** : `LaunchPluginHandler`
      construit sa liste 2,5 s avant que notre `ITransformationService` ne soit
      découvert depuis `mods/`. Un `ILaunchPluginService` livré dans `mods/` n'est
      jamais installé sur Forge 47. La garde `PLUGIN_NOT_INSTALLED` l'a détecté
- [ ] **BLOQUANT** : rouvrir ADR-017 sur la voie de livraison du transformateur. En
      l'état, l'instrumentation ne sonde rien chez un joueur. Le développement ne
      fonctionne que parce que `minecraftArtifacts` place le JAR sur le classpath
      d'amorçage, ce qu'un joueur ne peut pas faire
- [x] Ordre relatif des `ILaunchPluginService` — tranché par ADR-018 : non
      garantissable sur ModLauncher 10, l'injection est rendue indifférente à l'ordre
- [x] Suppression de la configuration Mixin morte (zéro mixin déclaré, un avertissement
      à chaque lancement)
- [ ] Confirmer par un essai réel qu'une méthode portant un mixin **et** une sonde se
      comporte identiquement dans les deux ordres (hypothèse la plus fragile d'ADR-018)
- [ ] Exposer l'instrumentation dans `/rfx status` (aujourd'hui seulement au journal)
- [ ] C-40 : deux artefacts à construire, empreindre et vérifier
- [ ] Retrait d'une sonde sur régression JIT (R-313)

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

**Niveau A — micro-benchmarks Rust** (autonome, mesure ce qui existe aujourd'hui)

- [x] Crate `crates/rfx-bench` avec criterion, hors de la chaîne de dépendances du
      runtime (INV-13 ne le concerne pas : rien ne dépend de lui)
- [x] B-10 : calcul du `WorkId` (hachage d'empreinte)
- [x] Chemin chaud du profiler : agrégation d'un enregistrement, clôture d'un tick à
      N unités, histogramme et quantiles
- [x] C-31 : réservation et libération de budget, vidage d'un tampon de sonde
- [x] Sérialisation CBOR du statut, aller et retour
- [x] Collecteur : convertit les estimations de criterion en
      `benchmarks/results/*.json` au schéma normatif (R-861), médiane et IQR, jamais
      la moyenne seule, avec matériel (C-45), commit et date
- [x] `benchmarks/README.md` : R-580, et comment relire un résultat
- [x] CI : les benchmarks sont compilés, jamais exécutés sur une machine partagée

**Niveau B — macro-benchmarks en jeu**

- [x] `MacroRecorder` : mesure du temps de tick, armé par propriété système, absent
      sinon, et indépendant du runtime — il mesure aussi bien RFX éteint qu'allumé
- [x] Ticks contaminés par une collecte mémoire marqués et comptés, jamais supprimés
      (PARTIE 21.3, point 8)
- [x] `benchmarks/run-macro.sh` : une exécution = un processus distinct (point 5)
- [x] Deux configurations comparées, `a-rfx-off` et `b-rfx-on`
- [x] Agrégateur `rfx-bench macro` : médiane et IQR **inter-exécutions**, conformité
      calculée et non supposée, rejet au-delà de 10 % de dispersion (point 7)
- [x] `comparison.trustworthy` : un écart entre deux campagnes douteuses n'est pas une
      mesure
- [ ] **Campagne normative** : 5 exécutions × 12 000 ticks × 2 configurations, plus de
      deux heures. Non lancée à ce jour
- [ ] Profils de modpack de la PARTIE 22 — la mesure actuelle décrit un serveur nu,
      sans aucun mod tiers, et l'acceptation « overhead < 2 % » porte sur un profil
      chargé
- [ ] Côté client : FPS, frametime, 1 % low

**Ce que le niveau A ne mesure pas**, et qu'il ne faut pas lui faire dire : le coût
Java de `RfxProbes.enter/exit` sur le chemin chaud. Un chronométrage Java naïf mentirait
(élimination de code mort), et JMH est une dépendance à peser séparément. T-131 reste
donc ouvert.

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
