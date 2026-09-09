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
| `rfx.sampling.samples_taken` | counter | — | Piles prélevées sur le fil autoritatif (R-322) |
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
