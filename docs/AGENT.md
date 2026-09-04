# docs/AGENT.md — reprise de projet sans contexte antérieur

Ce fichier permet à un agent de reprendre RUSTFORGE-X **sans aucune conversation
antérieure**. Il est normatif au sens de la PARTIE 27.2 du cahier des charges.

> **Source de vérité** : `docs/spec/RUSTFORGE-X_Cahier_des_Charges_v1.0.md`.
> En cas de doute, c'est ce document qui tranche, pas ce fichier, pas le code.

---

## 0. État courant du projet

**Jalon en cours : M0 (Bootstrap) — composants livrés, jalon en cours de clôture.**

| Élément | État |
|---|---|
| Squelette Forge (MDK 1.20.1 / Forge 47.4.23) | en place |
| Workspace Rust (`crates/`) | `rfx-model`, `rfx-core`, `rfx-ffi` |
| C-27 Rust Runtime Core | implémenté, `STABLE` |
| C-45 Hardware Probe | implémenté, `STABLE` |
| C-03 Native Loader | implémenté, `STABLE` |
| C-02 Bootstrap | implémenté, `STABLE` |
| C-37 Configuration | implémenté, `STABLE` (options du jalon uniquement) |
| C-01 Forge Integration | implémenté, `STABLE` (aucun hook de tick à ce jalon) |
| C-38 CLI | `/rfx status` seulement, `STABLE` |
| C-40 Release System | build natif, artifact-verify et CI en place |
| Composants C-04 à C-53 | **non implémentés** |
| Tests | 55 côté Java, 47 côté Rust, tous verts |
| Benchmarks B-xx | **aucun** |

Le mod a été lancé dans Minecraft : il démarre, charge son binaire natif, mesure la
machine, et s'arrête proprement. Aucun chiffre de performance n'existe et aucun ne
doit être écrit nulle part tant que le harnais de bench (C-36, jalon M1) n'en a pas
produit un fichier de résultats.

**Reste à faire pour clore M0** : `INSTALLATION.md` et `RELEASING.md` (à la première
release), et la vérification sur serveur dédié et client réels.

---

## 1. Carte du dépôt

```text
RustForgeX/
├── build.gradle                  point d'entrée unique du build (Gradle + ForgeGradle 6)
├── gradle.properties             versions MC/Forge, mod_id, groupId, mappings
├── settings.gradle
├── cahier des charges/           CDC original fourni par le client (ne pas modifier)
├── docs/
│   ├── AGENT.md                  ce fichier
│   ├── spec/                     copie versionnée du CDC (source de vérité)
│   ├── decisions/                ADR (voir section 9)
│   └── diagrams/
├── src/main/java/dev/rustforgex/ code Java (voir la table ci-dessous)
├── src/main/resources/           mods.toml, pack.mcmeta, mixins, natives/ (générés)
├── src/generated/resources/      sortie de datagen (tâche runData)
├── tasks/todo.md                 plan de travail courant
├── tasks/lessons.md              règles apprises + interdits du contrat agent
└── crates/                       workspace Rust (rfx-model, rfx-core, rfx-ffi)
```

> **Écart assumé au CDC** : la PARTIE 3.6 place les sources Java sous `java/src/`.
> Elles sont à la racine (`src/`), cf. **ADR-013**. Lire la PARTIE 3.6 en substituant
> `java/src/` par `src/`. Le découpage en sous-paquets est inchangé.

### Correspondance composant vers emplacement

| Composants | Emplacement cible |
|---|---|
| C-01 Forge Integration, C-06 Event Observer, C-41 Mod Discovery | `src/main/java/dev/rustforgex/forge/` |
| C-02 Bootstrap, C-03 Native Loader | `src/main/java/dev/rustforgex/bootstrap/` |
| C-04 JVM Instrumentation | `src/main/java/dev/rustforgex/instrument/` |
| IF-01..IF-06 côté Java | `src/main/java/dev/rustforgex/bridge/` |
| C-19 Snapshot (Java), C-22 Commit (Java) | `snapshot/`, `commit/` |
| C-37 Configuration | `src/main/java/dev/rustforgex/config/` |
| C-38 CLI et commandes | `src/main/java/dev/rustforgex/command/` |
| C-46 Dashboard UI | `src/main/java/dev/rustforgex/ui/` |
| C-35 Diagnostics | `src/main/java/dev/rustforgex/diag/` |
| C-27 runtime, C-31, C-34, C-17/18, C-16, DM-xx, ... | `crates/rfx-*` (cf. CDC PARTIE 3.6) |

---

## 2. Construire, tester, lancer, benchmarker

Prérequis : JDK 17 (téléchargé automatiquement par la toolchain Gradle), Rust stable
épinglé par `rust-toolchain.toml`, `cargo`, `rustfmt`, `clippy`.
Commandes exactes : voir `BUILDING.md` et CDC PARTIE 23.3.

```bash
./gradlew compileJava
./gradlew build
./gradlew test
cargo test
./gradlew clean && cargo clean
```

Le premier build télécharge et décompile Minecraft : comptez plusieurs minutes.

---

## 3. Lancer un serveur et un client de test

```bash
./gradlew runClient
./gradlew runServer
./gradlew runGameTestServer
./gradlew runData
```

`runClient` / `runServer` utilisent `run/` comme répertoire de travail, `runData`
utilise `run-data/` et écrit dans `src/generated/resources/`. Les run configurations
IntelliJ équivalentes sont dans `.idea/runConfigurations/`.

---

## 4. Identifiants normatifs

Définis en PARTIE 0.4 du CDC :

```text
R-xxx exigence       C-xx composant      IF-xx interface     DM-xx modèle de données
SM-xx machine états  INV-xx invariant    FM-xx défaillance   RISK-xx risque
T-xxx test           B-xx benchmark      ADR-xxx décision    M-x jalon
E-xxxx code d'erreur
```

Toute implémentation DOIT citer l'identifiant concerné dans le commentaire de tête du
module ET dans le message de commit :

```text
feat(C-17): work stealing deque [R-420, T-260]
```

---

## 5. Ordre d'implémentation (jalons M0..M10)

Chaque jalon produit un **JAR installable et jouable**. Aucun jalon ne laisse le
projet dans un état non fonctionnel.

| Jalon | Contenu | Livrable |
|---|---|---|
| **M0** | C-01, C-02, C-03, C-27 (squelette), C-37, C-45, C-40 | JAR qui se charge, charge le natif, sonde le matériel, expose `/rfx status`, ne fait rien d'autre |
| M1 | C-04, C-05, C-06, C-31, C-34, C-35, C-38, C-41, C-36 | profilage adaptatif complet, overhead < 2 % mesuré |
| M2 | C-07..C-10, C-12, C-16, C-17, C-18, C-48 | task graph + scheduler, aucune transformation du jeu |
| M3 | Décision | moteur de décision, toujours sans exécution transformée |
| M4 | Exécution sûre | snapshots, command buffers, commit, validation |
| M5 | Adaptation et domaine entités | apprentissage + C-49 |
| M6 | IR et chunks | RF-IR + C-50 |
| M7 | Compilation et client | backend JIT + C-53 |
| M8 | SDK | C-39 |
| M9 | Durcissement | sécurité, confinement, watchdog |
| M10 | Stabilisation | release candidate |

Détail complet et Definition of Done : CDC PARTIES 29 et 30.

---

## 6. Invariants à ne jamais violer (CDC PARTIE 8.1)

| ID | Invariant |
|---|---|
| INV-01 | le task graph est acyclique |
| INV-02 | l'état autoritatif n'est muté que sur le thread autoritatif |
| INV-03 | aucun effet non validé n'atteint l'état autoritatif |
| INV-04 | aucune exécution IR/native sans validation shadow complète |
| INV-05 | mêmes entrées et même fingerprint donnent une séquence de commandes commitées identique |
| INV-06 | aucun worker n'acquiert de verrou JVM ni de moniteur Java |
| INV-07 | aucun thread natif ne s'attache à la JVM |
| INV-08 | le thread autoritatif n'attend jamais un worker au-delà de la deadline de drain |
| INV-09 | tout snapshot utilisé pour produire des commandes est revalidé au commit |
| INV-10 | l'overhead total de RF-X est mesuré et borné |
| INV-11 | toute décision est explicable |
| INV-12 | aucune logique du moteur ne dépend d'un nom de mod |
| INV-13 | aucune dépendance de couche inférieure vers couche supérieure |
| INV-14 | aucune allocation Java dans le chemin de sonde |
| INV-15 | aucun accès réseau initié par RF-X |

Un invariant qui gêne ne se contourne pas : c'est le code qui est faux (contrat 3.6).

---

## 7. Procédure de debug

1. Logs Forge : `run/logs/latest.log` et `run/logs/debug.log`
   (`forge.logging.console.level=debug` est déjà actif dans `build.gradle`).
2. `/rfx status` et `/rfx why <workload>` en jeu (C-38, C-43), dès M0/M1.
3. `rfx-cli` pour l'analyse de dumps hors-jeu (C-38).
4. Diagnostics persistés sous `<gameDir>/rustforgex/` (C-48).
5. Crash JVM : `hs_err_pid*.log` à la racine du répertoire de travail du run.

## 8. Quand un test échoue

1. **Ne jamais désactiver le test** (contrat 3.12).
2. Reproduire en isolation : `./gradlew test --tests "<pattern>"` ou `cargo test <nom>`.
3. Un test instable (flaky) est un **défaut réel**, jamais du bruit (contrat 6.3).
4. Si un invariant est en cause : corriger le code, pas l'invariant.
5. Si un gain n'est pas reproductible, il n'existe pas (contrat 6.4).

## 9. Commits et PR

- Format : `type(C-xx): description [R-xxx, T-xxx]`
- Types : `feat`, `fix`, `test`, `docs`, `refactor`, `bench`, `chore`, `build`.
- Un commit touchant du code cite au moins un identifiant normatif (contrat 4.5).
- Une PR ne passe que si build, tests et lints passent (CDC PARTIE 24.3).

**ADR** : `ADR-001` à `ADR-012` sont réservés aux décisions obligatoires listées en
CDC PARTIE 27.3 et restent à rédiger. Les ADR de l'agent commencent à `ADR-013`
(cf. **ADR-014**). Toute divergence volontaire au CDC exige un ADR daté (contrat 1.4).

## 10. Ce qu'il ne faut jamais faire

Liste intégrale : FINAL AGENT EXECUTION CONTRACT section 3, recopiée dans
`tasks/lessons.md`. En résumé : pas de placeholder dans du STABLE, pas de chiffre
inventé, pas de nom de mod dans le moteur, pas de mutation hors thread autoritatif,
pas d'invariant affaibli, pas de panic Rust à travers la FFI, pas de test désactivé,
pas de réseau, pas de redistribution de Minecraft, Forge, mods ou assets tiers.

**L'agent NE DOIT PAS décider seul** (contrat section 5) : changer un contrat IF-xx
publié, changer un DM-xx sans migration, changer un invariant, ajouter une dépendance
lourde ou une bibliothèque native tierce, modifier la licence, publier une release.

## 11. Ajouter un composant, une métrique, une option, un test

- **Composant** : lire sa fiche en CDC PARTIE 5 AVANT d'écrire du code. Il DOIT
  exposer ses métriques et son niveau de maturité (contrat 4.1) et avoir un fallback
  Java testé s'il est sur un chemin optimisé (4.2).
- **Option de configuration** : défaut, plage, validation et documentation dans
  `CONFIGURATION.md` (4.6). Fichier : `<gameDir>/rustforgex/config/rustforgex.toml`.
- **Code d'erreur** : ajouter à l'annexe A.2 du CDC (4.7).
- **Structure persistée** : magic, version de schéma et CRC (4.8).
- **Test** : lui donner un identifiant `T-xxx` du catalogue PARTIE 20.3.

## 12. Niveaux de maturité (CDC PARTIE 0.3)

`STABLE` (actif par défaut, testé, benchmarké) · `EXPERIMENTAL` (inactif par défaut) ·
`DISABLED` (présent, désactivé) · `FUTURE` (interfaces et tests de contrat seulement).

Une fonctionnalité `STABLE` ne contient aucun `TODO`, `FIXME`, `unimplemented!()`,
`todo!()`, placeholder ou faux benchmark. Vérifié mécaniquement par le job CI
`lint-no-fiction` (CDC PARTIE 24).
