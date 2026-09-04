# BUILDING

Construction de RUSTFORGE-X depuis les sources. Référence normative : cahier des
charges, PARTIE 23.

## Prérequis

| Outil | Version | Note |
|---|---|---|
| JDK | 17 | téléchargé automatiquement par la toolchain Gradle si absent |
| Gradle | 8.8 | fourni par le wrapper (`./gradlew`), ne pas installer |
| ForgeGradle | 6.x | résolu par le build |
| Rust | stable, épinglé par `rust-toolchain.toml` | requis à partir de M0 |
| `cargo`, `rustfmt`, `clippy` | fournis par rustup | |
| `cbindgen` | optionnel | génération d'en-têtes de contrôle |

Cibles natives visées : `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu` (best effort).

> Le workspace Rust (`crates/`, `Cargo.toml`, `rust-toolchain.toml`) n'existe pas
> encore : il est créé au début du jalon M0. Les commandes `cargo` ci-dessous ne
> s'appliquent qu'à partir de là.

## Organisation du build

Gradle est le **point d'entrée unique**. La tâche `:buildNative` appellera `cargo` pour
chaque cible activée ; les artefacts natifs seront copiés dans
`src/main/resources/natives/<os>-<arch>/` avec leur fichier `.sha256`, puis empaquetés
dans le JAR.

## Commandes

```bash
# nettoyage complet
./gradlew clean && cargo clean

# compilation
./gradlew compileJava

# tests
./gradlew test
cargo test

# lint
./gradlew check
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# vérifier les cibles que cette machine ne compile pas nativement
# (`check` n'a pas besoin d'un éditeur de liens croisé)
rustup target add x86_64-unknown-linux-gnu
cargo check --target x86_64-unknown-linux-gnu

# build complet du JAR (natif de la plateforme hôte inclus)
./gradlew build
```

Le premier build télécharge et décompile Minecraft : comptez plusieurs minutes et
plusieurs Go de cache dans `~/.gradle`.

> Tout code placé derrière un `#[cfg(target_os = ...)]` est ignoré par le compilateur
> local. Après un renommage transverse, le `cargo check --target` ci-dessus est le seul
> moyen de constater qu'on n'a pas cassé une plateforme qu'on ne construit pas ici.

## Lancer le jeu en développement

```bash
./gradlew runClient          # client de dev,       répertoire de travail : run/
./gradlew runServer          # serveur dédié,       répertoire de travail : run/
./gradlew runGameTestServer  # exécute les gametests puis quitte
./gradlew runData            # datagen vers src/generated/resources/
```

## Dépannage

| Symptôme | Cause probable | Action |
|---|---|---|
| `Could not resolve net.minecraftforge:forge` | réseau ou maven Forge indisponible | vérifier la connexion, relancer ; le maven Forge est déclaré dans `settings.gradle` |
| Build très lent au premier lancement | décompilation de Minecraft | normal, une seule fois ; ne pas interrompre |
| `Unsupported class file major version` | JDK autre que 17 utilisé | la toolchain Gradle impose Java 17, vérifier `java.toolchain` dans `build.gradle` |
| Mojibake dans le nom ou la description du mod en jeu | caractère accentué dans `gradle.properties` | ce fichier est lu en ISO-8859-1 : le garder en ASCII pur |
| Ressources non rafraîchies au lancement depuis l'IDE | `copyIdeResources` désactivé | il DOIT rester à `true` dans le bloc `minecraft` |

En cas d'échec de build, joindre la sortie de `./gradlew build --stacktrace` au rapport.
