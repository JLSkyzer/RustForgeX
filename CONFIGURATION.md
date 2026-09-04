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
| `telemetry.enabled` | booléen | `true` | `true` / `false` | non | Collecte **locale** des métriques. Aucune donnée ne quitte la machine, jamais. |
| `runtime.panic_threshold` | entier | `3` | `1` .. `100` | non | Panics tolérées pour un sous-système avant sa désactivation (R-523). |

### Les autres sections de la PARTIE 28.2

La configuration normative du cahier des charges décrit aussi les sections
`profiler`, `analysis`, `decision`, `scheduler`, `snapshot`, `commit`, `validation`,
`shadow`, `cache`, `mirror`, `ir`, `network`, `learning`, `diagnostics`, `ui` et
`overrides`.

Elles **n'existent pas encore** dans le fichier généré. Elles pilotent des composants
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

La commande affiche le mode effectif, l'état du runtime, la classe matérielle mesurée
et la maturité de chaque composant. Un champ que la sonde n'a pas mesuré est affiché
« non mesuré » : aucun zéro n'y passe pour une mesure.
