# CONFIGURATION

Toutes les options de RUSTFORGE-X, leurs plages et leurs effets.
Référence normative : cahier des charges, PARTIE 28. Composant : C-37.

## Fichier

```text
<répertoire de jeu>/rustforgex/config/rustforgex.toml
```

Il est créé avec les valeurs par défaut, commentées, au premier lancement. Toutes les
valeurs par défaut sont sûres : une installation qui n'y touche jamais reste correcte.

## Priorité des sources

Du plus fort au plus faible :

1. propriétés système `-Drustforgex.<section>.<clé>=<valeur>` ;
2. le fichier `rustforgex.toml` ;
3. les valeurs par défaut.

Exemple, pour désactiver le mod le temps d'un lancement sans toucher au fichier :

```bash
java -Drustforgex.general.enabled=false -jar ...
```

## Règles de validation

- Une valeur **hors plage** ou de type incorrect est rejetée, signalée dans les
  journaux avec la plage attendue, et remplacée par le défaut. Il n'existe aucun cas
  où une valeur invalide produit un comportement indéfini.
- Une **clé inconnue** est conservée telle quelle et signalée. Elle n'est jamais
  supprimée : elle appartient peut-être à une version plus récente, et perdre le
  réglage d'un utilisateur lors d'un aller-retour de version serait inacceptable.
- Les options marquées **redémarrage requis** ne peuvent pas changer en cours de
  partie : elles dimensionnent des structures créées au démarrage.

## Options du jalon M0

| Clé | Type | Défaut | Plage | Redémarrage | Effet |
|---|---|---|---|---|---|
| `general.enabled` | booléen | `true` | `true` / `false` | oui | `false` : RUSTFORGE-X ne fait rien du tout. Le natif n'est même pas extrait. |
| `general.mode` | texte | `balanced` | `safe`, `balanced`, `performance`, `experimental`, `debug` | non | Profil de risque et d'agressivité des décisions (PARTIE 28.3). |
| `general.side_client` | booléen | `true` | `true` / `false` | oui | Activer le runtime sur le client. |
| `general.side_server` | booléen | `true` | `true` / `false` | oui | Activer le runtime sur le serveur dédié. |
| `memory.max_native_mb` | entier | `512` | `16` .. `16384` | oui | Plafond de mémoire native, en mébioctets. |
| `telemetry.enabled` | booléen | `true` | `true` / `false` | oui | Télémétrie **de tick** : chronométrage des accroches (`rfx.hook.duration_ns`) et journal de tick en `DEBUG`. À `false`, ni l'un ni l'autre ; les échecs des accroches restent comptés, et `/rfx status` comme `/rfx report` fonctionnent toujours. Aucune donnée ne quitte la machine, jamais. |
| `diagnostics.report_on_incident` | booléen | `true` | `true` / `false` | oui | Consigne chaque incident dans `<gameDir>/rustforgex/crash/rfx-crash-<ts>.json` : accroche désactivée après cinq échecs, panic native (E-3001). Un fichier par incident et par partie, seize au plus, chemins anonymisés. Rien ne quitte la machine. |
| `runtime.panic_threshold` | entier | `3` | `1` .. `100` | non | Panics tolérées pour un sous-système avant sa désactivation (R-523). |
| `profiler.max_workloads` | entier | `20000` | `1000` .. `200000` | oui | Unités de travail suivies simultanément. Au-delà, la plus froide est évincée (R-321). Un plafond plus haut affine la mesure et coûte de la mémoire native. |
| `profiler.cpu_budget_pct` | entier | `2` | `1` .. `50` | oui | Part d'un cœur accordée au profilage, en pourcent (H-07). Au-delà, la profondeur de sondage descend d'un cran, jusqu'à l'arrêt. Le budget de temps de tick en découle dans la même proportion. Le quart de cette part borne aussi l'échantillonnage de piles : sa cadence ralentit (de 100 Hz jusqu'à 1 Hz) quand un prélèvement coûte plus cher (ADR-037). |

> Le coût réellement mesuré s'affiche dans `/rfx status`, ligne « Coût mesuré » : il
> vient de la mise en pause périodique de la PARTIE 12.4, et non d'une estimation.

### Base des unités de travail (C-09, ADR-049)

| Clé | Type | Défaut | Plage | Redémarrage | Effet |
|---|---|---|---|---|---|
| `cache.enabled` | booléen | `true` | `true` / `false` | oui | Base des unités de travail, `<gameDir>/rustforgex/cache/workload.db` : relue au lancement en arrière-plan, écrite hors du tick toutes les cinq minutes et à l'arrêt. À `false`, elle n'est ni lue ni écrite, et aucun fil n'est créé. |
| `cache.max_memory_mb` | entier | `128` | `8` .. `4096` | oui | Mémoire estimée de la base, en mébioctets. Elle est comptée aussi dans `memory.max_native_mb`, et bornée par ce qu'il en reste. Au-delà, les unités les plus froides sont évincées (R-362). Mesuré : 46,6 Mio pour 20 000 unités dans le pire cas (ADR-048). |

La base est jetée en entier, puis reconstruite en jouant, dès que l'environnement
change : version de Minecraft, de Forge, de RUSTFORGE-X ou de Java, ajout, retrait ou
mise à jour d'un mod, classe de matériel, ou **n'importe quelle option** de ce fichier.
C'est voulu : une mesure faite dans un autre contexte ne doit pas guider le runtime
(R-360). Un fichier illisible est renommé en `workload.db.bak`, jamais lu en partie.

### Graphe d'appel (C-07, ADR-051)

| Clé | Type | Défaut | Plage | Redémarrage | Effet |
|---|---|---|---|---|---|
| `analysis.enabled` | booléen | `true` | `true` / `false` | oui | Graphe d'appel des unités les plus coûteuses. Après le chargement, le fil `rustforgex-analysis` lit les en-têtes de toutes les classes de l'installation, puis le bytecode de celles qu'une analyse demande ; rien ne se passe dans le tick. Les cibles de mixin, d'access transformer et de coremod sont tenues pour inconnues. Sans instrumentation armée, le fil n'est pas créé. À `false`, aucune classe n'est lue. |
| `analysis.cpu_budget_pct` | entier | `5` | `1` .. `50` | oui | Part d'un cœur accordée au fil d'analyse, en pourcent : après chaque lot de travail, il s'efface le temps nécessaire pour ne pas dépasser cette part. |
| `analysis.max_call_depth` | entier | `12` | `1` .. `64` | oui | Profondeur d'exploration d'un graphe ; au-delà, il est marqué tronqué (R-340). |
| `analysis.max_nodes_per_workload` | entier | `2000` | `100` .. `20000` | oui | Méthodes au plus par graphe ; au-delà, il est marqué tronqué (R-340). La mémoire de l'analyse est comptée dans `memory.max_native_mb`. |

### Cadence de la mesure de coût (PARTIE 12.4)

| Clé | Type | Défaut | Plage | Rechargeable | Description |
|---|---|---|---|---|---|
| `profiler.baseline_period_ticks` | entier | `2048` | `20` .. `6000` | non | Ticks entre deux mises en pause de mesure |
| `profiler.baseline_pause_ticks` | entier | `512` | `5` .. `512` | non | Durée d'une pause, et taille des deux fenêtres comparées |
| `profiler.baseline_cycles` | entier | `30` | `3` .. `64` | non | Cycles agrégés avant qu'une mesure soit rendue |

Le coût de RUSTFORGE-X est mesuré en éteignant périodiquement le sondage et en
comparant la médiane des ticks actifs à celle des ticks en pause. `baseline_pause_ticks`
fixe la taille des deux fenêtres, et décide donc si cette comparaison mesure quelque
chose.

Le profilage ne peut pas rendre un tick plus rapide : **une différence négative par
cycle est nécessairement du bruit**. C'est le critère de qualité de la mesure, et il a
servi à choisir le défaut. Mesuré sur le serveur de banc, même charge :

| queue négative | cycles positifs | coût rendu |
|---|---|---|
| fenêtre 20 : −1 527 250 ns | 14 / 30 | 0 % |
| fenêtre 300 : −1 167 200 ns | 11 / 20 | 1,85 % |
| **fenêtre 512 : −67 900 ns** | **17 / 20** | **6,29 %** |

Attention au sens de lecture : une médiane de différences **signées** est biaisée vers
zéro quand le bruit domine. Les petits chiffres ci-dessus ne sont pas des coûts faibles,
ce sont des **mesures ratées** — à vingt ticks par fenêtre, la mesure rend zéro quoi
qu'il arrive, et le gouverneur ne se déclenche jamais.

Le prix de la fenêtre par défaut est double, et assumé (voir
[ADR-024](docs/decisions/ADR-024.md)) :

- le profilage est éteint **20 % du temps** ;
- une mesure demande `cycles × (période + pause)` ticks, soit **64 minutes de jeu**.
  Pendant ce délai aucune ligne de base n'existe, et la profondeur de sondage ne peut
  pas remonter au-delà de `LIGHT`.

Ces valeurs se règlent : une campagne qui cherche à départager deux cadences descend
`baseline_cycles` pour échanger de la précision contre du temps.

### Section `instrumentation`

| Clé | Type | Défaut | Plage | Rechargeable | Description |
|---|---|---|---|---|---|
| `instrumentation.early_arm` | booléen | `false` | — | non | Armer le sondage dès la construction du mod plutôt qu'au setup commun. |

Le transformateur est enregistré au lancement, mais ne pose de sonde qu'une fois *armé*,
ce qui suppose le runtime natif démarré. Toute classe chargée entre les deux ressort
inchangée — et définitivement, puisqu'une classe ne se charge qu'une fois.

Mesuré sur un serveur de 288 mods : avec le défaut, **32 712** des 76 327 classes visées
passent avant l'armement, et **2 635** méthodes sont sondées. Avec `early_arm = true`,
il n'en passe plus que **5 318**, et **13 615** méthodes sont sondées — cinq fois plus de
couverture, parce que les blocs et entités des mods se chargent pendant la construction
de ceux-ci.

Le défaut reste `false` : le surcoût de RUSTFORGE-X croît avec le nombre de méthodes
sondées (ADR-021), et celui de l'armement anticipé n'est pas encore mesuré. Voir
[ADR-022](docs/decisions/ADR-022.md).

La ligne de journal de l'armement indique les deux chiffres, quelle que soit l'option :
combien de classes sont passées avant l'armement, et combien avant même la construction
du mod — ces dernières étant hors d'atteinte dans les deux cas.

### Les autres sections de la PARTIE 28.2

La configuration normative du cahier des charges décrit aussi les sections
`analysis`, `decision`, `scheduler`, `snapshot`, `commit`, `validation`,
`shadow`, `cache`, `mirror`, `ir`, `network`, `learning`, `diagnostics`, `ui` et
`overrides`.

La section `cache` n'expose que `enabled` et `max_memory_mb` : `max_disk_mb` n'a pas
de rôle propre tant que `workload.db` est le seul fichier du cache — sa taille suit la
mémoire de la base (25 Mio sur disque pour 46,6 Mio en mémoire, mesuré, ADR-048) —, et
`result_cache_ttl_ticks` comme `purge_on_version_change` concernent des caches qui
n'existent pas encore.

La section `diagnostics` n'expose que `report_on_incident` : `keep_days` et
`max_disk_mb` supposent une éviction des fichiers anciens qui n'existe pas encore, et
le nombre de dumps est pour l'instant borné par partie (ADR-033).

La section `profiler` n'expose que les deux options ci-dessus : les autres options de
profilage de la PARTIE 28.2 ne pilotent encore rien.

Ces sections **n'existent pas encore** dans le fichier généré. Elles pilotent des composants
qui ne sont pas implémentés : les déclarer maintenant donnerait à croire qu'un réglage
a un effet alors qu'il n'en aurait aucun. Chaque section apparaîtra au jalon qui la
rend opérante. Si vous ajoutez l'une de ces clés à la main, elle sera conservée et
signalée comme inconnue, sans effet.

## Modes

| Mode | Risque de correction max | Exploration | Destiné à |
|---|---|---|---|
| `safe` | 0.05 | 0 % | serveurs de production prudents |
| `balanced` | 0.15 | 2 % | usage courant, valeur par défaut |
| `performance` | 0.30 | 5 % | machines dédiées, gain recherché |
| `experimental` | 0.30 | 5 % | tests des chemins non stabilisés |
| `debug` | 0.15 | 0 % | diagnostic : assertions d'invariants, journalisation par tick |

Au jalon M0, le mode n'influence encore aucune décision — aucune transformation n'est
prise. Il est transmis au runtime natif et détermine une seule chose : la disponibilité
de `/rfx panic-test`, réservée à `debug`.

Le mode `debug` n'est pas destiné au jeu normal : il instrumente au maximum et le dit
clairement dans les journaux.

## Vérifier la configuration effective

En jeu, avec la permission niveau 3 :

```text
/rfx status
```

La commande est traduite : les textes viennent de `assets/rustforgex/lang/fr_fr.json`
et `en_us.json`, jamais du code. Le rapport s'affiche donc dans la langue du client.

La commande affiche le mode effectif, l'état du runtime, la classe matérielle mesurée
et la maturité de chaque composant. Un champ que la sonde n'a pas mesuré est affiché
« non mesuré » : aucun zéro n'y passe pour une mesure.
