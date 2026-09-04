# SECURITY

Modèle de menace, code `unsafe`, dépendances et signalement.
Référence : cahier des charges, PARTIE 19.

## Ce que RUSTFORGE-X ne fait jamais

- **Aucun accès réseau.** RUSTFORGE-X n'ouvre aucune connexion, n'émet aucune requête,
  ne contacte aucun service. Ni télémétrie, ni vérification de mise à jour, ni
  rapport d'erreur distant (INV-15, ADR-012). Ce qui est mesuré sur votre machine
  reste sur votre machine.
- **Aucune donnée personnelle collectée.** Le hachage de classe matérielle
  (`class_hash`) est calculé sur des **paliers** — palier de cœurs, présence d'AVX2,
  palier de mémoire — précisément pour que deux machines semblables produisent la même
  valeur. Il identifie une classe de matériel, jamais une machine ni un utilisateur.
- **Aucune redistribution.** Le JAR ne contient ni Minecraft, ni Forge, ni aucun mod
  ou asset tiers. Le job de vérification d'artefact échoue si une entrée inattendue
  apparaît dans l'archive.

## Surface d'attaque

| Surface | Traitement |
|---|---|
| Fichier de configuration | valeurs validées contre un schéma typé et borné ; une valeur invalide est rejetée et remplacée par le défaut |
| Binaire natif embarqué | condensé SHA-256 vérifié **avant** tout chargement ; un binaire substitué est refusé et le runtime passe en `DISABLED` |
| Données franchissant la frontière FFI | pointeurs nuls, longueurs aberrantes et handles inconnus rejetés avant tout usage ; les tampons sont bornés |
| Panics du code natif | capturées à chaque point d'entrée, comptées, et le runtime s'arrête de lui-même au-delà de 10 panics en 60 secondes |

### Vérification du binaire natif

Chaque binaire empaqueté est accompagné de son condensé SHA-256, produit au build.
Au démarrage, C-03 relit le binaire embarqué, recalcule son condensé et refuse de
charger quoi que ce soit en cas d'écart (R-300, `E-1003`). Le fichier est ensuite
extrait dans un chemin **versionné par ce condensé**, si bien qu'un binaire modifié ne
peut jamais écraser un binaire vérifié.

Vous pouvez vérifier vous-même le contenu d'un JAR :

```bash
unzip -p rustforgex-<version>.jar natives/windows-x86_64/rfx_native.dll.sha256
unzip -p rustforgex-<version>.jar natives/windows-x86_64/rfx_native.dll | sha256sum
```

Les deux valeurs doivent coïncider.

## Code `unsafe`

Le code Rust est écrit en `unsafe` uniquement là où la tâche l'impose, et chaque bloc
porte sa justification dans le code même :

| Emplacement | Raison | Portée |
|---|---|---|
| `crates/rfx-ffi` | ce crate **est** la frontière : il reçoit des pointeurs de la JVM | chaque fonction documente ses préconditions ; toutes valident leurs arguments avant usage |
| `crates/rfx-core/src/hw/windows.rs` | deux appels Win32 pour la topologie processeur et la mémoire | aucun pointeur ne survit à l'appel ; structures `#[repr(C)]` d'entiers ; un échec renvoie une sonde vide |

Partout ailleurs, `unsafe` est interdit par configuration du crate
(`unsafe_code = "deny"` ou `"forbid"`), donc refusé à la compilation.
`crates/rfx-core/src/hw/linux.rs` n'en contient aucun : tout provient de `/proc` et
`/sys` en lecture seule.

## Dépendances (SBOM)

Le projet limite volontairement ses dépendances. À ce jalon :

| Dépendance | Rôle | Licence |
|---|---|---|
| `serde` | dérivation de sérialisation | MIT ou Apache-2.0 |
| `ciborium` | encodage CBOR côté natif | Apache-2.0 |
| `jni` | types et conventions d'appel JNI | MIT ou Apache-2.0 |

Côté Java, **aucune dépendance d'exécution** n'est ajoutée au JAR : l'encodage CBOR et
la lecture du fichier de configuration sont écrits dans le projet, restreints au
strict nécessaire. JUnit 5 est la seule dépendance, et seulement pour les tests.

La sonde matérielle n'utilise aucune bibliothèque tierce : elle appelle directement le
système, ce qui évite d'ajouter du code non audité au chemin de démarrage.

## Confinement des pannes

Un défaut de RUSTFORGE-X ne doit jamais empêcher de jouer :

- toute exception au démarrage conduit à `DEGRADED` — le jeu tourne exactement comme
  sans le mod, sans instrumentation ni thread supplémentaire ;
- toute exception dans une accroche Forge est capturée et comptée ; l'accroche est
  désactivée après cinq échecs ;
- aucune panic Rust ne peut atteindre la JVM : le binaire est compilé avec
  `panic = "unwind"` et chaque point d'entrée est enveloppé dans `catch_unwind`. Ce
  comportement est vérifié par un test qui provoque une vraie panic à travers la
  frontière et constate qu'elle en ressort sous forme de code d'erreur.

Retirer le JAR du dossier `mods` rétablit à l'identique le comportement d'origine :
RUSTFORGE-X n'écrit rien dans les sauvegardes.

## Signaler une vulnérabilité

Ouvrez un signalement privé auprès du mainteneur du dépôt plutôt qu'un ticket public,
en décrivant l'impact et les étapes de reproduction. Ce projet n'ayant pas encore
publié de version, il n'existe pas de politique de rétroportage ; les correctifs sont
intégrés à la branche de développement.
