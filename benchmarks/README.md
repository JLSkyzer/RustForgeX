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
| **B — macro** | MSPT, TPS, FPS, frametime, allocations, **overhead de RUSTFORGE-X** | à écrire |

**Le niveau A ne répond pas à « quel est l'overhead ? ».** Il mesure des morceaux, pas
le tout. Seul le niveau B — serveur dédié scripté, monde à graine figée, comparaison des
configurations B et C de la PARTIE 21.2 — peut y répondre. Ne pas faire dire au premier
ce que seul le second sait.

## Lancer le niveau A

```bash
cargo bench -p rfx-bench
cargo run -p rfx-bench --release
```

La première commande mesure et écrit les échantillons bruts sous `target/criterion/`.
La seconde les relit et produit `benchmarks/results/micro-<horodatage>-<commit>.json`
au schéma de la PARTIE 21.3.

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
- **L'overhead réel en jeu**, qui relève du niveau B.
- **Le coût du franchissement FFI**, mesuré au démarrage par C-45 et publié dans
  `/rfx status`, mais pas encore versionné ici.
