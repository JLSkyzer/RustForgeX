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
- [x] Position du transformateur dans la chaîne — **tranchée, et défavorablement** :
      ADR-018 a établi que l'ordre des plugins de lancement n'est pas garantissable, et
      ADR-019 a rendu la question sans objet — un `ITransformer` passe **toujours avant**
      Mixin. La PARTIE 5.4 demandait le dernier rang ; il est inatteignable. C'est un
      écart assumé, pas une tâche en attente
- [ ] Coûts par niveau mesurés sous leur seuil (T-131) — demande le harnais C-36
- [ ] Retrait de sonde si régression JIT au-delà du seuil (R-313) — demande un
      détecteur de désoptimisation, qui n'existe pas

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
- [x] **Producteur d'échantillons de pile écrit** : `StackSampler` à 100 Hz sur le fil
      autoritatif, `StackFrameIndex` pour la correspondance trame → sonde. Il ne prélève
      pas pendant une pause de mesure (PARTIE 12.4) et dépose ses identifiants dans une
      file à producteur/consommateur uniques que le fil du tick draine — `ProbeSink` est
      attaché par fil, écrire depuis le fil d'échantillonnage aurait perdu les mesures
- [x] `profiler.max_workloads` et `profiler.cpu_budget_pct` déclarées, documentées et
      **effectivement lues** : elles traversent la frontière et construisent le
      `ProfilerConfig`, qui se bâtissait jusqu'ici sur ses défauts

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
- [x] ADR-017 rouvert et tranché : **ADR-019** remplace la voie de livraison —
      `ITransformationService` + `ITransformer` à cibles énumérées, la seule voie
      supportée depuis `mods/`, et la seule qui rende le développement identique à la
      production
- [x] ADR-019 implémenté : `RfxClassTransformer`, `TargetScanner`, `RfxLaunchPlugin`
      supprimé
- [x] **Vérifié en production** : 272 mods, 5526 cibles, instrumentation armée,
      3462 méthodes sondées, aucun mod cassé, 20,001 TPS
- [x] **Classes des mods ciblées** : deux filtres, sur le contenu des classes et non
      sur leur nom — le code qui référence le client, et les classes que Mixin patche
      (classes mixin elles-mêmes et cibles déclarées par `@Mixin`, lues par ASM, T-135)
- [x] **Vérifié en production** : 78 207 cibles (852 laissées à Mixin sur 1499
      déclarées), 272 mods chargés, 0 échec, **17 546 méthodes sondées**, 19,998 TPS
- [x] **Coût mesuré par C-36**, campagne conforme PARTIE 21.3 (5 × 12 000 ticks × 2
      configurations, commit `784d8d9`) : MSPT p95 2,523 → 3,372 ms, soit **+33,7 %** ;
      p50 +40,8 % ; TPS inchangé à 20,000 ; démarrage 110 → 118 s
- [ ] **Le budget de 2 % (H-07, acceptance C-05) n'est pas tenu** : +33,7 % sur le p95.
      En absolu 0,85 ms sur 50 ms, donc le jeu ne souffre pas sur cette machine — mais le
      budget est normatif, et il est dépassé d'un ordre de grandeur
- [x] **`OverheadMeter` ne mesurait pas la dépense dominante** — corrigé : la ligne de
      base de la PARTIE 12.4 mesure le tick entier, donc aussi les `RfxProbes.enter` et
      `exit` injectés. Les deux instruments convergent (32,3 % contre 33,7 %)
- [x] **Auto-mesure de la PARTIE 12.4 implémentée** : `BaselineSampler`, pause de
      20 ticks toutes les 6000, comparaison de deux médianes adjacentes, verdict
      immédiat (ADR-020). Le statut distingue « pas encore mesuré » de « ne coûte rien »
      (R-770). 8 tests T-140 côté Rust, 2 côté Java
- [x] **Vérifié en jeu** : mesure au tick 6020 d'un serveur de production, 446 750 ns
      par tick, soit **32,3 %** du tick. La campagne C-36, mesurée de l'extérieur,
      donnait **+33,7 %** sur le p95. Les deux instruments convergent
- [x] **Les 180 lignes `FATAL` sont supprimées** : une classe référençant un type absent
      de l'installation n'est plus désignée comme cible (T-136). Vérifié en production :
      0 `FATAL`, 0 mention de `squaremap`, 0 échec de mod, instrumentation armée.
      Coût : 2015 classes écartées, 16 566 méthodes sondées au lieu de 17 543, et une
      seconde d'énumération de plus
- [ ] Réduire encore : 76 202 classes restent déclarées pour une fraction réellement
      sondée, chacune décodée et réécrite pour rien. Évaluer l'éligibilité au moment de
      l'énumération supposerait d'analyser le code de chaque classe au démarrage — à
      mesurer avant de le décider (ADR-019)
- [x] **La profondeur ne remonte plus avant la première ligne de base** (ADR-020,
      deuxième volet) : le profiler atteignait `DEEP` en 25 s sur la foi de compteurs
      aveugles à sa dépense dominante. Vérifié en jeu : `LIGHT` au lieu de `DEEP`
- [ ] Échelons 2 et 3 de la PARTIE 12.4 — « suspendre les nouvelles décisions », puis
      `DEGRADED` : rien à suspendre tant que le moteur de décision n'existe pas (ADR-020)
- [ ] Troisième configuration — transformateur présent, sondes jamais posées — pour
      séparer le coût de la transformation de celui des sondes. Écrite après la
      campagne : `run-macro-prod.sh` est lu au fil de l'eau par bash pendant l'exécution,
      l'éditer en cours de campagne la corromprait. Demande aussi que l'agrégateur
      compare chaque configuration à la référence, et non les deux premières
- [x] Dette d'ADR-018 close **par construction** : une méthode ne peut plus porter un
      mixin et une sonde, puisque les cibles de mixins sont exclues. L'hypothèse
      d'ADR-018 était d'ailleurs fausse — un `@ModifyVariable` l'a réfutée
- [ ] Cibles atteintes **indirectement** par un mixin (héritage, configuration
      construite à l'exécution) : aucune constatée, aucune écartée
- [x] Ordre relatif des `ILaunchPluginService` — tranché par ADR-018 : non
      garantissable sur ModLauncher 10, l'injection est rendue indifférente à l'ordre
- [x] Suppression de la configuration Mixin morte (zéro mixin déclaré, un avertissement
      à chaque lancement)
- [x] Instrumentation exposée dans `/rfx status` : état, classes vues, méthodes
      sondées, et les échecs de transformation quand il y en a
- [x] C-40 : `artifactVerify` contrôle désormais **les deux** artefacts — entrées
      autorisées, absence de classe de test, entrées obligatoires du JAR de lancement,
      empreintes des natifs — et écrit `SHA256SUMS` (PARTIE 25.2)
- [ ] Reste de C-40 : version du manifeste égale au tag, plage de taille attendue,
      licences des dépendances dans NOTICE (PARTIE 25.4)

### Étape F — C-06 Event Observer — T-150..T-154

Fiche de conception : `docs/design/C-06-event-observer.md`, écrite depuis l'API réelle
d'`eventbus 6.2.33` et non depuis une supposition.

- [ ] **D'abord** : `net.minecraftforge.eventbus` est-il ouvert à notre module ? Une
      ligne au `LOAD_COMPLETE`. La réponse décide de l'étendue des étapes 2 et 3
- [ ] Étape 1 — observer sans rien modifier : auditeurs sur `Event.class` en `HIGHEST` et
      `LOWEST`, API publique seule, aucun risque R-330. Donne les types postés, la durée
      de dispatch, l'annulation et le résultat finaux, la trace d'ordre
- [ ] Mesurer le coût de cet auditeur par la ligne de base de la PARTIE 12.4 avant
      d'aller plus loin : il s'exécute des milliers de fois par tick
- [ ] Étape 2 — reconstituer ordre, priorité et `owner` par lecture réflexive
      (`EventBus.busID`, `ListenerList.getListeners`) ; aucune écriture
- [ ] Étape 3 — envelopper, et seulement si l'attribution de l'annulation à un handler
      précis se révèle nécessaire. Traiter une priorité entière d'un coup : `register`
      ajoute en queue, donc un remplacement un par un violerait R-330
- [ ] Désenveloppement automatique si un mod inspecte le listener (FM-14)
- [ ] T-154 exige un surcoût **mesuré** sous 30 ns : suppose un harnais JMH, même dette
      que T-131

> Règle, pas tâche : ne pas employer `net.minecraftforge.unsafe` pour contourner
> l'encapsulation d'`eventbus`. Si le module est fermé, on écrit l'ADR et on s'en tient
> à l'API publique.

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
- [x] **Campagne normative exécutée** le 2026-09-05 sur `784d8d9` : 5 exécutions ×
      12 000 ticks × 2 configurations, conforme et déclarée fiable par l'agrégateur.
      Résultat dans `benchmarks/results/macro-1788623402-784d8d9.json`
- [ ] Seconde campagne sur `38eacfa`, après le filtre des types absents et l'auto-mesure
      de la 12.4, pour mesurer ce que ces deux correctifs changent
- [ ] Profils de modpack **reproductibles** de la PARTIE 22. La mesure actuelle porte
      bien sur un profil chargé — 272 mods — mais c'est le modpack personnel de
      l'utilisateur, non versionnable et non reproductible par un tiers. Il manque aussi
      le monde pré-généré du point 2 de la PARTIE 21.3 : le monde actuel est réutilisé
      d'une campagne à l'autre et accumule ses régions, ce qui fait dériver les valeurs
      absolues de MSPT entre deux campagnes
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
