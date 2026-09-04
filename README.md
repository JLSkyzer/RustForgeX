# RUSTFORGE-X

**Runtime adaptatif Java/Rust pour Minecraft Forge.**

RUSTFORGE-X observe un modpack Forge en fonctionnement, analyse ce qu'il fait
réellement, et ne déporte du travail vers du code natif que lorsque la correction de
cette transformation est prouvée. Quand la preuve manque, il reste en Java. Le jeu doit
se comporter exactement comme sans le mod.

- Cible : **Minecraft Java Edition 1.20.1 + Forge 47.x (Java 17)**
- Périmètre : client, serveur dédié, nombre arbitraire de mods Forge

## Ce que ce projet n'est pas

- ce n'est pas une réécriture de Minecraft en Rust ;
- ce n'est pas un traducteur automatique de bytecode arbitraire vers du natif : seul un
  sous-ensemble déclaré est supporté (CDC PARTIE 10.2), et cette limite est définitive ;
- ce n'est pas un mod qui promet un gain chiffré : le gain dépend entièrement du
  modpack, il est mesuré localement, jamais garanti.

## État de maturité

> **M0 (Bootstrap) — non démarré.**
>
> Le dépôt contient aujourd'hui le squelette Forge, le cahier des charges versionné et
> l'outillage de travail. **Aucun composant fonctionnel n'est implémenté.** Aucune
> fonctionnalité n'est `STABLE`, aucun benchmark n'a été exécuté.

Progression détaillée : [`tasks/todo.md`](tasks/todo.md) · état pour un agent
reprenant le projet : [`docs/AGENT.md`](docs/AGENT.md).

## Installation

Non disponible : aucun JAR n'a encore été publié. La procédure sera décrite dans
`INSTALLATION.md` à la première release (jalon M0).

## Construire depuis les sources

```bash
./gradlew build
```

Prérequis et commandes complètes : [`BUILDING.md`](BUILDING.md).

## Documentation

| Fichier | Contenu |
|---|---|
| [`docs/spec/`](docs/spec/) | cahier des charges V1.0 — **source de vérité du projet** |
| [`docs/AGENT.md`](docs/AGENT.md) | carte du dépôt, jalons, invariants, procédures |
| [`docs/decisions/`](docs/decisions/) | ADR — une décision structurante par fichier |
| [`BUILDING.md`](BUILDING.md) | prérequis, commandes, dépannage de build |

Les documents `ARCHITECTURE.md`, `INSTALLATION.md`, `CONFIGURATION.md`,
`COMPATIBILITY.md`, `TROUBLESHOOTING.md`, `BENCHMARKS.md`, `RELEASING.md`,
`SECURITY.md` et `RUST_MOD_SDK.md` sont exigés par la PARTIE 27.1 du cahier des charges
et seront rédigés au jalon où leur contenu devient réel — jamais avant, pour ne pas
documenter des fonctionnalités inexistantes.

## Licence

Tous droits réservés. Voir [`LICENSE`](LICENSE).

RUSTFORGE-X ne redistribue ni Minecraft, ni Forge, ni aucun mod ou asset tiers.
