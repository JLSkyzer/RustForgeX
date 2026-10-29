# TODO — RUSTFORGE-X

**Jalon courant : M2 (Analyse et ordonnancement).** M0 est livré. M1 est clos avec
réserves (ADR-042) : ce qui reste à reprendre y est listé, avec le jalon avant lequel le
faire.
Source de vérité : `docs/spec/RUSTFORGE-X_Cahier_des_Charges_v1.0.md`.

---

## M0 — Bootstrap ✅ LIVRÉ (2026-09-04)

C-27, C-45, C-03, C-02, C-37, C-01, C-38 (`/rfx status`), C-40 (build natif, CI,
artifact-verify). 57 tests Java, 47 tests Rust. Vérifié en jeu : démarrage, sonde,
arrêt propre, connexion client/serveur.

Reste à faire à la première release : `INSTALLATION.md`, `RELEASING.md`.

---

## M1 — Observation ✅ CLOS AVEC RÉSERVES (2026-10-16, ADR-042)

> **Clos par décision de Killian (ADR-042).** `[x]` = fait. `[~]` = fermé sans être fait :
> reporté, ou constat sans suite propre. L'ADR-042 dit ce qui doit être repris, et avant
> quel jalon (« avant M3 », « avant toute release »).

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
- [~] Aucune fuite sur un cycle long (T-370) — demande le test long T-700

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
- [~] Coûts par niveau mesurés sous leur seuil (T-131) — demande le harnais C-36
- [~] Retrait de sonde si régression JIT au-delà du seuil (R-313) — demande un
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
- [~] Réduire encore : 76 202 classes restent déclarées pour une fraction réellement
      sondée, chacune décodée et réécrite pour rien. Évaluer l'éligibilité au moment de
      l'énumération supposerait d'analyser le code de chaque classe au démarrage — à
      mesurer avant de le décider (ADR-019)
- [x] **La profondeur ne remonte plus avant la première ligne de base** (ADR-020,
      deuxième volet) : le profiler atteignait `DEEP` en 25 s sur la foi de compteurs
      aveugles à sa dépense dominante. Vérifié en jeu : `LIGHT` au lieu de `DEEP`
- [~] **Le bruit de la ligne de base est MESURÉ, et la méthode ne peut pas marcher.**
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
- [~] Conséquence chiffrée : pour résoudre 250 µs dans un bruit d'écart-type ~1,5 ms, il
      faudrait environ **350 cycles**, soit 112 000 ticks — une heure et demie de jeu par
      mesure. La cadence 300/20 de la PARTIE 12.4 ne peut pas y arriver, quel que soit
      le nombre de cycles raisonnable
- [~] Piste à instruire demain : **apparier des ticks adjacents** plutôt que deux blocs
      de vingt séparés d'une seconde. Deux ticks consécutifs se ressemblent bien plus
      que deux blocs distants ; l'appariement annule la part lente de la variance, qui
      est probablement l'essentiel. Coût à évaluer : basculer le niveau des sondes à
      chaque tick suppose de retransmettre la table, ce qui coûte et fausserait la
      mesure. À mesurer avant de décider
- [~] **CASE ROUVERTE — les zéros de la ligne de base ne sont PAS expliqués.** Cochée
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
- [~] **Ne pas publier de médiane sur cette campagne sans dire ce qu'elle mélange.**
      Certaines exécutions finiront à `OFF`, d'autres à `DEEP` : la médiane agrégerait
      deux régimes de fonctionnement différents, et le chiffre n'aurait pas de sens
      physique
- [x] **Le gouverneur s'engage quand la ligne de base voit juste.** Au tick 9 600, une
      mesure à 3,01 % dépasse le budget : le profileur descend jusqu'à `OFF` et désarme
      les 2 541 sondes. Le mécanisme fonctionne ; ce qui l'alimente, non — voir la case
      rouverte ci-dessus
- [~] **Conséquence à assumer : une campagne mesure désormais un mélange.** Les 12 000
      ticks couvrent une période sondée puis une période éteinte. Le p50 qui en sort
      n'est donc pas « le coût de l'instrumentation » mais celui du produit tel qu'il se
      comporte. Les deux chiffres sont utiles, ils ne répondent pas à la même question —
      à séparer explicitement dans le prochain ADR
- [~] **L'armement anticipé mesure un coût PLUS FAIBLE que le tardif** (2,43 % contre
      3,01 %) alors qu'il sonde cinq fois plus de méthodes. À relativiser : la mesure
      qui les sépare est celle dont l'instabilité vient d'être constatée. Explication possible : il
      atteint le budget plus tôt, donc le gouverneur l'éteint plus tôt, donc la fenêtre
      de pause tombe sur un système déjà moins actif — ce serait un artefact de mesure,
      pas une propriété. **Une seule exécution** ; à confirmer sur les cinq
- [~] Texte d'origine, conservé parce qu'il a orienté trois ADR : « la mise en pause de
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
- [~] **Ce constat portait sur des sondes mortes et doit être refait.** « Le gouverneur
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
- [~] **Trancher la référence du budget.** H-07 borne à « 2 % d'un cœur » : mesuré
      **0,32 %**, tenu avec un facteur six de marge. Le budget de MSPT, celui
      qu'`OverheadMeter` applique, vaut 1,5 % du tick : à +6,6 %, il ne l'est pas. La
      Definition of Done dit « sous 2 % » sans préciser la référence. **Cette ambiguïté
      doit être levée par une décision explicite, pas par le choix du chiffre le plus
      flatteur**
- [~] R-311 prévoit une exception — sonder quand même une méthode courte si
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
- [~] **On ne peut pas acheter de la couverture en armant plus de sondes.** Trois
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
- [~] **La part du TEMPS de tick expliquée reste non mesurée**, et le dire est
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
- [~] ~~PROCHAIN TRAVAIL — vidage multi-thread~~ (PARTIE 5.39 étape 5, RISK-05).
      `probeBufferFlush` doit accepter un appel hors fil autoritatif, sans verrou dans
      le chemin chaud (R-320), sans multiplier les traversées (R-700 : dix-huit threads
      feraient déjà dix-huit traversées par tick), sans jamais muter l'état Minecraft
      (INV-02). C'est là que se trouve la couverture qu'on cherchait depuis trois ADR
- [~] Le banc a une dispersion d'exécution à exécution d'environ **15 ms sur le MSPT
      médian** (55,23 puis 71,03 ms pour la même configuration). Toute campagne qui
      compare deux configurations doit les **apparier par position**, sinon elle ne
      mesure que l'ordre des exécutions
- [~] Le profilage est désormais éteint 20 ticks sur 320, soit 6,3 % du temps
      d'observation contre 0,33 % avant. À mesurer et à assumer explicitement
- [~] Échelons 2 et 3 de la PARTIE 12.4 — « suspendre les nouvelles décisions », puis
      `DEGRADED` : rien à suspendre tant que le moteur de décision n'existe pas (ADR-020)
- [~] Troisième configuration — transformateur présent, sondes jamais posées — pour
      séparer le coût de la transformation de celui des sondes. Écrite après la
      campagne : `run-macro-prod.sh` est lu au fil de l'eau par bash pendant l'exécution,
      l'éditer en cours de campagne la corromprait. Demande aussi que l'agrégateur
      compare chaque configuration à la référence, et non les deux premières
- [x] Dette d'ADR-018 close **par construction** : une méthode ne peut plus porter un
      mixin et une sonde, puisque les cibles de mixins sont exclues. L'hypothèse
      d'ADR-018 était d'ailleurs fausse — un `@ModifyVariable` l'a réfutée
- [~] Cibles atteintes **indirectement** par un mixin (héritage, configuration
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
- [~] Reste de C-40 : version du manifeste égale au tag, plage de taille attendue,
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
- [~] Ne pas conclure de cette paire que l'observateur est gratuit : une seule paire
      courte, et certains écarts sont négatifs, ce qui borne la précision. Ce qu'elle
      établit, c'est qu'il ne coûte pas un ordre de grandeur
- [~] **Douze types d'événements seulement** sur 38 000 distributions : c'est la limite du
      serveur au repos, pas celle de l'instrument. Un serveur joué en poste des centaines,
      et c'est exactement ce que le profil de charge doit corriger
- [~] La trace d'ordre que l'acceptance de la PARTIE 5.6 demande n'est pas écrite :
      personne ne la consommerait encore, et une trace que rien ne lit est la
      configuration morte qu'ADR-018 proscrit
- [~] Mesurer le coût de cet auditeur par la ligne de base de la PARTIE 12.4 avant
      d'aller plus loin : il s'exécute des milliers de fois par tick
- [~] Étape 2 — reconstituer ordre, priorité et `owner` par lecture réflexive
      (`EventBus.busID`, `ListenerList.getListeners`) ; aucune écriture
- [~] Étape 3 — envelopper, et seulement si l'attribution de l'annulation à un handler
      précis se révèle nécessaire. Traiter une priorité entière d'un coup : `register`
      ajoute en queue, donc un remplacement un par un violerait R-330
- [~] Désenveloppement automatique si un mod inspecte le listener (FM-14)
- [~] T-154 exige un surcoût **mesuré** sous 30 ns : suppose un harnais JMH, même dette
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
- [~] Enregistrer les threads créés par les mods (étape 5 de la PARTIE 5.39, RISK-05) :
      demande un hook sur la création de thread, ou l'échantillonnage des piles
- [x] `rfx.discovery.unknown_owner_ratio` compté dans `ProbeRegistry`, relevé à l'arrêt
      — mesuré à la fin du chargement il ne portait que sur 160 sondes, l'essentiel des
      classes se chargeant pendant la partie. **Vérifié en jeu : 2 641 sondes, 0 non
      rattachée, 0,0 %.** L'explication est structurelle : `TargetScanner` ne vise que
      des classes de JAR de mods, et ces JAR déclarent leurs paquets
- [~] Ce compteur ne voit pas la **mauvaise** attribution, seulement l'absence
      d'attribution. Deux mods partageant un préfixe de paquet seraient tous deux
      comptés « rattachés », l'un à tort. C'est ce que l'attribution par module corrige
- [~] **L'attribution par module n'a aucun appelant en production** : le transformateur
      ne connaît que le nom interne de la classe. La table existe, est testée, et sert à
      `/rfx mods` — mais `ownerOf(module, classe)` n'est appelé que par les tests. Route
      possible : `StackTraceElement.getModuleName()` dans l'échantillonneur de piles, ou
      `Class.getModule()` partout où un `Class` est disponible
- [~] Acceptance PARTIE 5.39 : moins de 5 % des **classes chaudes** en `owner = unknown`.
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
- [x] `/rfx top`, `/rfx why <workid>`, `/rfx workload <workid>` (ADR-063) : entrée FFI
      `rfx_workload_explain`, type CBOR `WorkloadCard` ; dans `/rfx top`, le nom d'une
      unité propose `/rfx why` au clic. Identifiant saisi en hexadécimal non signé
- [~] Fiche d'explication de la PARTIE 5.33 : `MEASURED` et `ANALYSIS` (descripteur
      C-08 résumé, avec la liste des faits qui maintiennent une unité en `UNKNOWN`).
      **`DECISION` dit qu'aucune décision n'est prise** : le moteur (C-23 à C-27, M3)
      n'existe pas, le remplir serait inventer
- [~] R-570 : fiche en moins de 50 ms — mesurée par T-421 (2026-10-29, deux passes de
      20 exécutions) pour l'identifiant `0`, unité **non suivie et non décrite** :
      `why` médiane 0,34-0,36 ms, maximum 1,1 ms ; `workload` maximum 1,5 ms. Reste à
      mesurer sur une unité réellement décrite, en session de jeu
- [x] T-421 : premier `/rfx discover` à 5,2 ms puis 4,1 ms (2026-10-29). Cause mesurée :
      tri complet de la table, `label()` reconstruit à chaque comparaison ; table pleine
      (8 192), 10 à 18 ms à chaud, 57 ms à froid hors jeu. Sélection des k meilleures sur
      un instantané des compteurs (2 à 5 fois moins), et calcul sur le fil des rapports,
      réponse renvoyée sur le fil du serveur. Ensuite, deux passes : premier `discover`
      à 1,3 ms, médiane 0,16-0,18 ms. Le harnais ne capture que la réponse immédiate :
      la réponse différée suit le chemin de `/rfx report`
- [x] R-561 : coût de la télémétrie sous 0,2 % du MSPT (T-401, ADR-034) — pire
      rapport **0,117 %** sur 5 exécutions (médiane 0,085 %), serveur de banc sans joueur,
      MSPT 1,6-2,0 ms. Marge d'un facteur 1,7 seulement
- [x] Le journal de tick (`TickTrace`) coûtait 144 à 193 µs par écriture sur le fil du
      serveur (ADR-034) : il s'écrit désormais sur le fil des rapports ; le fil du serveur
      ne fait que déposer la tâche (`TickTraceTest`)
- [x] `HookGuard` chronométrait chaque appel sans que rien ne l'exporte : séries
      `rfx.hook.calls/errors/duration_ns{hook=…}` et `rfx.hook.disabled` désormais
      relevées par `RfxRuntime.metrics()` (PARTIE 4.19), documentées dans `BENCHMARKS.md`
- [x] `telemetry.enabled` n'était lue par rien : elle coupe désormais la télémétrie de
      tick — chronométrage des gardes et journal de tick. Les échecs restent comptés
      (FM-02), les relevés sur commande restent disponibles. Côté natif, le champ transmis
      n'est toujours lu par rien : les compteurs de la fenêtre de tick servent aussi au
      chien de garde R-707. Vérifié en jeu (serveur de banc, `CommandTiming`) : avec,
      21 séries `rfx.hook.*` et journal écrit par le fil `rustforgex-report` ; sans,
      aucune durée publiée et aucun journal, les 11 commandes sous 5 ms
- [x] Dump d'incident `crash/rfx-crash-<ts>.json` (T-411, ADR-033) — accroche désactivée
      et panic native E-3001 du cycle de tick ; un dump par échec et par partie, seize au
      plus, écrit hors du fil, atomique, anonymisé. Option `diagnostics.report_on_incident`
- [~] T-411, reste : rejeu par `rfx-cli` et champs d'unité de travail (schéma 2, avec le
      déport) ; passage en `HALTED` non observé au moment où il a lieu ; panics hors du
      cycle de tick non remises (ADR-033)
- [~] **`hook_budget_exceeded` vaut 123 sur 4 039 ticks** — première fois qu'il est non
      nul. Budget : 500 µs par accroche (`DEFAULT_MAX_HOOK_NS`), soit ~1 % des appels.
      Deux explications tiennent, et le compteur actuel **ne permet pas de trancher** :
      soit `tick_end` traîne (il parcourt 2 650 unités dans `adapt_probe_levels`), soit
      une pause GC tombe pendant la mesure — `record_hook` mesure du temps écoulé, donc
      attribue les pauses GC à nos accroches. La campagne a compté 1 564 ticks
      contaminés par le GC sur 12 000, ce qui rend la seconde explication très plausible
- [x] Rendre ce compteur diagnosticable (2026-10-29) : dépassements comptés par
      accroche (`rfx.tick.hook_budget_exceeded{hook=begin|phase|end}`), pire durée et
      accroche qui l'a produite (`rfx.tick.worst_hook_ns`, ligne de `/rfx status`).
      Un `tick_end` lent concentre les dépassements sur `end` ; une pause GC les
      répartit au prorata des appels (quatre `phase` par tick). Première observation,
      serveur de banc sans joueur, 859 ticks : **2 dépassements, tous deux sur `end`**,
      pire durée 2,22 ms. Trop peu pour trancher ; à relire sur une campagne
- [ ] Trancher la cause des dépassements sur une campagne (`medium` ou `heavy`) avec
      ces compteurs ; si `end` domine, mesurer `adapt_probe_levels` dans `tick_end`

### Étape H bis — Ramener le coût du sondage sous le budget

État mesuré au 2026-09-09 : **~129 µs par tick** (deux mesures indépendantes à 2 %
l'une de l'autre), contre un budget de 1,5 % du MSPT ≈ **63 µs**. Facteur deux à
trouver.

- [x] **L'horloge lue pour rien** : `System.nanoTime()` remplissait `timestamp_ns`, que
      le natif ne lit jamais. 249 → ~129 µs
- [x] ~~Le niveau COUNTER violait R-700~~ — livré, comptage par lot (commit `7923836`).
- [~] ~~**Le niveau COUNTER viole R-700 — chantier principal.**~~ Pour transporter un
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
- [~] Côté natif : `RecordKind::Enter` fait `calls_this_tick += 1`. Il devra faire
      `+= record.value`. Java écrira toujours une valeur ≥ 1
- [~] Le balayage naïf serait un piège : 2 649 entiers par fil et par tick coûteraient
      plusieurs microsecondes pour rien quand une poignée de sondes seulement est
      appelée. D'où `touched`, alimenté au passage de zéro à un
- [~] R-320 : les deux tableaux sont alloués une fois par fil, jamais dans le chemin
      chaud. T-142 doit rester vert
- [~] Petits gains au passage : `sink` est lu deux fois par appel (`enter` puis
      `record`), une suffit ; `probeId < 0` ne peut pas arriver, les identifiants
      venant de constantes injectées dans le bytecode
- [~] Vérifier en jeu, puis **une** mesure de qualité comparable à la référence
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
- [x] (réglé le 2026-10-29 par le commit porté dans chaque exécution, ci-dessous) **À l'agrégation** : l'agrégateur lit le commit de `HEAD`, qui aura avancé depuis
      le lancement. Agréger depuis un `git checkout 38eacfa` détaché — `benchmarks/runs`
      est ignoré par git et survit au changement — sans quoi le résultat citerait un
      commit dont le code n'a jamais été mesuré
- [x] Faire porter au fichier d'exécution le commit **mesuré**, pour que l'agrégateur
      n'ait plus à le déduire de `HEAD` (2026-10-29) : tâche Gradle `buildStamp`
      (`dev/rustforgex/build.properties`, commit et arbre modifié, sans horodatage),
      champ `build` du fichier d'exécution, agrégateur qui retient le commit des
      exécutions, déclare un mélange et ne retombe sur `HEAD` qu'en le disant. Vérifié
      en jeu : banc court, commit du JAR relu
- [~] **Profils de charge de la PARTIE 22 — la lacune la plus importante de la mesure.**
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
- [~] Lecture des budgets sous charge : « 2 % d'un cœur » → 0,275/50 ms = **0,55 %**,
      tenu avec un facteur quatre de marge. Budget MSPT de 1,5 % → +2,7 %, toujours
      dépassé mais d'un facteur 1,8 au lieu de 4. **Une seule paire**, pas une campagne
- [x] **Mod synthétique de charge écrit** (PARTIE 22, R-870, R-871) :
      `rustforgex-bench-synth`, artefact séparé dans son propre paquet `rfxbench.synth`
      — indispensable, `TargetScanner` refusant `dev/rustforgex/`. Paramétrable en
      unités de travail, coût CPU, allocations, gestionnaires d'événements, fils propres
      et non-déterminisme. Inerte sans propriétés. Chargé par Forge parmi les 272 mods
- [~] Le monde pré-généré avec une usine réelle n'est **pas** la bonne réponse : la
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
- [~] **Mesurer ce que l'armement anticipé coûte au tick** avant d'en faire le défaut.
      ADR-021 a établi que le coût croît avec le nombre de méthodes sondées ; quintupler
      cette population rouvre la question. Paire B/C sous charge `mobs` lancée avec
      `run-macro-prod.sh … mobs true`, à comparer aux +2,7 % p95 de l'ordonnancement
      tardif
- [~] Calibrer les profils sur la table de la PARTIE 22 : `vanilla` 200 entités /
      400 chunks, `light` 10 mods / 500 / 600, `medium` 60 / 2 000 / 1 200, `heavy`
      150 / 8 000 / 2 500. Le profil actuel est en dessous du plus petit
- [~] Monde pré-généré du point 2 de la PARTIE 21.3 : le monde actuel est réutilisé et
      accumule ses régions, ce qui fait dériver les valeurs absolues entre campagnes
- [~] Le modpack d'essai est celui de l'utilisateur : non versionnable, donc non
      reproductible par un tiers
- [~] Côté client : FPS, frametime, 1 % low

**Ce que le niveau A ne mesure pas**, et qu'il ne faut pas lui faire dire : le coût
Java de `RfxProbes.enter/exit` sur le chemin chaud. Un chronométrage Java naïf mentirait
(élimination de code mort), et JMH est une dépendance à peser séparément. T-131 reste
donc ouvert.

### Definition of Done du jalon (CDC PARTIE 29.3)

- [~] T-130..T-134, T-140..T-144, T-150..T-154, T-370..T-373, T-400..T-402,
      T-410..T-412, T-420..T-422, T-440..T-442 verts

      **Inventaire du 2026-10-10** (31 identifiants, lus dans le code et les tests, pas
      exécutés ; quatre statuts recoupés à la main — T-131, T-411, T-422, T-442) :

      | statut | identifiants |
      |---|---|
      | couvert (19) | T-130, **T-132**⁷, T-134, T-141, T-142¹, T-143, T-144, T-371², T-372, T-400³, T-402, T-412⁴, **T-421**⁶, **T-422**⁵, T-440, **T-441**⁸, T-442, **T-401**¹⁰, **T-420**¹¹ |
      | partiel (2) | T-140 (coût tenu sous `medium` et `heavy` synthétique, ADR-038 ; stabilité du classement 0,71-0,85 pour 0,9), **T-411**⁹ (dump des incidents existants ; rejeu par `rfx-cli` attendu avec le déport, ADR-033) |
      | absent (6) | T-133 (retransformation écartée par ADR-016), T-150..T-153 (proxy de C-06 non écrit), T-373 (arènes non implémentées) |
      | bloqué (4) | T-131 et T-154 (micro-mesure JMH à arbitrer), T-370 (test long T-700), T-410 (C-19..C-27 n'existent pas) |

      Réserves des « couverts » : ¹ chemin chaud Java sans compteur d'allocation ;
      ² passage en `DEGRADED` non testé ; ³ balayage statique sans `src/launch` ;
      ⁴ pas de test du rapport de bout en bout ; ⁵ `RfxCommandsTest` (2026-10-10) parcourt
      tout l'arbre de `/rfx` : chaque commande répond en texte traduisible, toutes sont
      refusées au niveau 2 et exécutées aux niveaux 3 et 4 ; vérifié en retirant
      l'exigence de permission (le test échoue) ; ⁶ `CommandTiming` (serveur de banc, RF-X
      actif, chaque commande 20 fois) : d'abord **2 dépassements** — `/rfx report`
      écrivait sur le fil du serveur (7,6 ms), et le premier appel payait le chargement
      des classes (6,3 ms pour `status`). Écriture passée hors du fil, préchauffage au
      démarrage (16 ms) ; ensuite 0 sur 200. Marge mince : premier `status` à 3,9-4,5 ms.
      Au passage, rapports nommés à la milliseconde : à la seconde, ils s'écrasaient ;
      ⁷ défaut trouvé en écrivant le test : une injection qui levait en cours de route
      laissait la méthode à moitié injectée dans une classe chargée quand même. Injection
      désormais atomique par méthode (copie, remise en place, compteur
      `unprobeableMethods`) ; test qui échoue si la remise en place est retirée ;
      ⁸ sentinelle à fil-piège dans un paquet inventorié, et compteur de classes chargées
      de la JVM inchangé pendant une découverte ; un `Class.forName` injecté dans la
      découverte fait échouer le test (2 classes chargées) ;
      ⁹ `IncidentRecorderTest` (6 tests) : dump unique, JSON valide, versions, chaîne des
      causes dont la première trame désigne l'origine, chemins anonymisés. Trois mutations
      vérifiées : sans remise de l'incident par la garde, sans remise des codes par le
      cycle de tick, sans anonymisation des messages — chaque fois un test échoue.
      Serveur de test lancé : versions lues en jeu, aucun dump intempestif ;
      ¹⁰ `run-telemetry.sh` (ADR-034) : télémétrie mesurée en place sur le serveur de banc,
      5 exécutions × 6 000 ticks après 3 600 d'échauffement, part native par
      micro-benchmark ; pire rapport 0,117 % du MSPT pour un seuil de 0,2 % ;
      ¹¹ ADR-035 : à M1, T-420 porte sur les commandes dont le composant existe (`status`,
      `mods`, `top`, `profile`, `discover`, `report`, `panic-test`) ; `why`, `workload`,
      `mode`, `set`, `disable`, `enable`, `bench`, `validate`, `cache` viendront avec leur
      jalon. `panic-test` ajoutée (mode `debug`, 20/20 panics confinées en jeu, incident
      consigné). Commandes mutantes journalisées avec auteur et horodatage (R-600).
      Serveur de banc : 220 exécutions, aucune au-delà de 5 ms ; le premier appel lent
      venait de la première commande du serveur (`seed` vanilla : 4,85 ms).

      Deux décisions dépassent l'assistant : arbitrer une dépendance de micro-mesure
      (JMH, T-131/T-154), et T-410 qui exige des composants d'un jalon ultérieur — à
      trancher par ADR (report du test, ou découpage de la fiche au bloc MEASURED).
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
- [~] **La marge sur la lecture MSPT est mince et la mesure est faible** : 6 cycles
      positifs sur 10, queue négative à −1,14 ms. ADR-024 dit qu'une telle médiane est
      biaisée vers zéro — le coût réel pourrait dépasser 275 µs. La lecture « part d'un
      cœur » garde, elle, un facteur 4 et tient même si le chiffre était sous-estimé de
      moitié
- [x] Le profil `heavy` (8 000 entités) **tue le serveur de banc**, sauvegarde coupée
      ou non, **avec ou sans RUSTFORGE-X** (vérifié le 2026-10-12 : même pile, collisions
      dans le fil `overworld` de DimThread). Mesuré à la place sur un serveur synthétique,
      option 1 de la PARTIE 22 (ADR-036)
- [x] **Échantillonnage réglé sur son coût et classement sur fenêtre longue**
      (ADR-037) : sous `heavy`, cadence 69 → 16 Hz, corrélation de rang 0,126 → **0,838**
      (14 unités communes sur 20)
- [~] **T-140 échoue encore sous `heavy`** : coût relevé 1,45 et 1,93 ms par tick malgré
      la cadence divisée par cinq — l'attribution à l'échantillonnage d'ADR-036 est
      **retirée**. Corrélation 0,838 pour 0,9. Hypothèse à éprouver : sondes sur des
      méthodes vanilla appelées en masse (`BlockCollisions#computeNext` en tête) ; le
      relevé porte désormais `calls_per_tick_x100`. Au moins trois exécutions par
      configuration avant d'écrire une cause : la ligne de base varie de 0,69 à 1,95 ms
      entre exécutions identiques
- [x] **Coût sous `heavy` attribué, 3 exécutions contre 3** (ADR-037, 2026-10-14) : les
      sondes sur Minecraft et Forge. Médiane 1,44 ms par tick avec elles, 0,22 ms sans
      (`platform_targets=false`), séparation complète ; `BlockCollisions#computeNext` à
      8 187 appels par tick
- [x] **Plafond par sonde** (ADR-038) : au-delà de 1 000 appels par tick, une sonde
      s'éteint seule pendant 1 200 ticks. Sous `heavy`, 3 exécutions : coût **0 / 0,34 /
      0 ms** par tick (contre médiane 1,44 ms sans plafond), 15-16 sondes plafonnées,
      gouverneur jamais bridé. **Critère de coût de T-140 tenu sous `heavy`**
- [~] T-140, stabilité du classement : 0,714 / 0,848 / 0,798 selon la paire, pour 0,9.
      Part due au jeu (IA aléatoire de 8 000 entités) non mesurée
- [x] Rejoué `medium` sur le modpack réel (ADR-038, 2026-10-15), 3 + 3 exécutions :
      part d'un cœur tenue (≤ 1,6 %), part du tick **non tenue** (1,7 – 7,0 % pour 1,5 %,
      MSPT de 13 – 17 ms), gouverneur bridé dans les 6, classement presque vide.
      L'échantillonnage réglé coûte moins que la cadence fixe (0,40 contre 0,74 ms)
- [x] Règle de budget tranchée par Killian (option 2) : plancher de 0,3 ms sur le
      budget de tick (ADR-039). Sous `medium` réel : bridé 1 fois sur 3 au lieu de 6 sur 6
- [~] **Classement vide sous `medium` réel : 99,3 % des prélèvements hors sonde.**
      61 % pendant que le serveur attend le tick suivant (`MinecraftServer#m_5667_`),
      32 % pendant qu'il attend les fils de dimension d'un mod de tick parallèle
      (`IntLatch#waitUntil`). Correctifs proposés : A, ne prélever que fenêtre de tick
      ouverte ; B, échantillonner aussi les fils qui exécutent du code sondé pendant le
      tick (génériquement, sans nommer de mod — INV-12)
  - [x] A, prélever pendant la phase de jeu seulement (ADR-040, `2cb5cbc`) : plus aucune
        pile dans `m_5667_`, mais 97 – 98 % des piles du tick dans `IntLatch#waitUntil`,
        classement 0 / 1 / 0. Coût relevé plus haut (0,49 – 1,30 ms), bridé 3 sur 3,
        **cause non établie** (moins de piles pendant le tick qu'avant)
  - [x] B, prélever aussi les fils actifs pour le tick (`9207aad`) : 9 400 – 11 200
        prélèvements attribués (contre 9 – 15), 9 – 14 unités classées, mais la tête du
        classement est faite de fils de fond **endormis** dans une méthode sondée
        (corrélation 0,361). Coût 0,39 – 1,09 ms, bridé 3 sur 3
  - [x] Filtre des fils qui attendent (`105725d`, ADR-041) : 534 – 1 046 attributions
        seulement, 3 – 7 unités, corrélation −0,647. Le travail actif passe hors sondes
  - [~] Contrôle sans échantillonnage (`t140-k/nosampling`) : **invalide**, Killian
        jouait sur la machine de 03:30 à la fin (idem exécution 3 de `t140-k/idle`)
  - [~] **Décision de Killian** (ADR-041, Conséquences) : échantillonnage réservé à
        `DEEP`, part de budget abaissée, ou classement `medium` par les seules sondes
        chronométrées. T-140 sous `medium` réel : non tenu
  - [~] Pourquoi le contrôle n° 3 finit à `OFF` : non examiné
- [~] Vérifier sous `medium` (modpack) que la cadence reste à 100 Hz (ADR-037)
- [~] Aucun test de gameplay ne diverge de la référence non instrumentée

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
      - [x] **G-06 passe en égalité stricte** (ADR-032) : circuit de redstone sur 10 000
            ticks, 21 empreintes identiques dans les trois exécutions, mêmes compteurs
            d'activité (anneau 3 500 changements…), mêmes erreurs
      - [x] **G-12 passe** (ADR-032) : conteneurs en égalité stricte ; 17 549 recettes
            d'atelier, 87 changent à chaque lancement chez les références comme chez
            RF-X (Blue Skies, Sophisticated Storage), aucune n'est imputable
      - [x] **G-14 passe en égalité stricte** (ADR-032) : météo pilotée par 21
            événements planifiés, cycle jour/nuit, 23 empreintes identiques
      - [x] **G-11 passe** (ADR-032) : 24 commandes vanilla exécutées en égalité
            stricte ; 263 commandes enregistrées éprouvées sans exécution, deux varient
            à chaque lancement chez les références (Data Planets, ZPS)
      - [x] **G-04 passe** (ADR-032) : 30 sauts Overworld, Nether, End ; trois
            voyageurs en égalité stricte, le cochon bruité par les mods chez les
            références comme chez RF-X
      - [~] G-04, point ouvert : un saut vers l'End a été refusé de façon intermittente
            (2 exécutions sur 8, dont une référence) ; cause non lue, journalisée depuis
      - [x] **G-09 passe** (ADR-032) : fenêtre de chunks rapide, ~1 200 chargements et
            déchargements ; état strict au-dessus de Y = 160 ; critère des erreurs
            rendu symétrique
      - [~] G-09, à surveiller : entités de bloc, candidat intrus 7 fois contre 1 et 0
            (p = 0,003, sous le seuil corrigé de 0,00067 sans l'atteindre)
      - [x] **G-07 passe en égalité stricte** (ADR-032) : Create et Mekanism sur
            30 minutes, mêmes produits dans les trois exécutions
      - [~] **G-13 non concluant** (ADR-032) : divergence A/B au relevé 3, mais l'A/A
            sans RF-X produit le même signal — le test de l'intrus suppose des éléments
            indépendants, une foule le fausse. Refaire sans interactions (couloirs
            séparés) ou avec un jugement qui tienne compte de la dépendance
      - [~] Les autres scénarios G-02, G-05, G-10, G-15 ;
            G-02, G-10 et G-15 demandent des clients
- [~] JAR installable et jouable, client et serveur dédié.
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
- [~] **Le test client reste faible et la case reste ouverte.** L'environnement de
      développement ne charge que **trois mods** : la ligne d'énumération dit
      `0 écartées, 0 référençant un type absent`. Le risque signalé n'est donc **pas
      exercé** — côté client, `dedicatedServer=false` n'écarte plus les classes qui
      référencent `net.minecraft.client.*`, et sur un vrai modpack ce sont des dizaines
      de milliers de classes supplémentaires qui deviennent candidates
- [~] La vraie épreuve est une instance CurseForge cliente avec son modpack. Elle touche
      les installations de l'utilisateur : à faire avec son accord explicite, pas d'office

---

## M2 — Analyse et ordonnancement ✅ CLOS AVEC RÉSERVES (2026-10-30, ADR-065)

Réserves (ADR-065) : T-190, T-194, T-262, T-267, T-212 ; précision de C-11 reportée à M3.

**Livrable** : workloads modélisés, task graph, scheduler opérationnel utilisé
uniquement pour des tâches internes (analyse, persistance). **Aucune transformation du
jeu.**

**Composants** : C-07 (Call Graph), C-08 (Workload Profiler), C-09 (Workload DB),
C-10 (Dependency Analyzer), C-12 (Thread Analyzer), C-16 (Task Graph), C-17 (Scheduler),
C-18 (Worker Runtime), C-48 (Persistance / diagnostics).

**DoD** : T-160..T-164, T-170..T-172, T-180..T-184, T-190..T-194, T-210..T-212,
T-250..T-253, T-260..T-267, T-270..T-273. Sur 37 tests, seuls T-184, T-262 et T-267
demandent une mesure ; les autres sont des tests de justesse.

- [x] Lire les fiches de C-16, C-17, C-18 (PARTIE 5) et leurs IF/DM/INV avant tout code
- [x] C-16 Task Graph — T-250..T-253 (INV-01 : acyclique) : crate `rfx-taskgraph`,
      12 tests, chacun éprouvé par un défaut volontaire (ADR-043)
  - [x] Mesurer l'acceptance : 500 nœuds construits en moins de 100 µs — **68 à 69 µs**
        (`cargo bench -p rfx-bench --bench scheduler`, 2026-10-16, trois exécutions,
        machine chargée à 42-68 % par Firefox, Spotify, Discord, fond animé)
  - [ ] Publier `rfx.taskgraph.*` dans la télémétrie quand un graphe sera construit en jeu
  - [ ] Miri sur `rfx-taskgraph` (PARTIE 19.1)
- [x] C-17 : dépendance `crossbeam-deque` décidée (ADR-044), inscrite au SBOM
- [~] C-17 Scheduler — T-260..T-267 (INV-06, INV-08, R-740). Crate `rfx-scheduler` :
  - [x] Pool, files par priorité, vol de travail, endormissement sans réveil perdu,
        contre-pression `E-2401`, fenêtre critique (R-423), panic confinée (R-524),
        `FORCE_SERIAL`. T-260 (10^7 tâches), T-261, T-266 et 6 autres tests
  - [x] Exécution d'un `TaskGraph` par compteurs de dépendances (`run_graph`) :
        R-740 (mêmes résultats en série et avec 1, 2, 4, 16 workers, avec ou sans fusion
        et découpage), échec propagé aux dépendants, drain à l'échéance sans attendre
        (INV-08). Le cahier numérote T-263 à la fois R-740 (PARTIE 21) et « annulation
        effective » (5.17) : les tests portent le nom de l'exigence, pas ce numéro
  - [x] Annulation coopérative (T-263 : une tâche en cours s'arrête quand on annule son
        jeton), `rfx.sched.deadline_misses`. Abandon après l'échéance : `run_graph` rend
        la main et ignore les résultats tardifs (INV-08)
  - [ ] Délai de grâce de 2 ms et rétrogradation du travail (5.17.7) : la rétrogradation
        relève de C-15 (M3) ; le délai de grâce sera câblé avec lui
  - [x] Parallélisme imbriqué (5.17.9) : `TaskContext::spawn` / `join`, profondeur 4
        (`max_nesting`), au-delà exécution en ligne ; en attendant, un worker n'exécute que
        des tâches plus profondes, de sa propre file (pile bornée, pas d'interblocage).
        T-264 : arbres de profondeur 10 sur 1, 2, 4 workers et en série
  - [x] Sur-souscription (R-421, FM-25, ADR-045) : `report_external_load`, workers actifs
        divisés par deux au-dessus du seuil, +1 par relevé en dessous, série après huit
        relevés à un worker. T-265 (4 tests, charge simulée)
  - [x] Capteur réel côté Java (ADR-046, décision de Killian) : fils tiers ayant consommé
        ≥ 10 % d'un cœur depuis le relevé précédent, fil `RFX-LoadSensor`, une fois par
        seconde, entrée FFI `rfx_sched_report_external_load`. Le compte des états
        `RUNNABLE` relevait 49-50 sur le modpack (≈ 46 fils en attente native, vus au
        `jcmd`) ; le temps CPU relève 3 (max 12). Vérifié en jeu, 299 relevés
  - [x] Ordonnanceur démarré par le runtime (ADR-061, choix de Killian du 2026-10-29) :
        relevé de charge transmis, fenêtre critique ouverte pendant le tick, base des
        unités de travail (relecture, décodage parallèle, écriture) en tâches
        `BACKGROUND` soumises en détaché, jamais en ligne sous le verrou ; fil `RFX-IO`
        gardé en secours. Métriques `rfx.sched.*` du pool. Vérifié en jeu : 14 workers,
        `workload.db` relue par le pool. 14 mutants tués sur 15 (la relance pendant le
        retrait n'est pas testable de façon déterministe, consignée)
  - [x] Famine (FM-24) : pendant `run_graph`, le fil qui attend exécute lui-même le travail
        `CRITICAL` resté en file plus d'une milliseconde, et rien d'autre ; attente par
        `yield` (un sommeil d'1 ms peut en durer 15 sous Windows). `famine_escalations`
  - [ ] T-267 (soumission < 200 ns) **non tenu** dans ces mêmes conditions : 243 à 274 ns
        avec un jeton d'annulation neuf par tâche, 204 à 215 ns avec un jeton partagé
        (l'allocation du jeton pèse ~50 ns). Supprimer les réveils quand un worker cherche
        déjà n'a rien donné de mesurable (±5 %, le bruit) : retiré. À remesurer machine au
        repos avant toute autre conclusion
  - [ ] T-262 (échéances tenues à 99 % sous `heavy`) : demande l'ordonnanceur branché en jeu
- [x] C-18 Worker Runtime — T-270..T-273 (ADR-047) : arène `rfx_memory::Arena` sur
      `bumpalo`, allocations faillibles seulement, une par fil et par niveau d'imbrication,
      remise à zéro après chaque tâche. T-270, T-271, T-273 (0 allocation système sur
      10 000 tâches) ; T-272 porté par T-263. Piège de `bumpalo` 3.20 trouvé et contourné :
      un premier bloc au-dessus du plafond désactive le plafond
  - [x] Faire entrer les arènes dans le budget de mémoire native (ADR-062) : arènes
        taillées pour que leur pire cas borné tienne dans un quart du budget (R-533 prime
        sur le défaut de 16 Mio), réservé avant le démarrage ; refusé, l'ordonnanceur
        reste éteint et la base garde son fil de secours. 4 mutants sur 5 tués (la
        libération après un échec de démarrage des fils n'est pas provocable en test)
- [~] C-09 Workload DB et C-48 Persistance — crate `rfx-cache` (ADR-048), 21 tests,
      seize défauts volontaires détectés :
  - [x] `Fingerprint` (DM-05) dans `rfx-model` : environnement et unité de travail
  - [x] Base indexée par identifiant et par propriétaire, historique borné à 32,
        éviction des plus froides puis des moins récemment vues (T-183)
  - [x] `workload.db` : en-tête, somme xxh3, tout ou rien ; autre environnement rejeté
        sur l'en-tête, entrée au code changé écartée (T-180, T-181) ; fichier illisible
        ou de schéma inconnu mis de côté en `.bak` (T-182)
  - [x] Fil `RFX-IO`, file de 4 sans attente, écriture atomique, rotation par âge puis
        taille ; disque plein → persistance désactivée, jeu inchangé (T-184, T-500 à
        T-502, sur un système de fichiers simulé et un vrai répertoire)
  - [x] Écarts à IF-12 (ADR-048, décision 7) **validés par Killian le 2026-10-29** :
        `hot()` en `Vec`, `evict` par plafonds, `persist` par le fil d'écriture,
        `LoadOutcome`, instantané complet plutôt qu'« append-only + index »
  - [x] Brancher dans `rfx-core` (ADR-049, demande de Killian du 2026-10-16) :
    - [x] `rfx-cache` : la base vit sur `RFX-IO` (relecture au démarrage sans attente,
          lots de nouveautés et de dynamiques, écriture si quelque chose a changé ;
          disque refusé → la base continue en mémoire sans écrire)
    - [x] `rfx-model` : `DbOpen` (chemin et environnement venus de Java), `DbStatus`,
          options `cache.enabled` et `cache.max_memory_mb` (PARTIE 28)
    - [x] `rfx-core` : empreinte d'environnement, descripteurs gardés à
          l'enregistrement, collecte des dynamiques par tranches (le verrou rendu entre
          deux tranches : le tick n'attend jamais plus d'une tranche), mémoire comptée
          dans le budget natif (`Pool::Cache`)
    - [x] `rfx-ffi` : `rfx_db_open`, `rfx_db_sync` ; pas d'entrée de fermeture :
          `rfx_shutdown` ferme la base hors du verrou global, attente bornée à 10 s
    - [x] Java : ouverture au chargement complet (l'empreinte a besoin de
          l'inventaire des mods ; l'étape 9 du Bootstrap le dit), fil `rustforgex-db`
          hors tick toutes les 5 min, télémétrie `rfx.db.*`, `CONFIGURATION.md`
    - [x] Lancé en jeu (2026-10-17, `runGameTestServer` ×2) : base écrite à l'arrêt
          (390 478 octets), « 413 unités relues au lancement » au second, aucun
          avertissement. Le serveur de test ne vit qu'un tick : les mesures en jeu
          restent à voir sur une vraie partie ; un test de bout en bout de 40 ticks
          les prouve en attendant (160 appels relus du fichier)
    - [ ] Voir les mesures d'une vraie partie arriver dans `workload.db` (client ou
          serveur joué plus de cinq minutes)
  - [x] Acceptance (ADR-048, 2026-10-16, cas extrême : noms longs, historique plein,
        machine chargée à 43-58 % par d'autres programmes) : **46,6 Mio** pour 20 000
        unités (allocateur instrumenté, < 64 Mio) ; chargement **117,7 ms** (< 200 ms,
        `cargo bench -p rfx-bench --bench cache`). Le premier jet ne tenait ni l'un
        (64,1 Mio : enregistrements en ligne dans un `Vec` qui double) ni l'autre
        (423 ms : historique en CBOR nommé, décodage en série) ; corrigés par `Box`,
        historique en entiers et décodage parallèle par blocs
- [x] C-07 Call Graph — T-160..T-164 (R-340, R-341, FM-15), ADR-050. Ordre retenu :
      C-07 d'abord, C-10 s'appuie sur son graphe, C-08 orchestre les analyses
  - [x] `rfx-bytecode` : lecture d'un `.class` (JVMS §4) sans dépendance, jamais de
        panic sur une entrée malformée ; sites d'appel `invoke*`, méthodes d'amorçage
  - [x] `rfx-analyzer` : index des classes chargées, résolution statique, monomorphe,
        polymorphe bornée (≤ 4), lambda, réflexion → non résolue, Tarjan, bornes
        (`max_call_depth` 12, `max_nodes_per_workload` 2000 → `truncated`)
  - [x] Classes de test compilées par `javac` depuis des sources du dépôt (aucune
        classe tierce redistribuée)
  - [x] Brancher en jeu (ADR-051, demande de Killian du 2026-10-17) : fil
        `rustforgex-analysis`, hiérarchie complète des couches de modules avant tout
        pas, bytecode à la demande, cibles de mixin / access transformer / coremod
        tenues pour inconnues, `rfx.callgraph.*`. Vérifié sur `runServer` : 62 690
        classes déclarées en 67 à 94 s, fil mesuré entre 2,7 et 4,6 % d'un cœur
        (budget 5 %) ; sans charge, aucune unité chaude, donc aucun graphe
  - [x] Coût d'un graphe mesuré (ADR-052, `rfx-bench callgraph`) : sous-types
        mémorisés, médiane 119,1 → 23,2 s pour 32 graphes, graphes identiques
  - [x] Seconde étape (ADR-052) : appel redéfinissable par `Object` mégamorphe,
        sous-types empruntés ; 32 graphes en 1,87 s (médiane), graphes identiques
  - [x] (ancien libellé) Mesurer le coût d'un graphe sur des classes réelles, et en isoler la
        cause avant d'optimiser (ADR-051, « Vérification en jeu »)
  - [x] (ADR-059) Avant que C-10 ou C-08 utilisent les graphes : réexaminer les risques
        résiduels de l'ADR-051 (classes générées, coremods à noms calculés,
        transformateurs tiers)
  - [x] Acceptance (ADR-053, demande de Killian du 2026-10-17) : 85,2 % des sites des
        200 méthodes de référence résolus ; 26 033 sites résolus confrontés à la JVM,
        0 faux résolu, 0 invérifiable. L'oracle a d'abord trouvé 9 faux résolus,
        corrigés (troncature, résolution JVMS)
- [~] C-08 Workload Profiler — T-170..T-172 (R-350, R-351, FM-16), ADR-054. Demande de
      Killian du 2026-10-17. C-10 à C-13 absents : leurs faits sont `None`, donc
      `UNKNOWN` (P-02) ; le graphe de C-07 ne sert qu'à compter non résolus et
      troncature, qui ne peuvent qu'aggraver (risques de l'ADR-051 sans effet tant
      qu'aucun drapeau permissif ne peut être posé)
  - [x] `rfx-model` : DM-06 (domaines, précision), DM-07, DM-08, DM-09, DM-10, DM-11,
        DM-12 ; règles de cohérence T-011
  - [x] `rfx-analyzer` : `classify` pure (R-351), confiance (R-218) ; T-170 table de
        vérité complète (225 000 combinaisons, attentes recalculées à part)
  - [x] `rfx-core` : descripteurs, sélection par score, troncature par épuisement des
        tours (FM-16, T-171), recalcul sur changement de `bytecode_hash` (R-350,
        T-172), changements significatifs comptés ; mutants tués
  - [x] Métriques `rfx.analysis.*` (queue_depth, completed, truncated, cpu_ms, …),
        télémétrie Java, BENCHMARKS.md ; `rfx.callgraph.queue` renommée
        `rfx.analysis.queue_depth`
  - [x] Build, tests, jeu lancé (`runGameTestServer`, aucune erreur). Sans charge, le
        serveur de développement n'a pas d'unité chaude : les descripteurs ne se
        voient pas en jeu ; la chaîne profileur → descripteur → statut est vérifiée
        par le test d'intégration du runtime
  - [x] Persistance des descripteurs dans `workload.db` (ADR-064, choix de Killian du
        2026-10-29) : faits propres à l'unité, tampon du code d'analyse, schéma 2 qui
        lit le schéma 1, circuit dans `rfx_analysis_step` ; drapeaux et confiance
        reclassés quand une unité décrite est observée (un descripteur relu restait
        sinon « jamais observé »). Test de bout en bout par un vrai fichier
  - [ ] Acceptance de C-08 (attend C-10 à C-13 en jeu, profil `heavy`)
- [~] C-11 Data Flow Analyzer — T-200..T-205 (R-380..R-382, FM-20), ADR-055. Avant
      C-10, qui a besoin de ses ensembles d'accès (demande de Killian du 2026-10-17,
      « ok go la suite »)
  - [x] `rfx-bytecode` : décodeur d'instructions complet (effet sur la pile, champs,
        branchements, table des exceptions) ; `jsr`/`ret` => non analysable —
        599 749 méthodes du corpus réel rejouées à la hauteur exacte du compilateur
  - [x] `rfx-analyzer` : interprétation abstraite intraprocédurale (sources des
        références, point fixe, gestionnaires d'exceptions) — 599 749 méthodes du
        corpus analysées, aucune incohérente ; 10 mutants sur 10 tués
  - [x] Effets : champs, statiques, tableaux, verrous, exceptions ; table domaine
        dérivée de la hiérarchie ; objets locaux non échappés — 26 ancres toutes
        présentes dans le corpus ; 12 mutants sur 12 tués
  - [x] Interprocédural : résumés des appelées, arguments, récursion, UNKNOWN (R-380)
        — 9 mutants sur 9 tués ; 19 unités `EXACT` sur 200 méthodes de référence
  - [ ] Réduire l'inconnu, par ordre de poids mesuré : appels par interface (12 746
        arêtes sur 200 graphes), bornes du graphe (98 tronqués), cibles sans code.
        Chaque signature connue est une affirmation à prouver (ADR à écrire).
        Mesure par unité (ADR-055, 2026-10-29) : les cibles sans code (174 natives du
        JDK) ne sont la seule cause d'**aucune** unité, écartées ; `clone()` de tableau
        rapporterait 5 unités en écritures seulement, non retenu. Fermeture
        conditionnelle choisie le 2026-10-29 puis **réfutée par la mesure** avant tout
        code : Guava, JNA, commons-lang, jline et netty fabriquent des `Proxy`
        d'interfaces quelconques, créés à l'exécution après les graphes (ADR-055).
        **Reporté à M3 par Killian le 2026-10-30** : C-11 reste sûr et imprécis en M2 ;
        la voie des cibles observées s'appuiera sur la vérification dynamique (R-382)
        que M3 construit pour valider les déports
  - [x] Branchement dans C-08 (`Facts.reads`, `writes`, `side_effects`), métriques
        `rfx.dataflow.*` — reste à observer en jeu sous charge (aucune unité chaude
        dans un serveur sans joueur)
  - [ ] Acceptance : corpus annoté de 150 méthodes, zéro sous-approximation
        d'écriture, ≥ 60 % EXACT ; vérification dynamique (R-382, T-205) à part
- [x] C-10 Dependency Analyzer — T-191..T-193 (R-370, R-372, FM-19), ADR-056 : dépendances de données au niveau des domaines, toutes les unités décrites supposées
      du même tick ; `data_deps` remplies, `order_deps` absentes faute de C-06 ; 7 mutants sur 7 tués. T-190 attend des régions (C-11), T-194 la planification
      réelle (C-16, C-17)
- [x] C-12 Thread Analyzer — T-210, T-211 (R-391), ADR-057 : analyse statique, confinement
      dérivé des classes (champs lus par `rfx-bytecode`), R-390 au pire cas faute
      d'observation des rôles de fil ; 13 mutants sur 13 tués. T-212 couvert par la
      classification ; l'observation réelle demande que les sondes portent le rôle de fil
      (DM-02, changement d'interface à décider avec Killian)
- [x] C-13 Determinism Analyzer — T-220, T-221 (R-217, R-401), ADR-058 : recherche
      statique des sources (appels écrits, domaines de C-11, méthodes natives,
      réflexion, graphe incomplet), sans shadow run (`verified_runs = 0`) ; C-12 reçoit
      ses faits (`main_thread_required = NO` atteint par une unité pure) ;
      `external_locks` connu (vide) sur un graphe complet sans verrou ; 26 mutants sur 26
      tués. Pas encore observé en jeu : le serveur GameTest s'arrête avant le premier
      graphe. T-222 attend le moteur parallèle, T-223 C-21
- [x] `order_deps` (C-10, étape 2 : même flux d'événement, l'un peut annuler ou modifier
      l'événement) — dernier fait qui manque à C-08 : sans lui, tout descripteur reste
      `UNKNOWN`. Voie choisie par Killian le 2026-10-18 : « Revue ADR-051 puis statique »
  - [x] Revue des angles morts de l'ADR-051 (ADR-059) : trois angles morts de plus
        trouvés (greffons de lancement de Forge, fichiers de données des coremods —
        défaut réel, 22 cibles de `method_redirector.js` manquées —, redéfinition des
        méthodes de paquet). Parades : `@OnlyIn` et méthodes de paquet non résolus,
        classes réécrites par Forge absentes, `@ObjectHolder` sans `final` ; l'analyse
        ne démarre pas devant un service de transformation tiers, des données de
        coremod non nommées ou une classe du jeu qui définit des classes. 17 + 4
        mutants tués. Le jeu a pris en défaut la première liste de services (Mixin) ;
        corrigé. Non observé en jeu : la fin du relevé (le serveur GameTest s'arrête
        avant)
  - [x] Règle statique prudente (ADR-060) : `order_deps` liste les partenaires WAW,
        WAR et verrou ; inconnu pour un rappel possible (racine d'instance, paramètre
        objet ou tableau, synthétique, désignée par une poignée de sa classe, inconnue)
        ou des accès inconnus. Limite actée : la dépendance de contrôle (C-10 étape 3)
        n'a pas de champ dans DM-08, R-331 reste à la charge du consommateur.
        Premier descripteur hors `UNKNOWN` (`Chance.pure`, drapeau `PURE`)
  - [x] Tests, 13 mutants sur 13 tués (un 14ᵉ équivalent : code retiré)
- [x] Banc : chaque exécution consigne la charge de la machine hors serveur et se
      déclare faussée au besoin (ADR-042, « Bancs réalistes ») : relevé par seconde
      pendant la fenêtre (`MachineLoad`, fil `RFX-BenchLoad`), champ `machine` du
      fichier d'exécution, non-conformité au-delà de 10 % au p95 (seuil de départ, non
      mesuré) ou sans relevé. Vérifié en jeu : banc court, 13 relevés, 27,5 % en médiane
      et 70 % au p95 sur la machine de développement occupée

---

## M3 — Décision

**Livrable** : décisions calculées et expliquées, appliquées uniquement pour TL0..TL2
(`JAVA_ONLY`, `JAVA_BATCH`).

**Composants** : C-11 (reste), C-13 (reste), C-14, C-15, C-32, C-33, C-43.

**DoD** : T-200..T-205, T-220..T-223, T-230..T-232, T-240..T-245, T-380..T-384,
T-390..T-392, T-460..T-461.

**Sortie** : premiers gains réels par batching, `/rfx why` complet.

- [ ] Lire les fiches de C-14, C-15, C-32, C-33, C-43 (PARTIE 5) et leurs IF/DM/INV,
      puis ce qui reste de C-11 (T-200..T-205) et de C-13 (T-220..T-223), avant tout code
- [ ] Plan de M3 : ordre des composants, ADR d'entrée, ce qui demande Killian.
      Ordre retenu : C-14 (pur, sans dépendance), puis C-33 (règles de compatibilité,
      que le filtre de C-15 lit), C-15 limité à TL0..TL2, C-43 (`/rfx why` complet),
      C-32, et ce qui reste de C-11 et C-13
- [x] C-14 Risk Engine — T-230..T-232 (ADR-066) : crate `rfx-decision`, quatre
      composantes, R1..R10 cumulées, R10 toujours plancher (seule lecture monotone),
      blast radius défini prudemment ; `TransformationLevel` (DM-13, partiel) dans
      `rfx-model`. Table de vérité et monotonie sur les 2 048 combinaisons de onze faits.
      4 mutants sur 4 tués, dont la lecture « R10 seulement sinon », qui survivait
      d'abord : la confiance basse du test ne dépassait aucun plancher
  - [ ] Brancher : risque de chaque descripteur, `rfx.risk.*`, bloc `DECISION` de
        `/rfx why` (avec C-15)
- [~] C-33 Compatibility Engine — T-390..T-392 (ADR-067)
  - [x] Moteur pur dans `rfx-decision` : six verdicts ordonnés par le niveau qu'ils
        autorisent, la source la plus prioritaire qui s'exprime décide, le plus
        restrictif au sein d'une source ; sélecteurs `modid` / `modid:Classe#methode` ;
        quarantaine datée levée au changement d'empreinte ; test statique sans nom de mod.
        4 mutants sur 4 tués (la réflexion seule n'était pas testée, ajouté)
  - [ ] Section `[overrides]` de la configuration (Java, natif, `CONFIGURATION.md`)
  - [ ] Quarantaines : création sur incident, persistance dans `compatibility.db`
  - [ ] Propriétés détectées depuis l'analyse (cibles de transformateurs, réflexion)
  - [ ] `rfx.compat.*`
- [~] C-15 Decision Engine — T-240..T-245 (ADR-068)
  - [x] Moteur pur dans `rfx-decision` : filtre dur avec motifs DM-15 (huit genres),
        gain = temps économisé `coût × (1 − 1/speedup)` (seule lecture compatible avec
        R-410 : sans mesure, rien ne s'active), pénalité de risque, politique des modes,
        plafond TL2 du jalon, stabilité 600 ticks et verrouillage FM-21. DM-13
        (`StrategyKind`) et DM-15 (`RejectReason`) dans `rfx-model`. Filtre exhaustif sur
        34 560 cas × 8 genres ; 5 mutants sur 5 tués (la pénalité de risque n'était
        testée qu'à l'égalité, resserré)
  - [x] Bloc `DECISION` de `/rfx why` et `/rfx workload` : risque, verdict et décision
        calculés à la demande (`rfx-core::explain`, premier pas de C-43), genres rejetés
        avec leurs motifs, « non appliquée (M3) ». Le verdict ne voit que la réflexion
        tant que règles utilisateur, quarantaines et transformateurs concurrents ne sont
        pas branchés (sans effet en M3 : le plafond TL2 vaut `FORCE_SERIAL`)
  - [ ] Décision tenue par unité, `E-2201`, `rfx.decision.*`, `rfx.risk.*`
  - [ ] Mesure du speedup de `JAVA_BATCH` : l'exploration des modes (2 à 5 %), avec
        l'application des décisions
- [ ] Précision de C-11 sur les appels par interface (reportée de M2, ADR-055) : cibles
      observées sous la vérification dynamique de R-382

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
- ~~**Remote git**~~ — remote `origin` sur GitHub (`JLSkyzer/RustForgeX`), CI à chaque
  poussée.
- ~~Écarts à IF-12, persistance des descripteurs, voie de C-11, Miri~~ — **décidés le
  2026-10-29** : écarts validés ; descripteurs persistés dans `workload.db` avec
  migration ; Miri installé. La fermeture conditionnelle des interfaces, retenue, a
  été réfutée par la mesure avant tout code (ADR-055) ; le 2026-10-30, Killian reporte
  la précision de C-11 à M3.

---

## M11 — Refonte du moteur de rendu (après la 1.0.0)

Jalon ajouté au CDC le 2026-10-03 par décision de Killian (ADR-031, PARTIE 30). Il ne
commence qu'une fois la 1.0.0 publiée ; rien de ce qui suit ne doit infléchir M0 à M10.

- [ ] Spécifier le jalon par un ADR avant tout code : composant, exigences, tests,
      Definition of Done. Point de départ : `docs/design/C-53-exploration-rendu-vulkan.md`.
- [ ] Choisir, ou non, une bibliothèque native tierce (`wgpu`, `ash`) : décision de
      Killian (contrat 5).
