# Métriques et mesures de RUSTFORGE-X

Ce fichier répond à **R-562** : toute métrique exposée par C-34 y est documentée — nom,
type, unité, sémantique. Le harnais de mesure lui-même, sa méthodologie et ses campagnes
sont décrits dans [`benchmarks/README.md`](benchmarks/README.md) ; ils ne sont pas
recopiés ici.

Un test de fondation vérifie que **chaque métrique produite par le code figure dans ce
tableau**. Une métrique ajoutée sans sa ligne fait échouer la construction : c'est le
seul moyen qu'un catalogue reste vrai.

## Rien ne quitte la machine

**R-560** : aucune télémétrie externe, aucune requête réseau sortante. Ce n'est pas une
politique mais une absence de code, et T-400 le vérifie en relisant les sources — aucune
d'elles n'a le droit de nommer un type qui ouvre une connexion.

**R-571** : les chemins sont anonymisés dans les rapports exportables. Le répertoire de
jeu devient `<gameDir>`, le répertoire personnel `<home>`. Un rapport est fait pour être
envoyé ; il ne doit pas dire où habite celui qui l'envoie.

## Types

| type | sens |
|---|---|
| `counter` | décompte qui ne fait que croître |
| `gauge` | valeur instantanée |
| `ratio` | rapport entre deux mesures, en pourcent |

## Une valeur non mesurée n'est pas publiée

Plusieurs métriques n'apparaissent **que** si leur source a effectivement mesuré. C'est
R-660 : un zéro à la place d'une mesure absente est indiscernable d'un zéro mesuré, et
c'est exactement ce qui rend un tableau de bord trompeur. Les lignes concernées sont
marquées « conditionnelle ».

## Catalogue

### `rfx.tick` — cycle de tick (C-01, IF-02)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.tick.count` | counter | — | Ticks dont la fenêtre a été ouverte et fermée |
| `rfx.tick.unbalanced` | counter | — | Fenêtres ouvertes sans fermeture correspondante |
| `rfx.tick.invalid_transitions` | counter | — | Transitions refusées par SM-01 |
| `rfx.tick.hook_budget_exceeded` | counter | — | Accroches ayant dépassé leur budget (INV-14) |

### `rfx.probes` — transport des enregistrements (C-31, IF-03)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.probes.records_consumed` | counter | — | Enregistrements lus par le natif |
| `rfx.probes.records_lost` | counter | — | Enregistrements perdus faute de place |
| `rfx.probes.native_bytes` | gauge | octets | Mémoire native détenue par les tampons |
| `rfx.probes.native_limit_bytes` | gauge | octets | Plafond (`memory.max_native_mb`) |

### `rfx.profiler` — profileur (C-05)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.profiler.workloads_tracked` | counter | — | Unités de travail suivies simultanément |
| `rfx.profiler.records_ingested` | counter | — | Enregistrements intégrés aux mesures glissantes |
| `rfx.profiler.records_unknown` | counter | — | Enregistrements à identifiant de sonde inconnu |
| `rfx.profiler.evictions` | counter | — | Unités évincées, la plus froide en premier |
| `rfx.profiler.level_changes` | counter | — | Changements d'état du profileur (SM-02) |
| `rfx.profiler.call_capped` | counter | — | Sondes éteintes en ce moment pour plus de 1 000 appels par tick ; l'échantillonnage en rend le coût (ADR-038) |
| `rfx.profiler.overhead_pct` | ratio | % | Coût **estimé** par les compteurs, en part d'un cœur |
| `rfx.profiler.mspt_pct` | ratio | % | Coût **estimé**, en part du temps de tick |
| `rfx.profiler.baseline_measurements` | counter | — | Mises en pause ayant produit une mesure (PARTIE 12.4) |
| `rfx.profiler.baseline_overhead_pct` | ratio | % | Coût **mesuré** par mise en pause — *conditionnelle* |

La distinction entre `overhead_pct` (estimé) et `baseline_overhead_pct` (mesuré) est
essentielle : le premier vient des compteurs du profileur, qui ne voient pas le coût des
appels injectés dans le bytecode ; le second vient d'une comparaison réelle avec des
ticks non instrumentés. **Une mesure prime sur une estimation** (R-770).

### `rfx.instr` — instrumentation (C-04)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.instr.armed` | gauge | — | 1 si le transformateur est armé (ADR-017) |
| `rfx.instr.classes_seen` | counter | — | Classes soumises au transformateur depuis l'armement |
| `rfx.instr.classes_missed` | counter | — | Classes passées **avant** l'armement, hors de portée à jamais (ADR-022) |
| `rfx.instr.methods_probed` | counter | — | Méthodes portant effectivement une sonde |
| `rfx.instr.transform_failures` | counter | — | Transformations abandonnées, classe rendue intacte (FM-09) |
| `rfx.instr.probes_requested` | counter | — | Identifiants de sonde demandés au natif |

`classes_missed` mérite une lecture : une classe ne se charge qu'une fois, et celles qui
traversent le transformateur avant son armement ne repasseront jamais. La métrique dit
donc ce que `methods_probed` vaut réellement comme couverture.

### `rfx.discovery` — inventaire des mods (C-41)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.discovery.mods` | counter | — | Mods inventoriés |
| `rfx.discovery.modules` | counter | — | Modules Java distincts les portant |
| `rfx.discovery.packages` | counter | — | Paquets déclarés, tous mods confondus |
| `rfx.discovery.duration_ms` | gauge | ms | Durée de la découverte, hachages exclus (R-621, ADR-023) |
| `rfx.discovery.unknown_owner_ratio` | ratio | % | Part des sondes sur une classe non rattachée — *conditionnelle* |

`unknown_owner_ratio` porte sur les **méthodes sondées, toutes**, pas seulement les
chaudes. Il ne prononce donc pas l'acceptance de la PARTIE 5.39, qui parle des classes
chaudes ; il montre seulement que l'attribution fonctionne.

### `rfx.sampling` — échantillonnage de piles (C-05)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.sampling.samples_taken` | counter | — | Piles prélevées pendant le tick, sur le fil autoritatif et les fils qui exécutent du code sondé (R-322, ADR-040, ADR-041) |
| `rfx.sampling.samples_queued` | counter | — | Échantillons attribués à une sonde et mis en file |
| `rfx.sampling.samples_unattributed` | counter | — | Échantillons qu'aucune sonde connue ne couvrait |
| `rfx.sampling.samples_dropped` | counter | — | Échantillons perdus, le fil autoritatif ne drainant plus |
| `rfx.discovery.unknown_frames` | counter | — | Méthodes distinctes vues s'exécuter sans sonde |
| `rfx.discovery.unknown_samples` | counter | — | Échantillons ayant désigné une de ces méthodes |
| `rfx.discovery.unknown_frames_dropped` | counter | — | Méthodes distinctes non apprises, recensement plein |
| `rfx.discovery.unknown_frame_ratio` | ratio | % | Part du fil autoritatif passée hors sonde — *conditionnelle* |

`unknown_frame_ratio` est la **mesure du trou de couverture**, et la seule série qui la
donne. ADR-027 a établi qu'élargir le sondage ne l'améliore pas : le budget finance
quelques milliers de sondes, et c'est déjà ce qu'on arme. Cette part dit ce qui reste
dehors, et le classement de `/rfx discover` dit *qui* — sans dépenser une sonde de plus.

La part porte sur le **temps du fil autoritatif**, pas sur celui du tick :
l'échantillonneur prélève aussi entre deux ticks. Les deux ne se confondent pas, et le
rapport de comparaison n'est pas connu.

### `rfx.events` — observation du bus (C-06)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.events.dispatched` | counter | — | Événements vus passer sur le bus Forge |
| `rfx.events.known_types` | counter | — | Types d'événements distincts rencontrés |
| `rfx.events.timed` | counter | — | Distributions chronométrées, une sur soixante-quatre |
| `rfx.events.abandoned` | counter | — | Chronométrages abandonnés, imbrication trop profonde |

### `rfx.sched` — ordonnanceur (C-17)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.sched.external_load_reports` | counter | — | Relevés de charge externe reçus par le natif, un par seconde (R-421, ADR-046) |
| `rfx.sched.external_runnable` | gauge | fils | Fils tiers ayant consommé au moins 10 % d'un cœur depuis le relevé précédent, au dernier relevé — *conditionnelle* |
| `rfx.sched.external_runnable_max` | gauge | fils | Plus haut relevé depuis le démarrage — *conditionnelle* |
| `rfx.sched.workers_active` | gauge | fils | Workers actifs du pool, après réduction pour sur-souscription (R-421) — *conditionnelle* : pool démarré (ADR-061) |
| `rfx.sched.queue_depth` | gauge | tâches | Tâches en attente dans les files d'injection — *conditionnelle* : pool démarré |
| `rfx.sched.executed` | counter | — | Tâches exécutées par le pool — *conditionnelle* : pool démarré |
| `rfx.sched.steals` | counter | — | Vols de travail entre workers — *conditionnelle* : pool démarré |
| `rfx.sched.rejected` | counter | — | Soumissions refusées : contre-pression (`E-2401`) ou exécution en série d'une soumission détachée — *conditionnelle* : pool démarré |
| `rfx.sched.panics` | counter | — | Tâches qui ont paniqué, worker sauf (R-524) — *conditionnelle* : pool démarré |
| `rfx.sched.deadline_misses` | counter | — | Tâches dont l'échéance est passée — *conditionnelle* : pool démarré |

Le compte se fait sur le temps processeur consommé, et non sur l'état `RUNNABLE` de la
JVM : un fil bloqué dans un appel natif — une attente réseau, par exemple — est
`RUNNABLE` sans rien consommer, et sur le modpack d'essai ces fils étaient la grande
majorité (ADR-046). Les fils internes de la JVM et les workers natifs n'y figurent pas.

### `rfx.db` — base des unités de travail (C-09, C-48)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.db.open_failures` | counter | — | Ouvertures échouées faute de pouvoir créer le fil d'écriture ; la base reste fermée, le jeu tourne normalement |
| `rfx.db.load` | gauge | code | Issue de la relecture du lancement : 0 en cours, 1 aucun fichier, 2 autre environnement (ignoré, R-360), 3 fichier illisible mis de côté en `.bak` (E-3003, FM-17), 4 relue, 5 lecture refusée par le système |
| `rfx.db.loaded_entries` | gauge | unités | Unités de travail relues du fichier — *conditionnelle* : relecture terminée |
| `rfx.db.load_rejected_entries` | counter | — | Entrées écartées à la relecture, leur code ayant changé (R-360) — *conditionnelle* : relecture terminée |
| `rfx.db.entries` | gauge | unités | Unités de travail dans la base |
| `rfx.db.memory_bytes` | gauge | octets | Mémoire estimée de la base, plafonnée par `cache.max_memory_mb` et comptée dans `memory.max_native_mb` ; l'estimation suit l'allocateur à 1 % près (ADR-048) |
| `rfx.db.evictions` | counter | — | Unités évincées de la base, les plus froides d'abord (R-362) |
| `rfx.db.pending` | gauge | unités | Unités découvertes, pas encore confiées au fil d'écriture |
| `rfx.db.syncs` | counter | — | Lots confiés au fil d'écriture, un toutes les cinq minutes |
| `rfx.db.handed_back` | counter | — | Lots rendus parce que la file était pleine ; repris au cycle suivant |
| `rfx.db.writes` | counter | — | Fichiers `workload.db` écrits, de façon atomique (R-224) |
| `rfx.db.write_failures` | counter | — | Échecs d'écriture ; le premier désactive l'écriture (R-692, FM-18) |
| `rfx.db.write_disabled` | gauge | booléen | 1 si l'écriture est désactivée ; la base continue en mémoire |
| `rfx.db.persist_ms` | gauge | ms | Durée de la dernière écriture, encodage compris, hors du fil du jeu (pool de l'ordonnanceur, ou fil `RFX-IO` de secours) — *conditionnelle* : au moins une écriture |

Rien de cette section n'est publié tant que la base n'est pas ouverte (R-660) : Java
l'ouvre quand l'inventaire des mods est connu, au chargement complet (ADR-049).

### `rfx.callgraph` — graphe d'appel (C-07)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.callgraph.hierarchy_classes` | gauge | classes | Classes de l'installation déclarées à l'analyse (nom, super-classe, drapeaux), lues dans les archives par le fil `rustforgex-analysis` |
| `rfx.callgraph.budget_refusals` | counter | — | Refus du budget mémoire natif ; le premier arrête l'analyse (R-533) |
| `rfx.callgraph.graphs` | counter | — | Graphes construits, un par unité chaude — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.rounds` | counter | — | Tours de construction, relances comprises quand une classe manquait — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.nodes` | counter | — | Méthodes, sommées sur tous les graphes — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.resolved_calls` | counter | — | Sites d'appel dont toutes les cibles sont connues — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.unresolved_calls` | counter | — | Sites d'appel non résolus : classe inconnue ou transformée, appel par une interface, plus de quatre cibles, aucune implémentation, méthodes par défaut ambiguës, `invokedynamic` hors lambda, réflexion, code illisible — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.unresolved_ratio` | ratio | % | Part des sites d'appel non résolus (FM-15) — *conditionnelle* : au moins un site rencontré |
| `rfx.callgraph.truncated` | counter | — | Graphes arrêtés par une borne, `analysis.max_call_depth` ou `analysis.max_nodes_per_workload` (R-340) — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.reflection` | counter | — | Graphes où un appel réflexif a été rencontré — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.classes_provided` | counter | — | Classes dont le bytecode a été lu pour l'analyse — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.classes_absent` | counter | — | Classes tenues pour inconnues : cibles de mixin, d'access transformer ou de coremod, introuvables ou illisibles — *conditionnelle* : hiérarchie déclarée |
| `rfx.callgraph.build_ms` | gauge | ms | Durée de la dernière construction, hors tick — *conditionnelle* : au moins une construction |

Rien de cette section n'est publié si `analysis.enabled` vaut `false`. Les graphes ne
pilotent encore aucune décision : ces métriques décrivent ce que l'analyse sait, pas
un gain (ADR-051).

### `rfx.analysis` — profileur d'unités de travail (C-08)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.analysis.queue_depth` | gauge | unités | Unités chaudes en attente d'analyse, les plus rentables d'abord — *conditionnelle* : hiérarchie déclarée |
| `rfx.analysis.completed` | counter | — | Descripteurs calculés, recalculs compris — *conditionnelle* : hiérarchie déclarée |
| `rfx.analysis.truncated` | counter | — | Descripteurs tronqués par une borne ou par l'épuisement des tours : `UNKNOWN`, jamais optimistes (FM-16) — *conditionnelle* : hiérarchie déclarée |
| `rfx.analysis.cpu_ms` | gauge | ms | Temps cumulé des pas d'analyse, horloge monotone du fil d'analyse : borne haute du temps processeur — *conditionnelle* : hiérarchie déclarée |
| `rfx.analysis.descriptors` | gauge | unités | Descripteurs tenus à jour — *conditionnelle* : hiérarchie déclarée |
| `rfx.analysis.unknown` | gauge | unités | Descripteurs `UNKNOWN` : un fait manque, traité comme le pire cas (P-02). Tant que C-10 à C-13 n'existent pas, tous le sont (ADR-054) — *conditionnelle* : hiérarchie déclarée |
| `rfx.analysis.descriptor_changes` | counter | — | Changements significatifs d'un descripteur (drapeaux, ou confiance combinée de 0,1), destinés à C-15 — *conditionnelle* : hiérarchie déclarée |
| `rfx.analysis.recomputes` | counter | — | Recalculs sur changement du bytecode de la méthode racine (R-350) — *conditionnelle* : hiérarchie déclarée |

### `rfx.dataflow` — analyse de flux de données (C-11)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.dataflow.exact_ratio` | gauge | ratio | Part des descripteurs dont les écritures sont `EXACT` : tout appel résolu et analysé, aucun domaine inconnu (R-381) — *conditionnelle* : au moins un descripteur |
| `rfx.dataflow.analysis_ns` | counter | ns | Temps cumulé de composition des effets le long des graphes, horloge monotone du fil d'analyse — *conditionnelle* : hiérarchie déclarée |

### `rfx.dep` — dépendances entre unités (C-10)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.dep.edges` | gauge | dépendances | Dépendances des unités décrites, vues de chacune : RAW, WAW, WAR, verrous, ou tout le tick pour une unité aux accès inconnus — *conditionnelle* : hiérarchie déclarée |
| `rfx.dep.conflicts` | gauge | conflits | Conflits d'écriture (WAW), vus de chaque unité — *conditionnelle* : hiérarchie déclarée |
| `rfx.dep.conservative_edges` | gauge | dépendances | Dépendances ajoutées par sûreté : tout le tick (accès `UNKNOWN`), verrous d'identité inconnue — *conditionnelle* : hiérarchie déclarée |
| `rfx.dep.analysis_ns` | counter | ns | Temps cumulé de calcul des dépendances, horloge monotone du fil d'analyse — *conditionnelle* : hiérarchie déclarée |

Les dépendances sont calculées au niveau des domaines, toutes les unités décrites étant
supposées du même tick ; l'ordre des flux d'événements (C-06) et les régions manquent
(ADR-056).

### `rfx.thread` — contraintes de fil (C-12)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.thread.main_required_ratio` | gauge | ratio | Part des descripteurs qui exigent le fil autoritatif : écriture de l'état du jeu, ou classe non sûre entre fils touchée — *conditionnelle* : au moins un descripteur |
| `rfx.thread.unknown_ratio` | gauge | ratio | Part des descripteurs dont l'exigence de fil est inconnue, traitée comme une exigence (P-02) ; tant que C-13 manque, aucune unité n'est libre — *conditionnelle* : au moins un descripteur |

L'analyse est statique : les rôles de fil sur lesquels une unité s'exécute ne sont pas
observés (ADR-057).

### `rfx.det` — déterminisme (C-13)

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.det.deterministic_ratio` | gauge | ratio | Part des descripteurs sans source de non-déterminisme sur un graphe complet — *conditionnelle* : au moins un descripteur |
| `rfx.det.nondet_sources{source=shared_random}` | gauge | descripteurs | Descripteurs où l'analyse trouve : générateur pseudo-aléatoire (`Random`, `RandomSource`, `Math.random`, domaine `Random`) |
| `rfx.det.nondet_sources{source=system_time}` | gauge | descripteurs | Descripteurs où l'analyse trouve : horloge (`System.nanoTime`, `currentTimeMillis`, `java/time`, domaine `Time`) |
| `rfx.det.nondet_sources{source=identity_hash}` | gauge | descripteurs | Descripteurs où l'analyse trouve : hachage d'identité (`System.identityHashCode`, `hashCode` ou `toString` d'`Object`) |
| `rfx.det.nondet_sources{source=hash_map_iteration}` | gauge | descripteurs | Descripteurs où l'analyse trouve : parcours d'une table de hachage du JDK |
| `rfx.det.nondet_sources{source=thread_scheduling}` | gauge | descripteurs | Descripteurs où l'analyse trouve : prise ou attente de verrou |
| `rfx.det.nondet_sources{source=external_io}` | gauge | descripteurs | Descripteurs où l'analyse trouve : entrée-sortie (`java/io`, `java/nio/file`, `java/net`, domaines `FileSystem`, `Network`) |
| `rfx.det.nondet_sources{source=native_call}` | gauge | descripteurs | Descripteurs où l'analyse trouve : méthode `native` dans le graphe |
| `rfx.det.nondet_sources{source=reflection}` | gauge | descripteurs | Descripteurs où l'analyse trouve : appel réflexif rencontré par C-07 |
| `rfx.det.nondet_sources{source=unknown}` | gauge | descripteurs | Descripteurs où l'analyse trouve : partie du graphe inconnue : ce qu'elle fait n'a pas été vu |

La recherche est statique : aucune exécution en shadow n'est comparée (ADR-058), et
`rfx.det.shadow_mismatches` n'est pas publiée.

`rfx.dataflow.mismatch` (PARTIE 5.11) n'est pas publiée : elle compterait les écarts
trouvés par la vérification dynamique (R-382), qui n'existe pas encore. Publiée à zéro,
elle laisserait croire à une vérification qui n'a pas lieu (ADR-055).

### `rfx.hook` — gardes des accroches (C-01, FM-02)

Une série par accroche : `setup`, `loadComplete`, `registerCommands`, `warmUpCommands`,
`serverStopping`, `serverTickPre`, `serverTickPost`. Ces noms sont fixés par le code,
jamais par une donnée d'exécution.

| métrique | type | unité | sémantique |
|---|---|---|---|
| `rfx.hook.calls{hook=…}` | counter | — | Exécutions de l'accroche, réussies ou non |
| `rfx.hook.errors{hook=…}` | counter | — | Exceptions capturées par la garde |
| `rfx.hook.duration_ns{hook=…}` | gauge | ns | Durée moyenne d'une exécution chronométrée ; absente si `telemetry.enabled = false` |
| `rfx.hook.disabled` | counter | — | Accroches désactivées après cinq échecs |

## Export

- **En mémoire** : `/rfx status`, `/rfx mods`.
- **Fichier JSON** : `/rfx report` écrit `<gameDir>/rustforgex/reports/rfx-report-<horodatage>.json`.
  Le nom porte l'horodatage et n'écrase jamais : deux rapports pris à deux moments sont
  deux observations.

## Cardinalité

La PARTIE 5.32 exige une cardinalité bornée. Aucune métrique ne porte d'étiquette libre,
et l'ensemble des noms est fixé par le code — jamais par une donnée d'exécution. Un
`WorkId` ne peut donc pas devenir un nom de série. La borne est de 256 séries et elle
est **vérifiée à l'exécution** : la dépasser lève, parce qu'un dépassement signalerait un
nom construit à partir d'une donnée, c'est-à-dire un défaut.
