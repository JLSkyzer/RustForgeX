# ARCHITECTURE

Couches, composants et flux de RUSTFORGE-X.
Référence normative : cahier des charges, PARTIE 3. Ce document décrit **ce qui
existe**, pas la cible complète : la cible est dans la spécification.

## Principe directeur

Java reste l'état autoritatif. RUSTFORGE-X observe, analyse, et ne déporte du travail
vers du code natif que lorsque la correction de cette transformation est prouvée.
Quand la preuve manque, il ne fait rien. C'est la règle qui décide de tous les
arbitrages de ce document.

## Couches

```text
┌──────────────────────────────────────────────────────────┐
│  Forge / Minecraft                                       │
└──────────────────────────┬───────────────────────────────┘
                           │  événements de cycle de vie, commandes
┌──────────────────────────▼───────────────────────────────┐
│  Java — dev.rustforgex                                   │
│    forge/      C-01  ancrage Forge, gardes, versions     │
│    bootstrap/  C-02  séquence de démarrage               │
│                C-03  extraction et chargement du natif   │
│                C-45  calibration du coût de frontière    │
│    config/     C-37  configuration validée               │
│    command/    C-38  /rfx status                         │
│    diag/       C-35  codes d'erreur                      │
│    bridge/     IF-01 déclaration de l'ABI, CBOR          │
└──────────────────────────┬───────────────────────────────┘
                           │  JNI + DirectByteBuffer (ADR-004)
┌──────────────────────────▼───────────────────────────────┐
│  Rust — crates/                                          │
│    rfx-ffi     IF-01 points d'entrée, capture des panics │
│    rfx-core    C-27  état global, erreurs                │
│                C-45  sonde matérielle                    │
│    rfx-model   DM-14 modèle canonique, CBOR              │
└──────────────────────────────────────────────────────────┘
```

Le sens des dépendances est strict et vérifié : `rfx-ffi` → `rfx-core` → `rfx-model`,
jamais l'inverse (INV-13). `rfx-core` ne connaît ni Java, ni la JVM, ni Forge :
toute la frontière est contenue dans `rfx-ffi`.

## Frontière Java ↔ natif

**Décision fondatrice (ADR-004)** : JNI classique, avec `DirectByteBuffer` comme mode
de transfert. Panama n'est pas stable sur Java 17 ; JNA et JNR ajouteraient un surcoût
et une dépendance.

**Règle de conception (R-700)** : la frontière se franchit **par lot et par tick**,
jamais par élément. Mille appels JNI sont remplacés par un appel portant un tampon de
mille éléments. Le budget cible est inférieur à 50 traversées par tick en régime
établi, quelle que soit la charge.

L'ABI est en C pur, versionnée indépendamment du produit :

- toute fonction renvoie un `i32` : `0` pour un succès, la valeur **négative** d'un
  code de l'annexe A.2 sinon ;
- aucune structure `repr(Rust)` ne traverse : uniquement des entiers et des tampons
  CBOR à schéma versionné ;
- `rfx_abi_version()` est appelée avant toute autre fonction ; un écart de version
  interdit tout autre appel ;
- **chaque** point d'entrée est enveloppé dans `catch_unwind` : aucune panic Rust ne
  peut atteindre la JVM.

Le module `jni_bridge` n'implémente rien : il traduit les types JNI vers cette ABI, qui
reste la seule implémentation. La validation des handles et la capture des panics ne
sont donc écrites qu'une fois.

### Handles

Le handle remis à Java est un entier opaque, jamais un pointeur. Il porte un motif de
poids fort et un numéro de génération : un entier arbitraire ne peut pas passer pour
un handle, et un handle libéré n'est jamais revalidé, même après une réinitialisation.

## Séquence de démarrage

```text
INIT ──▶ PROBE ──▶ LOAD_NATIVE ──▶ HANDSHAKE ──▶ CONFIGURE ──▶ READY
  │        │            │              │             │
  │        │            │              │             └─▶ DEGRADED  (init refusée)
  │        │            │              └───────────────▶ DISABLED  (ABI incompatible)
  │        │            └──────────────────────────────▶ DEGRADED  (natif absent)
  │        │                                             DISABLED  (binaire altéré)
  │        └───────────────────────────────────────────▶ DEGRADED  (plateforme inconnue)
  └────────────────────────────────────────────────────▶ DISABLED  (enabled = false)
```

`DEGRADED` et `DISABLED` ne sont pas des pannes : ce sont des états de fonctionnement
normal dans lesquels le jeu tourne exactement comme sans le mod. Aucune exception ne
remonte jusqu'à Forge — un mod d'optimisation qui empêche le jeu de démarrer est pire
que pas de mod du tout.

## Chargement du binaire natif

```text
lire  /natives/<os>-<arch>/<lib>.sha256        (condensé attendu)
lire  /natives/<os>-<arch>/<lib>               (binaire embarqué)
vérifier le condensé  ─── écart ──▶  refus, DISABLED, E-1003
extraire vers <gameDir>/rustforgex/native/<condensé>/<lib>
   ├── déjà présent et conforme ──▶ réutilisé tel quel
   └── sinon : fichier temporaire, revérification, renommage atomique
System.load(...)
```

Le chemin est versionné par le condensé : plusieurs versions coexistent sans conflit,
et un binaire modifié ne peut pas écraser un binaire vérifié. Si la racine de jeu
refuse l'écriture, le répertoire temporaire de la JVM est tenté avant d'échouer.

## Mesure du matériel

La sonde ne devine rien. Ce qu'une plateforme ne permet pas de mesurer reste à zéro,
et la couverture de sonde le déclare explicitement, pour qu'aucun consommateur ne
puisse confondre une mesure et une valeur par défaut.

Deux coûts ne sont pas mesurables depuis le natif, car ils incluent le trajet
aller-retour depuis la JVM : le coût d'un appel minimal, mesuré sur 10 000 appels
après chauffe, et le débit de transfert, mesuré sur 1 Kio, 64 Kio et 1 Mio. Java les
mesure et les publie dans la classe matérielle.

Le hachage de classe matérielle porte sur des **paliers**, pas sur des valeurs
exactes : deux machines semblables produisent le même hachage, ce qui rendra les
caches partageables sans jamais identifier une machine.

## Ce qui n'existe pas encore

Le jalon M0 ne pose **aucun hook de tick**. Le livrable exige que le jeu tourne
exactement comme sans le mod : s'accrocher au tick pour n'y rien faire coûterait du
temps à chaque tick sans rien apporter. Le cycle `rfx_tick_begin` / `rfx_phase` /
`rfx_tick_end` (IF-02) sera branché avec l'instrumentation et le profileur, au jalon
M1, quand il aura quelque chose à observer.

De même, aucun ordonnanceur, aucun moteur de décision, aucun cache et aucune
transformation n'existent. La carte complète des composants C-01 à C-53 et l'ordre de
leur arrivée sont dans `docs/AGENT.md` et dans la PARTIE 30 du cahier des charges.

## Décisions d'architecture

Les décisions structurantes sont dans `docs/decisions/`. Celles qui portent ce jalon :

- **ADR-013** — sources Java à la racine plutôt que sous `java/`
- **ADR-014** — les ADR de l'agent sont numérotés à partir de 013
- **ADR-015** — points d'entrée ABI et clé de configuration ajoutés au jalon M0

Les ADR-001 à ADR-012, dont le contenu est arrêté par le cahier des charges, restent à
rédiger au jalon où leur décision devient effective.
