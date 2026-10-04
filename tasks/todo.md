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
- [x] **Le dépassement d'un ordre de grandeur est résorbé** : +36 % est devenu +6,6 %
      après ADR-021. Ce qui reste est un écart d'un facteur quatre sur le budget de MSPT,
      et une marge d'un facteur six sur celui de cœur — voir la ligne ci-dessus
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
- [ ] **Le bruit de la ligne de base est MESURÉ, et la méthode ne peut pas marcher.**
      Relevé du 2026-09-07, 30 cycles, charge `mobs`, sondes armées, profileur `DEEP` :

      | | |
      |---|---|
      | différence minimale par cycle | **−1 527 250 ns** |
      | différence maximale par cycle | **+1 257 700 ns** |
      | cycles positifs | **14 sur 30** |
      | médiane retenue | 0 ns |

      Le signal cherché vaut ~250 000 ns. Le bruit par cycle vaut ±1 500 000 ns, soit
      **six fois le signal**, et le signe est celui d'un tirage à pile ou face — 14 sur
      30 quand le hasard pur en donnerait 15. Or le profilage ne peut pas rendre un tick
      plus rapide : la moitié des cycles rend une valeur physiquement impossible.
      Ce n'est donc pas de la dérive entre cycles, c'est du bruit **intra-cycle**.
- [ ] Conséquence chiffrée : pour résoudre 250 µs dans un bruit d'écart-type ~1,5 ms, il
      faudrait environ **350 cycles**, soit 112 000 ticks — une heure et demie de jeu par
      mesure. La cadence 300/20 de la PARTIE 12.4 ne peut pas y arriver, quel que soit
      le nombre de cycles raisonnable
- [ ] Piste à instruire demain : **apparier des ticks adjacents** plutôt que deux blocs
      de vingt séparés d'une seconde. Deux ticks consécutifs se ressemblent bien plus
      que deux blocs distants ; l'appariement annule la part lente de la variance, qui
      est probablement l'essentiel. Coût à évaluer : basculer le niveau des sondes à
      chaque tick suppose de retransmettre la table, ce qui coûte et fausserait la
      mesure. À mesurer avant de décider
- [ ] **CASE ROUVERTE — les zéros de la ligne de base ne sont PAS expliqués.** Cochée
      le 2026-09-07 sur la foi d'**une seule exécution**, décochée deux heures plus tard
      par la suivante. Les sondes mortes étaient *une* cause, pas *la* cause :

      | exécution | ligne de base | niveau final |
      |---|---|---|
      | `c-rfx-actif` nº1 | 3,01 % | `OFF` |
      | `c-rfx-actif` nº2 | **0,0 %** | **`DEEP`** |

      Deux exécutions de la même configuration, deux régimes opposés. La méthode de
      pause reste instable — c'était déjà le diagnostic d'ADR-020, quatrième volet, et
      il tient toujours. Vingt ticks en pause contre vingt ticks actifs ne suffisent pas
      à extraire un signal d'une charge qui varie de plusieurs millisecondes
- [ ] **Ne pas publier de médiane sur cette campagne sans dire ce qu'elle mélange.**
      Certaines exécutions finiront à `OFF`, d'autres à `DEEP` : la médiane agrégerait
      deux régimes de fonctionnement différents, et le chiffre n'aurait pas de sens
      physique
- [x] **Le gouverneur s'engage quand la ligne de base voit juste.** Au tick 9 600, une
      mesure à 3,01 % dépasse le budget : le profileur descend jusqu'à `OFF` et désarme
      les 2 541 sondes. Le mécanisme fonctionne ; ce qui l'alimente, non — voir la case
      rouverte ci-dessus
- [ ] **Conséquence à assumer : une campagne mesure désormais un mélange.** Les 12 000
      ticks couvrent une période sondée puis une période éteinte. Le p50 qui en sort
      n'est donc pas « le coût de l'instrumentation » mais celui du produit tel qu'il se
      comporte. Les deux chiffres sont utiles, ils ne répondent pas à la même question —
      à séparer explicitement dans le prochain ADR
- [ ] **L'armement anticipé mesure un coût PLUS FAIBLE que le tardif** (2,43 % contre
      3,01 %) alors qu'il sonde cinq fois plus de méthodes. À relativiser : la mesure
      qui les sépare est celle dont l'instabilité vient d'être constatée. Explication possible : il
      atteint le budget plus tôt, donc le gouverneur l'éteint plus tôt, donc la fenêtre
      de pause tombe sur un système déjà moins actif — ce serait un artefact de mesure,
      pas une propriété. **Une seule exécution** ; à confirmer sur les cinq
- [ ] Texte d'origine, conservé parce qu'il a orienté trois ADR : « la mise en pause de
      la PARTIE 12.4 ne suffit pas à gouverner. Cinq mesures sur
      la campagne : 13,3 % · 0,0 % · 0,5 % · 0,0 % · 0,0 %, quand la comparaison de
      configurations donne un écart stable de +36,4 %. Elle a vu juste une fois sur cinq.
      Un zéro est la borne, pas un coût nul : la fenêtre en pause a duré plus longtemps
      que la fenêtre active, la charge ayant bougé. Vingt ticks contre vingt ne sortent
      pas 0,9 ms d'un signal qui varie de plusieurs millisecondes. Conséquence : le
      profiler reste à `DEEP` en permanence et le gouverneur ne se déclenche jamais.
      Pistes dans ADR-020, quatrième volet — à départager par une campagne, pas par un
      raisonnement
- [x] **Piste 1 implémentée et vérifiée en jeu** : cadence 300/20 au lieu de 6000/20,
      médiane de 30 différences **signées**, borne à zéro appliquée une seule fois à la
      fin. Résultat sur serveur de production : 5,86 % sur 30 cycles, et
      `profiler_level: THROTTLED` — **le gouverneur d'overhead s'est déclenché pour la
      première fois**
- [x] **Campagne sur `5c9b24a`** : conforme, fiable, la moins dispersée des trois
      (écart-type inter-exécutions sous 4 % partout). MSPT p95 **+36,1 %**, contre
      +36,4 % et +33,7 %. **Trois campagnes, le même chiffre : le coût est stable et rien
      de ce qui a été fait ne l'a réduit**
- [ ] **Ce constat portait sur des sondes mortes et doit être refait.** « Le gouverneur
      ne peut pas ramener le coût sous 2 % » a été établi en comparant `THROTTLED` à
      `DEEP` — mais dans les deux cas aucune sonde ne mesurait, donc les deux états
      coûtaient la même chose parce qu'ils *faisaient* la même chose : rien.
      Le 2026-09-07, avec des sondes vivantes, le gouverneur descend jusqu'à `OFF` et
      désarme les 2 541 sondes ; reste à voir ce que ça change au MSPT.
      Texte d'origine conservé ci-dessous pour mémoire :
      L'exécution descendue à `THROTTLED` — aucune sonde armée — donne un p95 de
      3,186 ms, indistinguable des 3,181 à 3,268 ms des exécutions restées à `DEEP`.
      La dépense n'est pas ce que les sondes font, c'est qu'elles existent. **Le seul
      levier est le nombre de méthodes instrumentées**
- [x] **Seuil de sondage porté de 12 à 64 instructions** (ADR-021). R-311 est un
      plancher, pas une politique. Paire B/C courte sur le serveur de production :
      **16 544 → 2 598 méthodes sondées**, et l'écart p95 passe d'environ **+36 % à
      +2,7 %**. Le chiffre exact n'est pas établi — une seule paire, non conforme
- [x] **Campagne conforme sur le nouveau seuil** (`fbf659f`, 2026-09-06) : MSPT p95
      2,409 → 2,569 ms, soit **+6,6 %**, contre +36,1 % avant. Surcoût divisé par 5,5,
      dispersion sous 4,3 %, TPS inchangé
- [ ] **Trancher la référence du budget.** H-07 borne à « 2 % d'un cœur » : mesuré
      **0,32 %**, tenu avec un facteur six de marge. Le budget de MSPT, celui
      qu'`OverheadMeter` applique, vaut 1,5 % du tick : à +6,6 %, il ne l'est pas. La
      Definition of Done dit « sous 2 % » sans préciser la référence. **Cette ambiguïté
      doit être levée par une décision explicite, pas par le choix du chiffre le plus
      flatteur**
- [ ] R-311 prévoit une exception — sonder quand même une méthode courte si
      `calls_per_tick` est très élevé et le niveau `COUNTER`. **Non implémentée** : il
      faudrait savoir qu'une méthode est très appelée avant de la sonder, donc une
      seconde passe de transformation
- [x] **Question caduque : l'échantillonnage ne compensait pas 84 % des méthodes, il
      était la SEULE source de mesure.** Aucune sonde n'a jamais rapporté avant le
      2026-09-07 : `RfxProbes.install` n'était appelé que par les tests, et une table de
      niveaux vide se coinçait dans le cache natif. À reformuler une fois que les sondes
      auront tourné une campagne entière
- [x] **Seuil rendu configurable et balayé** (ADR-027).
      `-Drustforgex.instrumentation.min_instructions=N`, ramené au plancher de R-311.
      Par propriété système et non par le fichier : le transformateur est enregistré
      avant que le répertoire de jeu soit connu

      | | seuil 64 | seuil 12 |
      |---|---|---|
      | méthodes sondées | 2 580 | 16 729 |
      | coût | sous détection | **14,58 % du MSPT** |
      | cycles positifs | 5/10 | **9/10** |
      | TPS | 19,17 | 16,87 |

      **ADR-021 est confirmé, et son mécanisme chiffré** : les méthodes que le seuil
      écarte coûtent **412 ns par sonde et par tick** contre **104 ns** pour celles
      qu'il garde — quatre fois plus, parce qu'elles sont bien plus appelées
- [x] Recensement des refus ajouté : le seuil écarte **14 113 méthodes** que R-311
      autoriserait, soit 84,5 % de cette population. ADR-021 avait été écrit sans que
      ce chiffre existe
- [ ] **On ne peut pas acheter de la couverture en armant plus de sondes.** Trois
      tentatives, même conclusion : armement anticipé (×5,1 sondes, part expliquée
      0,3 → 0,4 %), seuil à 12 (×6,5 sondes, 14,58 % du MSPT). Le budget finance
      ~2 850 sondes, et c'est ce qu'on arme déjà
- [x] **Découverte des trames inconnues livrée** (ADR-028). `UnknownFrameIndex`,
      `/rfx discover [n]`, `/rfx discover reset`, `rfx.discovery.unknown_frame_ratio`,
      bloc `discovery` du fichier de campagne. Coût vérifié en plan équilibré
      `sans/avec/avec/sans` : à position égale, **−0,31 ms et −2,95 ms** — aucun coût
      mesurable
- [x] **Le trou de couverture n'était pas un défaut de couverture, mais de collecte.**
      `ProbeSink.flush()` vide le tampon **du thread qui l'appelle**, et n'est appelée
      qu'à la clôture du tick, donc par le seul fil autoritatif (`TickCycle:147`, site
      d'appel unique). Sur 8 900 ticks : **1 thread vide, 17 ne vident pas**,
      **151 959 516 passages** jamais remis et **39 914 830 enregistrements** perdus —
      dimthread (un thread par dimension), `Worker-Main`, `Physics thread` de
      Valkyrien Skies. Silencieusement, dans les deux cas
### Vidage multi-thread — plan (ADR-029)

- [x] **Le fil autoritatif ne videra PAS les tampons des autres.** Lire le tampon d'un
      thread qui écrit dedans demanderait un tampon double ou un anneau, donc de la
      synchronisation dans le chemin chaud. Chaque thread videra **le sien**
- [x] **Déclencheur : une époque de tick.** `ProbeSink` publie un `long volatile`
      incrémenté à la clôture de chaque tick. Un thread sondé compare son époque à
      celle-là au début de chaque appel — une lecture, une comparaison — et se vide
      lui-même quand elle a changé. Aucune course : chaque thread ne touche que son
      propre état
- [x] **Corriger la cause des 40 M d'enregistrements perdus** : `probeCapacity` grandit,
      mais un thread attaché tôt garde ses petits tableaux à vie, et tout identifiant
      au-delà retombe sur l'écriture directe qui sature le tampon. Les réallouer à la
      clôture d'époque — une allocation par tick au plus, à un point contrôlé, jamais
      dans le chemin d'appel (R-320)
- [x] **Tampon plein : vider au lieu de perdre.** Le nombre de traversées devient
      proportionnel au volume, non au nombre de threads. Le comptage étant groupé, un
      tampon de 2 048 enregistrements couvre largement un tick (R-700)
- [x] **Vérifié en jeu** (ADR-029), plan équilibré, 4 exécutions de 8 000 ticks :

      | | sans | avec |
      |---|---|---|
      | threads qui vident | 5 / 19 | **19 / 19** |
      | passages en attente | 193 092 269 | **21 989** |
      | enregistrements perdus | 47 820 126 | **0** |

      Coût apparié par position : **−2,25 ms** et **−9,00 ms** — aucun surcoût mesurable.
      Les 21 989 restants sont la valeur d'un tick en cours : on ne descend pas plus bas
- [x] **Couverture mesurée** (ADR-030), deux exécutions de 5 000 ticks sous `medium` :

      | | sans | avec |
      |---|---|---|
      | threads qui vident | 1 / 17 | **18 / 18** |
      | enregistrements perdus | 37 003 714 | **0** |
      | unités observées | 13 | **60** |
      | **appels par tick** | **289,1** | **29 578,8** |

      Le profileur voit **cent deux fois plus d'appels**. L'ordre de grandeur recoupe
      ADR-028 : 152 M de passages perdus sur 8 900 ticks font ~17 000 par tick
- [ ] **La part du TEMPS de tick expliquée reste non mesurée**, et le dire est
      obligatoire (R-660). Le chronométrage exige `DEEP` sur les 2 475 sondes, le
      gouverneur l'annule aussitôt (ADR-020, `apply_verdict` → `clear_requested_depth`).
      Ce n'est pas un garde-fou à lever : c'est un appel à **chronométrer moins de sondes
      à la fois**, donc à les choisir — ce que `/rfx discover` permet désormais
- [x] **`/rfx profile N` ne ment plus.** Le message annonce une *demande* et sa
      condition — « il s'arrêtera plus tôt si le coût dépasse le budget » — et le natif
      consigne désormais ce que la session a **réellement** duré
      (`profile_requested_ticks`, `profile_granted_ticks`, `profile_cancelled`).
      `/rfx top` affiche « chronométrage écourté : %s ticks obtenus sur %s demandés »
      quand le gouverneur a coupé, et ne dit rien quand la session est allée à son terme
- [ ] ~~PROCHAIN TRAVAIL — vidage multi-thread~~ (PARTIE 5.39 étape 5, RISK-05).
      `probeBufferFlush` doit accepter un appel hors fil autoritatif, sans verrou dans
      le chemin chaud (R-320), sans multiplier les traversées (R-700 : dix-huit threads
      feraient déjà dix-huit traversées par tick), sans jamais muter l'état Minecraft
      (INV-02). C'est là que se trouve la couverture qu'on cherchait depuis trois ADR
- [ ] Le banc a une dispersion d'exécution à exécution d'environ **15 ms sur le MSPT
      médian** (55,23 puis 71,03 ms pour la même configuration). Toute campagne qui
      compare deux configurations doit les **apparier par position**, sinon elle ne
      mesure que l'ordre des exécutions
- [ ] Le profilage est désormais éteint 20 ticks sur 320, soit 6,3 % du temps
      d'observation contre 0,33 % avant. À mesurer et à assumer explicitement
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

- [x] `net.minecraftforge.eventbus` **est ouvert** à notre module — relevé en production :
      « paquet interne ouvert à rustforgex — true ». Les étapes 2 et 3 sont techniquement
      possibles ; cela ne les rend pas souhaitables pour autant
- [x] **Étape 1 écrite** : `EventDispatchTable` (logique pure, 11 tests, sans Forge) et
      `EventObserver` (glu : deux auditeurs sur `Event.class`, `HIGHEST` et `LOWEST`,
      `receiveCanceled` des deux côtés). Aucun auditeur tiers touché — R-330 tenu par
      construction. Comptage systématique, chronométrage d'un événement sur 64
- [x] **Coût de l'observateur : non distinguable du bruit.** Paire courte, mêmes
      paramètres que sans lui : p50 +4,6 %, p95 **+0,7 %**, p99 −1,1 %, contre
      +5,8 / +2,7 / +3,1 sans l'observateur. 38 000 événements distribués, 576
      chronométrés, 0 abandon
- [ ] Ne pas conclure de cette paire que l'observateur est gratuit : une seule paire
      courte, et certains écarts sont négatifs, ce qui borne la précision. Ce qu'elle
      établit, c'est qu'il ne coûte pas un ordre de grandeur
- [ ] **Douze types d'événements seulement** sur 38 000 distributions : c'est la limite du
      serveur au repos, pas celle de l'instrument. Un serveur joué en poste des centaines,
      et c'est exactement ce que le profil de charge doit corriger
- [ ] La trace d'ordre que l'acceptance de la PARTIE 5.6 demande n'est pas écrite :
      personne ne la consommerait encore, et une trace que rien ne lit est la
      configuration morte qu'ADR-018 proscrit
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

- [x] Inventaire des conteneurs de mods, `owner_mod_hash`, attribution par module
      (ADR-023 : la table par chargeur est dégénérée sous ModLauncher, tous les mods
      partageant un chargeur ; 288 modules distincts pour 290 mods)
- [x] Indexation paresseuse : aucune classe chargée qui ne le serait pas (R-620).
      La découverte ne lit aucune archive — l'empreinte est calculée à la demande
- [x] Moins de 500 ms pour 250 mods (R-621) : **15 ms pour 290 mods** en production,
      et T-442 le vérifie sur 250 mods synthétiques
- [ ] Enregistrer les threads créés par les mods (étape 5 de la PARTIE 5.39, RISK-05) :
      demande un hook sur la création de thread, ou l'échantillonnage des piles
- [x] `rfx.discovery.unknown_owner_ratio` compté dans `ProbeRegistry`, relevé à l'arrêt
      — mesuré à la fin du chargement il ne portait que sur 160 sondes, l'essentiel des
      classes se chargeant pendant la partie. **Vérifié en jeu : 2 641 sondes, 0 non
      rattachée, 0,0 %.** L'explication est structurelle : `TargetScanner` ne vise que
      des classes de JAR de mods, et ces JAR déclarent leurs paquets
- [ ] Ce compteur ne voit pas la **mauvaise** attribution, seulement l'absence
      d'attribution. Deux mods partageant un préfixe de paquet seraient tous deux
      comptés « rattachés », l'un à tort. C'est ce que l'attribution par module corrige
- [ ] **L'attribution par module n'a aucun appelant en production** : le transformateur
      ne connaît que le nom interne de la classe. La table existe, est testée, et sert à
      `/rfx mods` — mais `ownerOf(module, classe)` n'est appelé que par les tests. Route
      possible : `StackTraceElement.getModuleName()` dans l'échantillonneur de piles, ou
      `Class.getModule()` partout où un `Class` est disponible
- [ ] Acceptance PARTIE 5.39 : moins de 5 % des **classes chaudes** en `owner = unknown`.
      Le compteur actuel porte sur les méthodes sondées, toutes, pas seulement les
      chaudes : il ne prononce pas l'acceptance, il montre seulement que l'attribution
      fonctionne

### Étape H — C-34 Telemetry, C-35 Diagnostics, C-38 complet

- [x] **C-34 : métriques typées à cardinalité bornée, export JSON.** 27 séries en six
      domaines, chacune nommée `rfx.<domaine>.<mesure>`, typée, unitée et documentée.
      Borne de 256 séries **vérifiée à l'exécution** — la dépasser lève, parce qu'un
      dépassement signale un nom construit à partir d'une donnée (T-402)
- [x] R-660 tenu dans la télémétrie : une valeur que la source n'a pas mesurée n'est
      pas publiée du tout. La ligne de base de la PARTIE 12.4 n'apparaît qu'après une
      pause réelle — un `0 %` sans mesure ferait passer une absence pour un coût nul
- [x] **T-400 : aucune socket.** Vérifié mécaniquement, pas promis : aucune source Java
      ni Rust n'a le droit de nommer un type ouvrant une connexion
- [x] **R-562 : toute métrique documentée dans `BENCHMARKS.md`**, et un test échoue si
      une métrique produite par le code n'y figure pas. Un catalogue qu'aucun test ne
      garde décrit bientôt un système qui n'existe plus
- [x] **R-571 : anonymisation** — `<gameDir>` et `<home>` remplacent les racines, plus
      longue d'abord, les deux séparateurs reconnus (T-412)
- [x] `/rfx mods` et `/rfx report`
- [ ] `/rfx why`, `/rfx top`, `/rfx workload` : demandent la liste des unités de travail
      les plus coûteuses, que le natif n'exporte pas encore. Suppose une fonction FFI
      de plus — travail normal, mais pas gratuit
- [ ] Fiche d'explication de la PARTIE 5.33 : **seul le bloc `MEASURED` est productible
      à ce jalon.** `ANALYSIS` suppose C-19 à C-22 (lectures, écritures, déterminisme),
      `DECISION` suppose C-23 à C-27. Aucun n'existe. Les remplir serait inventer
- [ ] R-570 : fiche en moins de 50 ms — non mesurable tant que la fiche n'existe pas
- [ ] R-561 : coût de la télémétrie sous 0,2 % du MSPT (T-401) — **non mesuré**. Le
      relevé n'a lieu que sur commande, jamais dans un tick, mais « probablement
      négligeable » n'est pas une mesure
- [ ] Dump d'incident `crash/rfx-crash-<ts>.json` (T-411)
- [ ] **`hook_budget_exceeded` vaut 123 sur 4 039 ticks** — première fois qu'il est non
      nul. Budget : 500 µs par accroche (`DEFAULT_MAX_HOOK_NS`), soit ~1 % des appels.
      Deux explications tiennent, et le compteur actuel **ne permet pas de trancher** :
      soit `tick_end` traîne (il parcourt 2 650 unités dans `adapt_probe_levels`), soit
      une pause GC tombe pendant la mesure — `record_hook` mesure du temps écoulé, donc
      attribue les pauses GC à nos accroches. La campagne a compté 1 564 ticks
      contaminés par le GC sur 12 000, ce qui rend la seconde explication très plausible
- [ ] Rendre ce compteur diagnosticable : retenir la pire durée observée et quelle
      accroche l'a produite. Un compteur qu'on ne sait pas interpréter ne sert à rien —
      c'est la même leçon que `CLASSES_SEEN` incrémenté du mauvais côté du retour

### Étape H bis — Ramener le coût du sondage sous le budget

État mesuré au 2026-09-09 : **~129 µs par tick** (deux mesures indépendantes à 2 %
l'une de l'autre), contre un budget de 1,5 % du MSPT ≈ **63 µs**. Facteur deux à
trouver.

- [x] **L'horloge lue pour rien** : `System.nanoTime()` remplissait `timestamp_ns`, que
      le natif ne lit jamais. 249 → ~129 µs
- [x] ~~Le niveau COUNTER violait R-700~~ — livré, comptage par lot (commit `7923836`).
- [ ] ~~**Le niveau COUNTER viole R-700 — chantier principal.**~~ Pour transporter un
      simple « +1 », le chemin chaud écrit **32 octets** dans le tampon et lit **trois
      champs `volatile`**. Or R-700 exige « par lot, jamais par élément » : le tampon
      le respecte pour les mesures chronométrées, le comptage franchit la frontière un
      élément à la fois

      Conception retenue, par fil sondé :

      ```java
      int[] counts;   // indexé par identifiant de sonde
      int[] touched;  // identifiants vus ce tick, pour ne pas balayer 2 649 entrées
      int  touchedLen;
      ```

      Chemin chaud au niveau COUNTER : `if (counts[id]++ == 0) touched[len++] = id;`
      — une lecture de tableau, une incrémentation, une comparaison. Ni `volatile`, ni
      écriture de 32 octets, ni traversée par appel.

      Vidange en fin de tick : parcourir `touched`, émettre **un** enregistrement
      `ENTER` par sonde touchée avec `value = count`, remettre à zéro. Le coût devient
      proportionnel au nombre de sondes **touchées**, pas au nombre d'**appels**
- [ ] Côté natif : `RecordKind::Enter` fait `calls_this_tick += 1`. Il devra faire
      `+= record.value`. Java écrira toujours une valeur ≥ 1
- [ ] Le balayage naïf serait un piège : 2 649 entiers par fil et par tick coûteraient
      plusieurs microsecondes pour rien quand une poignée de sondes seulement est
      appelée. D'où `touched`, alimenté au passage de zéro à un
- [ ] R-320 : les deux tableaux sont alloués une fois par fil, jamais dans le chemin
      chaud. T-142 doit rester vert
- [ ] Petits gains au passage : `sink` est lu deux fois par appel (`enter` puis
      `record`), une suffit ; `probeId < 0` ne peut pas arriver, les identifiants
      venant de constantes injectées dans le bytecode
- [ ] Vérifier en jeu, puis **une** mesure de qualité comparable à la référence
      (fenêtre 512, période 2048) avant d'annoncer un chiffre

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
- [x] **Seconde campagne exécutée** sur `38eacfa`, conforme et fiable. Résultat :
      `benchmarks/results/macro-1788636356-38eacfa.json`. **Le coût n'a pas baissé** :
      MSPT p95 2,468 → 3,366 ms, soit **+36,4 %** contre +33,7 % la veille. Les deux
      correctifs du jour n'y changent rien — le filtre n'a retiré que 2,6 % des cibles,
      et le gouverneur d'overhead ne s'est jamais déclenché
- [ ] **À l'agrégation** : l'agrégateur lit le commit de `HEAD`, qui aura avancé depuis
      le lancement. Agréger depuis un `git checkout 38eacfa` détaché — `benchmarks/runs`
      est ignoré par git et survit au changement — sans quoi le résultat citerait un
      commit dont le code n'a jamais été mesuré
- [ ] Faire porter au fichier d'exécution le commit **mesuré**, pour que l'agrégateur
      n'ait plus à le déduire de `HEAD`
- [ ] **Profils de charge de la PARTIE 22 — la lacune la plus importante de la mesure.**
      Formulation à corriger : « profil chargé » voulait dire 272 mods *chargés*, pas
      serveur *sous charge*. Les quatre campagnes ont mesuré un serveur **au repos** :
      aucun joueur, presque aucune entité, aucun chargement de région, aucune IA. Le tick
      y dure 1,6 à 2,5 ms quand un serveur joué en dure 20 à 45. Deux conséquences de
      sens opposé : le pourcentage relatif y est **surestimé** (dénominateur minuscule),
      et le coût absolu **sous-estimé** (bien moins de méthodes sondées sont réellement
      atteintes). Ce n'est donc ni pessimiste ni optimiste, c'est un autre régime
- [x] **Charge scriptée écrite et vérifiée en jeu** : `LoadProfile`, trois profils
      (`none`, `chunks`, `mobs`), idempotent, appliqué au tick 20 depuis le fil
      autoritatif. 210 commandes, **0 refusée**. Effet mesuré sur le serveur de
      production : MSPT p50 1,910 → **9,316 ms**, p95 3,143 → **15,637 ms**, types
      d'événements 12 → 24, TPS toujours à 20,000. **Le serveur au repos cachait un
      facteur cinq**
- [x] **Paire B/C sous charge mesurée** : p50 +2,6 %, p95 **+2,7 %**, p99 +3,8 %, TPS
      inchangé. Les deux effets annoncés sont réels et opposés — le pourcentage tombe de
      +6,6 % à +2,7 %, mais le **coût absolu monte** de 0,16 à **0,275 ms**, davantage de
      méthodes sondées étant réellement atteintes. Le tick a été multiplié par 4, notre
      coût par 1,7 seulement
- [ ] Lecture des budgets sous charge : « 2 % d'un cœur » → 0,275/50 ms = **0,55 %**,
      tenu avec un facteur quatre de marge. Budget MSPT de 1,5 % → +2,7 %, toujours
      dépassé mais d'un facteur 1,8 au lieu de 4. **Une seule paire**, pas une campagne
- [x] **Mod synthétique de charge écrit** (PARTIE 22, R-870, R-871) :
      `rustforgex-bench-synth`, artefact séparé dans son propre paquet `rfxbench.synth`
      — indispensable, `TargetScanner` refusant `dev/rustforgex/`. Paramétrable en
      unités de travail, coût CPU, allocations, gestionnaires d'événements, fils propres
      et non-déterminisme. Inerte sans propriétés. Chargé par Forge parmi les 272 mods
- [ ] Le monde pré-généré avec une usine réelle n'est **pas** la bonne réponse : la
      PARTIE 22 veut des charges, pas des mods tiers, et R-871 interdit un profil conçu
      autour d'un mod nommé. Reste utile en complément manuel, documenté dans
      `BENCHMARKS.md`, si l'utilisateur construit une base lui-même
- [x] **Générateur de sources écrit et vérifié** : `generateSynthSources` engendre
      120 classes × 20 méthodes = **2 400 méthodes sondables**, réglables par
      `-PrfxSynthClasses` / `-PrfxSynthMethods`. Vérifié en jeu : `methods_probed`
      2 648 → **5 050**, soit +2 402. La surface instrumentée est doublée par rapport au
      modpack seul
- [x] **Défaut de C-04 mis au jour par le générateur : les classes chargées pendant la
      construction des mods ne sont jamais sondées.** Chiffré par un compteur des ratés
      (`CLASSES_MISSED`) : sur 76 327 classes visées, **32 712 passent avant l'armement**,
      soit 43 %. Les 2 635 méthodes sondées ne sont pas « celles du modpack » mais
      « celles chargées après le setup commun »
- [x] **Le runtime peut démarrer au constructeur du mod** (ADR-022) : option
      `instrumentation.early_arm`, fausse par défaut. Vérifié en production, 288 mods :
      classes perdues 32 712 → **5 318**, méthodes sondées 2 635 → **13 615** (×5,2),
      zéro échec de transformation. Les 4 875 classes chargées avant la construction du
      mod restent hors d'atteinte — les viser supposerait de démarrer le natif dans la
      couche d'amorçage, ce qui touche INV-13
- [ ] **Mesurer ce que l'armement anticipé coûte au tick** avant d'en faire le défaut.
      ADR-021 a établi que le coût croît avec le nombre de méthodes sondées ; quintupler
      cette population rouvre la question. Paire B/C sous charge `mobs` lancée avec
      `run-macro-prod.sh … mobs true`, à comparer aux +2,7 % p95 de l'ordonnancement
      tardif
- [ ] Calibrer les profils sur la table de la PARTIE 22 : `vanilla` 200 entités /
      400 chunks, `light` 10 mods / 500 / 600, `medium` 60 / 2 000 / 1 200, `heavy`
      150 / 8 000 / 2 500. Le profil actuel est en dessous du plus petit
- [ ] Monde pré-généré du point 2 de la PARTIE 21.3 : le monde actuel est réutilisé et
      accumule ses régions, ce qui fait dériver les valeurs absolues entre campagnes
- [ ] Le modpack d'essai est celui de l'utilisateur : non versionnable, donc non
      reproductible par un tiers
- [ ] Côté client : FPS, frametime, 1 % low

**Ce que le niveau A ne mesure pas**, et qu'il ne faut pas lui faire dire : le coût
Java de `RfxProbes.enter/exit` sur le chemin chaud. Un chronométrage Java naïf mentirait
(élimination de code mort), et JMH est une dépendance à peser séparément. T-131 reste
donc ouvert.

### Definition of Done du jalon (CDC PARTIE 29.3)

- [ ] T-130..T-134, T-140..T-144, T-150..T-154, T-370..T-373, T-400..T-402,
      T-410..T-412, T-420..T-422, T-440..T-442 verts
- [x] **Overhead mesuré et affiché sous 2 % sur un profil de charge** (ADR-025).
      Mesuré sous le profil `medium` de la PARTIE 22 — 2 000 entités, 1 200 chunks,
      MSPT p50 de 20,8 ms, ce qui est enfin un serveur qui travaille :

      | lecture | valeur | budget | |
      |---|---|---|---|
      | part d'un cœur (H-07) | **0,51 %** | 2 % | ✅ facteur 4 |
      | part du MSPT (PARTIE 5.5) | **1,41 %** | 1,5 % | ✅ de peu |

      Toutes les mesures antérieures portaient sur `mobs` — 4,2 ms de MSPT, soit un
      serveur au repos, et un profil **sous la ligne `vanilla`** de la table. Le budget
      relatif au MSPT s'y resserrait au maximum, précisément là où le coût importe le
      moins
- [ ] **La marge sur la lecture MSPT est mince et la mesure est faible** : 6 cycles
      positifs sur 10, queue négative à −1,14 ms. ADR-024 dit qu'une telle médiane est
      biaisée vers zéro — le coût réel pourrait dépasser 275 µs. La lecture « part d'un
      cœur » garde, elle, un facteur 4 et tient même si le chiffre était sous-estimé de
      moitié
- [ ] Le profil `heavy` (8 000 entités) **tue le serveur de banc** : la sauvegarde
      automatique sérialise toutes les entités d'un coup, tick de 120 s, arrêt par le
      chien de garde. La table de la PARTIE 22 décrit des mods synthétiques, pas
      290 mods réels ; ses chiffres ne se transposent pas
- [ ] Aucun test de gameplay ne diverge de la référence non instrumentée

      **Plan G-03 (PARTIE 20.3.4), premier test de gameplay** — génération de 2 000
      chunks, comparaison NBT avec la référence. Choisi en premier parce que la
      génération de monde traverse le plus de code de mods sondé, et parce qu'elle part
      d'une graine : c'est le scénario le plus déterministe de la liste.

      - [x] Profil `worldgen` dans `LoadProfile` : jeu figé, `randomTickSpeed 0`,
            carré de 45 × 45 chunks forcé ; `forcedChunkRange` seule source de la
            géométrie, vérifiée chunk par chunk par un test
      - [x] `WorldDigest` : quatre empreintes par chunk — blocs, biomes, entités de bloc,
            structures. État lu **vivant** et non sérialisé (la palette dépend de l'ordre
            de pose). Lumière, cartes de hauteur, ticks planifiés et entités exclus, et
            le code dit pourquoi
      - [x] `DigestRecorder` (`rustforgex.bench.digest.out`) : génère, attend les 2 025
            chunks `FULL`, repose 200 ticks, hache, écrit, arrête
      - [x] `rfx-bench digest ref1 ref2 candidat` : test **symétrique** — laquelle des
            trois exécutions est l'intruse — avec loi binomiale de paramètre 1/3 et seuil
            p = 0,01 sur une composante bruitée, égalité stricte sur une composante
            déterministe (ADR-032)
      - [x] **A/A mesuré** : sans RF-X, deux générations à la même graine diffèrent déjà
            sur ~920 chunks sur 2 025 pour les blocs. Biomes et structures : 2 025 / 2 025
      - [x] **G-03 passé** (ADR-032) :

            | composante | RF-X intrus | p | verdict |
            |---|---|---|---|
            | biomes | 0 | — | égalité stricte |
            | structures | 0 | — | égalité stricte |
            | blocs | 239 (ref : 281, 153) | 0,124 | passe, résolution limitée |
            | entités de bloc | 3 (ref : 1, 2) | 0,320 | passe, résolution limitée |

            55 erreurs dans chacune des trois exécutions, messages identiques ; aucune
            `VerifyError`, aucune erreur de RF-X
      - [x] **Empreinte par section** (ADR-032, mise à jour) : le bruit vit entre
            Y = −16 et Y = 79, jamais au-dessus de 160, et pas sur les bords du carré.
            Les blocs se jugent désormais par hauteur, avec correction de Bonferroni :
            14 hauteurs en égalité stricte (28 350 sections), les 10 autres passent.
            Deuxième campagne, même verdict
      - [x] **Protocole** (`benchmarks/run-gameplay.sh`) : échauffement écarté, candidat
            au milieu. L'échauffement n'a PAS supprimé l'excès de la référence 1
            (1 303 contre 936 et 916) ; c'est la position du candidat qui protège le
            verdict. Cause de l'excès inconnue
      - [x] **G-03 a trouvé un défaut réel** : génération tuée par le chien de garde avec
            RF-X — refus du natif non retenu, places des threads morts jamais rendues.
            Corrigé (`9405d35`) : 81 s contre 73 à 82 s pour les références
      - [x] **G-03 et G-01 passent** (ADR-032) : égalité stricte au-dessus de Y = 160 et
            Y = 112, biomes et structures ; mêmes erreurs ; aucun chien de garde
      - [x] **G-08** (ADR-032) : blocs et biomes font un aller-retour parfait, avec et
            sans RF-X. Structures : 2 chunks diffèrent, les mêmes chez la référence
      - [x] **G-08, entités de bloc : c'est Lootr** (ADR-032). Seuls ses conteneurs
            changent à l'aller-retour (`tileId` aléatoire, table de butin), avec comme
            sans RF-X ; RF-X a cette fois le moins d'écarts (26 contre 59 et 68). Tous les
            autres types font un aller-retour exact. **G-08 passe**
      - [x] Comparateur : `rfx-bench roundtrips` juge l'aller-retour par classe de
            changement (entités : type et champ ; le reste : chunk ou section). Une classe
            produite par une référence est tolérée, toute autre est imputable. Vérifié sur
            la campagne du 2026-10-04 (passe) et par un défaut injecté (détecté)
      - [x] Campagne G-08 complète par le script, 2026-10-04 03:17–03:45 : statut 0
            (génération, allers-retours et erreurs ; ADR-032)
      - [ ] Les autres scénarios G-02..G-15 ; G-02, G-10 et G-15 demandent des clients
- [ ] JAR installable et jouable, client et serveur dédié.
      **Client lancé pour la première fois le 2026-09-08**, sous ForgeGradle
      (`runClient`) :

      ```
      [Render thread] RUSTFORGE-X actif (75 ms) : Runtime natif prêt
      [Render thread] Transformateur armé
      [main]          Cibles énumérées : 6865 classes en 195 ms
      [Worker-Main-7] 3 mods inventoriés en 6 ms
      ```

      Écran-titre atteint, **zéro erreur**, aucun `NoClassDefFoundError`, aucune
      exception sur le fil de rendu. La sécurité de côté (section 4.3) tient, et C-41
      fonctionne aussi côté client. 3 553 classes perdues avant l'armement, contre
      32 712 sur le serveur de banc
- [ ] **Le test client reste faible et la case reste ouverte.** L'environnement de
      développement ne charge que **trois mods** : la ligne d'énumération dit
      `0 écartées, 0 référençant un type absent`. Le risque signalé n'est donc **pas
      exercé** — côté client, `dedicatedServer=false` n'écarte plus les classes qui
      référencent `net.minecraft.client.*`, et sur un vrai modpack ce sont des dizaines
      de milliers de classes supplémentaires qui deviennent candidates
- [ ] La vraie épreuve est une instance CurseForge cliente avec son modpack. Elle touche
      les installations de l'utilisateur : à faire avec son accord explicite, pas d'office

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

- ~~**Licence définitive** (ADR-010)~~ — **décidée le 2026-10-03 : Apache 2.0** (ADR-010),
  avec `NOTICE` pour l'attribution. Ancienne note : `LICENSE` était « tous droits réservés »,
  cohérent avec `mod_license`. Hors périmètre de décision de l'agent (contrat 5).
- **Remote git** : le dépôt est local, aucun remote configuré, aucun push effectué.

---

## M11 — Refonte du moteur de rendu (après la 1.0.0)

Jalon ajouté au CDC le 2026-10-03 par décision de Killian (ADR-031, PARTIE 30). Il ne
commence qu'une fois la 1.0.0 publiée ; rien de ce qui suit ne doit infléchir M0 à M10.

- [ ] Spécifier le jalon par un ADR avant tout code : composant, exigences, tests,
      Definition of Done. Point de départ : `docs/design/C-53-exploration-rendu-vulkan.md`.
- [ ] Choisir, ou non, une bibliothèque native tierce (`wgpu`, `ash`) : décision de
      Killian (contrat 5).
