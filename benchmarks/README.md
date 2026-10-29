# Benchmarks — C-36

Cahier des charges : **PARTIE 21**. Ce répertoire porte le harnais de mesure et ses
résultats.

## La règle, avant tout le reste

> **R-580 / R-860 — aucune performance ne doit être inventée.**
> Un chiffre qui n'a pas été produit par ce harnais ne doit apparaître ni dans la
> documentation, ni dans le README, ni dans l'interface, ni dans une réponse.

C'est la raison d'être de ce répertoire. Tant qu'une mesure n'existe pas ici, la
réponse honnête à « combien ça coûte ? » est « je ne sais pas ».

> **R-581** — un benchmark sans intervalle de confiance ni nombre de répétitions est
> invalide. Chaque mesure publiée porte donc sa médiane, son écart interquartile, son
> intervalle de confiance et son nombre d'échantillons.

## Deux niveaux

| Niveau | Ce qu'il mesure | État |
|---|---|---|
| **A — micro** | coût unitaire des opérations natives : hachage, agrégation, tampons, sérialisation | implémenté |
| **B — macro** | MSPT, TPS, ramasse-miettes, **coût de RUSTFORGE-X** | implémenté (serveur) |

**Le niveau A ne répond pas à « quel est le coût ? ».** Il mesure des morceaux, pas le
tout. Seul le niveau B — serveur dédié scripté, comparaison de deux configurations —
peut y répondre. Ne pas faire dire au premier ce que seul le second sait.

## Lancer le niveau A

```bash
cargo bench -p rfx-bench
cargo run -p rfx-bench --release
```

La première commande mesure et écrit les échantillons bruts sous `target/criterion/`.
La seconde les relit et produit `benchmarks/results/micro-<horodatage>-<commit>.json`
au schéma de la PARTIE 21.3.

## Lancer le niveau B

```bash
./benchmarks/run-macro.sh                # campagne normative : > 2 heures
./benchmarks/run-macro.sh 2 400 200      # vérification du harnais : ~5 minutes
```

Le script lance le serveur dédié une fois par exécution et par configuration —
**processus distinct à chaque fois**, ce qu'exige la PARTIE 21.3 point 5 : c'est ce qui
rend les répétitions indépendantes, JIT et caches compris. Chaque exécution écrit
`benchmarks/runs/<configuration>-<n>.json`, puis `rfx-bench macro` les agrège.

Deux configurations sont comparées. Ce sont les configurations **B** et **C** de la
PARTIE 21.2, dont l'écart mesure le **coût du système** :

| Étiquette | Ce qui tourne |
|---|---|
| `b-mods-seuls` | le modpack sans RUSTFORGE-X : JAR du transformateur retiré de `mods`, `general.enabled=false` |
| `c-rfx-actif` | le même modpack, RUSTFORGE-X observant, instrumentation armée |

L'écart entre les deux est le **coût du profilage**. C'est le passage de B à C de la
PARTIE 21.2, à ceci près qu'il n'y a pas de modpack : la mesure décrit un serveur nu,
et rien d'autre. Un profil de charge de la PARTIE 22 donnera un chiffre différent.

### Les trois garde-fous du niveau B

- **`methodology_compliant`** est calculé, jamais supposé : cinq exécutions,
  3 600 ticks d'échauffement, 12 000 ticks mesurés. En dessous, le résultat est marqué
  non conforme et **ne peut pas être publié**. C'est ce qui empêche une vérification
  rapide du harnais de se faire passer un jour pour une mesure.
- **`rejected`** signale une dispersion inter-exécutions supérieure à 10 % sur le
  MSPT p95 (PARTIE 21.3, point 7). La machine n'était pas au repos : le chiffre décrit
  l'environnement, pas le logiciel.
- **Le commit mesuré** est celui que porte chaque fichier d'exécution (`build`, écrit
  dans le JAR à la build), plus celui de `HEAD` au moment d'agréger. Des exécutions de
  commits différents, ou qui ne le portent pas, sont signalées dans `notes`.
- **La charge de la machine hors serveur** est relevée une fois par seconde pendant la
  fenêtre de mesure (`machine` dans le fichier d'exécution : charge de toute la machine
  moins celle du jeu, en part de sa capacité). Une exécution au-delà de 10 % au p95, ou
  qui ne l'a pas relevée, rend la campagne non conforme, avec l'exécution nommée dans
  `compliance_notes` (ADR-042, « Bancs réalistes »). On ne demande pas une machine au
  repos ; on refuse de citer une mesure faussée. Le seuil de 10 % est un choix de départ,
  pas une mesure : à recalibrer. Les campagnes archivées, écrites avant ce relevé, sont
  désormais non conformes à la ré-agrégation.
Retirer le JAR du transformateur n'est pas un détail de mise en scène :
`general.enabled=false` éteint le runtime, mais laisse le transformateur énumérer ses
cibles et décoder en `ClassNode` chaque classe désignée. Une configuration B qui le
garderait ne mesurerait que le coût des sondes, en dissimulant celui de la
transformation. Le JAR du mod, lui, reste en place des deux côtés : il porte
l'enregistreur, et ce qui est présent des deux côtés ne pèse pas sur la différence.

- **`comparison.trustworthy`** ne vaut `true` que si les deux configurations sont
  conformes et non rejetées. Un écart entre deux campagnes douteuses n'est pas une
  mesure de coût.

Les ticks contaminés par une collecte mémoire sont **marqués et comptés**
(`gc_contaminated_ticks`), jamais supprimés : les retirer embellirait la mesure.

## Coût de la télémétrie (T-401)

```bash
./benchmarks/run-telemetry.sh <racine_du_serveur>   # ~50 minutes
```

R-561 borne la télémétrie à 0,2 % du MSPT. Un tel écart est hors de portée du niveau B :
la dispersion entre exécutions y dépasse plusieurs pour cent. La part Java est donc
chronométrée en place, sur le fil du serveur, par `TelemetryCost` ; la part native par
le micro-benchmark `tick_window/cycle`. `rfx-bench telemetry` juge le **pire** des cinq
rapports et écrit `results/telemetry-<horodatage>-<commit>.json`. Méthode, périmètre et
limites : ADR-034.

## Coût d'un graphe d'appel (C-07)

```bash
"$JDK/bin/jimage" extract --dir <jdk-extrait> "$JDK/lib/modules"
python benchmarks/callgraph-corpus.py \
    build/classpath/runGameTestServer_minecraftClasspath.txt <jdk-extrait> \
    target/callgraph-corpus.bin
cargo run -p rfx-bench --release -- callgraph target/callgraph-corpus.bin [répétitions] [racines]
```

Le corpus réunit les classes du jeu de développement (archives du classpath écrit par
ForgeGradle après un `runGameTestServer`) et celles du JDK. Il reste sous `target/` :
il contient des classes de Minecraft, de Forge et du JDK, qu'on ne redistribue pas.
L'analyse native est pilotée comme en jeu, sans Java ni frontière FFI ; chaque
répétition repart d'une analyse neuve. Résultat :
`results/callgraph-<horodatage>-<commit>.json`, un échantillon par graphe et par
répétition. Méthode, racines choisies et limites : ADR-052.

## Acceptance de C-07

```bash
./benchmarks/run-callgraph-acceptance.sh "$JDK/bin/java"
```

Sur le même corpus, le graphe de 200 méthodes de référence (classes serveur de
Minecraft, échantillon régulier) est construit comme en jeu ; le taux de résolution de
leurs sites doit dépasser 80 %. Puis chaque site résolu de tous les graphes est confronté
à la JVM par `CallGraphOracle.java` : classes chargées sans initialisation, résolution et
sélection JVMS réécrites sur la réflexion (ASM en repli quand elle échoue), rien de
partagé avec le code Rust. Un seul faux résolu, ou un site que l'oracle ne peut pas
trancher, et l'acceptance est refusée. Le `java` doit être celui du JDK dont viennent
les classes du corpus. Résultat : `results/callgraph-acceptance-<horodatage>-<commit>.json`.
Méthode et limites : ADR-053.

## Décodeur d'instructions de C-11

```bash
cargo run -p rfx-bench --release -- insn target/callgraph-corpus.bin
```

Sur le même corpus, chaque méthode est décodée et sa pile rejouée ; la hauteur obtenue
doit égaler le `max_stack` écrit par le compilateur, que la JVM fait respecter. Une
hauteur supérieure, un rejeu impossible ou une variable locale hors bornes trahit une
erreur du décodeur, et le code de sortie vaut `1`. Ce n'est pas une mesure de
performance : aucun temps n'y est rapporté. Résultat :
`results/insn-<horodatage>-<commit>.json`. Méthode : ADR-055.

## Interprétation intraprocédurale de C-11

```bash
cargo run -p rfx-bench --release -- dataflow target/callgraph-corpus.bin [max_cells] [max_visites]
```

Analyse chaque méthode du corpus sous le budget donné (illimité par défaut, pour le
mesurer ; `65536 64` reproduit le budget du jeu). Une méthode jugée incohérente est une
erreur de l'interpréteur, puisque la JVM accepte ce code : le code de sortie vaut alors
`1`. Le rapport donne aussi la distribution du travail par méthode (blocs, emplacements
d'état, passages), qui fixe le budget, puis ce que les effets intraproceduraux en
tirent avec la hiérarchie complète du corpus : précision des écritures, domaines écrits,
et ancres de la table des domaines absentes du corpus (il ne doit y en avoir aucune).
Enfin, sur les 200 méthodes de référence de l'acceptance de C-07, graphes construits
comme en jeu : effets composés de chaque unité, précision, causes d'inconnu et raisons
de non-résolution des arêtes.
Les durées viennent d'un seul passage : ce sont
des observations, pas un benchmark au sens de R-581. Résultat :
`results/dataflow-<horodatage>-<commit>.json`. Méthode : ADR-055.

## Lire un résultat

```json
{
  "schema": 1,
  "commit": "5cf192e…",
  "dirty": false,
  "level": "micro",
  "hardware": { "physical_cores": 8, "logical_cores": 16, "ram_gb": 31, "probed": true },
  "metrics": {
    "ingest/exit": {
      "median": 4.91, "iqr": 0.25,
      "ci_lower": 4.87, "ci_upper": 5.02, "confidence_level": 0.95,
      "samples": 100, "unit": "ns"
    }
  }
}
```

Trois champs décident de ce qu'on peut en dire :

- **`dirty`** — `true` signifie que l'arbre de travail portait des modifications non
  commitées. Le résultat ne décrit alors aucun état publiable : il sert à décider, pas
  à publier.
- **`hardware.probed`** — `false` signifie que la sonde matérielle n'a pas mesuré la
  machine. Les valeurs voisines ne sont alors pas des mesures.
- **`iqr`** — la dispersion. Un écart interquartile du même ordre que la médiane
  signale une mesure instable : charge externe, gel de fréquence, échantillon trop
  petit. La médiane seule ne le dirait pas.

## Comparer deux résultats

Comparer des fichiers mesurés sur des machines différentes n'a pas de sens. La
comparaison utile est entre deux commits **sur la même machine, au repos**. La PARTIE
21.4 fixe le seuil de non-régression à 3 % sur le MSPT p95 — donc au niveau B.

## Ce qui n'est pas mesuré, et qu'il faut savoir

- **Le coût Java de `RfxProbes.enter/exit`**, sur le chemin le plus chaud du jeu. Un
  chronométrage Java naïf mentirait — la JVM élimine le code dont le résultat n'est pas
  utilisé — et JMH est une dépendance à peser séparément. T-131 reste ouvert.
- **Le coût sur un modpack réel** : le niveau B mesure aujourd'hui un serveur nu, sans
  aucun mod tiers. Les profils de la PARTIE 22 restent à constituer, et l'acceptation
  « overhead < 2 % » porte sur eux.
- **Le client** : FPS, frametime, 1 % low. Le niveau B ne couvre que le serveur dédié.
- **Le coût du franchissement FFI**, mesuré au démarrage par C-45 et publié dans
  `/rfx status`, mais pas encore versionné ici.
