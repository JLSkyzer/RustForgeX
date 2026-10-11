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
