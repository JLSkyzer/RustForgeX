# RUSTFORGE-X
## Cahier des charges V1.0 ULTIMATE : runtime adaptatif Java/Rust pour Minecraft Forge

**Version : 1.0.0 (spécification d'implémentation)**
**Statut : SOURCE DE VÉRITÉ PRINCIPALE DU PROJET**
**Remplace : V0.3 (Architecture cible radicale et décision d'exécution automatique)**
**Cible initiale : Minecraft Java Edition 1.20.1 + Forge 47.x (Java 17)**
**Périmètre : client + serveur dédié + nombre arbitraire de mods Forge**
**Public visé : agent IA de développement autonome + relecteurs humains**

---

# TABLE DES MATIÈRES

```text
PARTIE 0   Préambule, conventions, hypothèses
PARTIE 1   Audit du V0.3 et registre des corrections
PARTIE 2   Vision, objectifs, principes directeurs
PARTIE 3   Architecture globale et inventaire des composants
PARTIE 4   Modèle de données canonique
PARTIE 5   Spécifications composant par composant (C-01 .. C-48)
PARTIE 6   Contrats techniques et ABI
PARTIE 7   Machines à états
PARTIE 8   Concurrence, correctness et invariants
PARTIE 9   JVM, bytecode, limites dures
PARTIE 10  Pipeline Java vers Rust, RF-IR, compilation
PARTIE 11  Validation, shadow execution, promotion/démotion
PARTIE 12  Profiling adaptatif et budgets
PARTIE 13  Boucle d'apprentissage
PARTIE 14  Caches
PARTIE 15  Architecture client et architecture serveur
PARTIE 16  Sous-systèmes de domaine génériques
PARTIE 17  Mod discovery
PARTIE 18  Compatibilité
PARTIE 19  Sécurité et confinement
PARTIE 20  Stratégie de test
PARTIE 21  Benchmarks
PARTIE 22  Profils de modpack de test
PARTIE 23  Build
PARTIE 24  CI/CD
PARTIE 25  Release et distribution
PARTIE 26  Installation
PARTIE 27  Documentation du dépôt
PARTIE 28  Configuration
PARTIE 29  Definition of Done
PARTIE 30  Roadmap et jalons
PARTIE 31  Critères d'acceptation
PARTIE 32  Annexes
FINAL AGENT EXECUTION CONTRACT
```

---

# PARTIE 0 : PRÉAMBULE, CONVENTIONS, HYPOTHÈSES

## 0.1 Nature du document

Ce document est une **spécification d'implémentation**. Il n'est ni un tutoriel, ni une roadmap commerciale, ni un manifeste.

Il doit permettre à un agent de développement de construire RUSTFORGE-X depuis un dépôt vide jusqu'à un artefact `rustforgex-<version>.jar` réellement compilable, testable, installable et publiable, sans avoir à réinventer les mécanismes fondamentaux du système.

Lorsque ce document ne peut pas spécifier une implémentation exacte (parce qu'elle dépend du code final, du matériel ou de mods inconnus), il fournit systématiquement :

```text
1. le contrat
2. les invariants
3. les contraintes
4. une implémentation de référence
5. les tests qui valident cette implémentation
```

## 0.2 Conventions de langage normatif

| Terme | Signification |
|---|---|
| DOIT / OBLIGATOIRE | exigence stricte, non négociable, testée par la suite d'acceptation |
| NE DOIT PAS | interdiction stricte |
| DEVRAIT | recommandation forte, dérogation autorisée si justifiée et documentée dans `docs/decisions/` |
| PEUT | option laissée à l'agent d'implémentation |
| N/A | non applicable à la phase courante |

## 0.3 Niveaux de maturité

Toute fonctionnalité de RUSTFORGE-X porte exactement un niveau de maturité, exposé dans le code (annotation/attribut), dans la configuration et dans les diagnostics.

| Niveau | Signification | Activé par défaut | Doit compiler | Doit avoir un fallback |
|---|---|---|---|---|
| `STABLE` | spécifié, implémenté, testé, benchmarké, validé | oui | oui | oui |
| `EXPERIMENTAL` | implémenté, partiellement validé, comportement pouvant changer | non | oui | oui |
| `DISABLED` | code présent mais désactivé par compilation ou configuration | non | oui | sans objet |
| `FUTURE` | uniquement spécifié : interfaces, contrats et tests de contrat existent, implémentation absente ou minimale et refusant l'activation | non | oui | oui |

Règle absolue : **une fonctionnalité annoncée `STABLE` ne doit contenir aucun `TODO`, `FIXME`, `unimplemented!()`, `todo!()`, placeholder, faux benchmark ou implémentation factice.** Ce point est vérifié mécaniquement (voir PARTIE 24, job `lint-no-fiction`).

## 0.4 Identifiants normatifs

Le document utilise des identifiants stables, référencés par le code, les tests et les commits :

```text
R-xxx     exigence (requirement)
C-xx      composant
IF-xx     interface / contrat
DM-xx     modèle de données (data model)
SM-xx     machine à états (state machine)
INV-xx    invariant
FM-xx     mode de défaillance (failure mode)
RISK-xx   risque
T-xxx     test
B-xx      benchmark
ADR-xxx   décision d'architecture
M-x       jalon (milestone)
E-xxxx    code d'erreur runtime
```

Toute implémentation DOIT référencer l'identifiant concerné dans les commentaires de tête de module et dans les messages de commit (`feat(C-17): ... [R-214]`).

## 0.5 Hypothèses explicites

Les hypothèses suivantes structurent l'architecture. Si l'une d'elles est invalidée à l'exécution, l'impact indiqué s'applique et le comportement de repli associé DOIT être déclenché automatiquement.

| ID | Hypothèse | Impact si fausse | Repli |
|---|---|---|---|
| H-01 | Minecraft 1.20.1 + Forge 47.x tourne sur Java 17 (LTS) | pas de Panama/FFM stable, pas de `MemorySegment` public | JNI + `DirectByteBuffer` uniquement (ADR-004) |
| H-02 | Le thread serveur ("Server thread") et le thread client ("Render thread"/"main") sont identifiables de façon fiable | affinité de thread non déterminable | tout workload devient `MAIN_THREAD_REQUIRED`, mode observation seul |
| H-03 | Les mods Forge n'utilisent pas massivement la génération dynamique de classes dans les chemins chauds | le fingerprint bytecode est instable, invalidations en cascade | dégradation vers profilage par échantillonnage et `JAVA_ONLY` |
| H-04 | Le coût d'un appel JNI simple est de l'ordre de 20 ns à 100 ns sur les cibles supportées | le modèle de coût de transfert est faux | recalibrage automatique au démarrage (C-45 Hardware Probe) |
| H-05 | La majorité du temps CPU d'un gros modpack est concentrée dans moins de 500 méthodes distinctes | la base de workloads explose | plafond dur `max_tracked_workloads`, éviction LRU |
| H-06 | Le monde Minecraft peut être lu de façon cohérente pendant une fenêtre de tick contrôlée par RUSTFORGE-X | pas de snapshot cohérent possible | snapshots limités aux données copiées explicitement, offload restreint aux workloads `PURE` |
| H-07 | L'utilisateur accepte un surcoût CPU d'analyse borné (par défaut 2 % d'un cœur) | analyse trop coûteuse | réduction automatique de profondeur puis arrêt du profilage (C-05) |
| H-08 | Les résultats des mods ne dépendent pas de l'adresse mémoire ni de l'ordre de hachage d'identité (`System.identityHashCode`) | déterminisme non préservable | classification `NON_DETERMINISTIC`, rejet de l'offload |

Toute autre hypothèse introduite pendant l'implémentation DOIT être ajoutée à ce tableau via un ADR.

## 0.6 Terminologie

| Terme | Définition normative |
|---|---|
| Workload | unité de travail observable et adressable, identifiée par un `WorkID`, correspondant à un point d'exécution (méthode, handler d'événement, tâche) dans un contexte d'appel donné |
| Capture | capacité du runtime à identifier, isoler et représenter un workload indépendamment de son chemin d'exécution Java d'origine |
| Offload | exécution d'un workload hors du thread propriétaire d'origine, dans le runtime Rust |
| Transformation | changement de stratégie d'exécution d'un workload (batching, parallélisation, offload, compilation) |
| Commit | application ordonnée et validée des effets produits hors du thread autoritatif |
| Thread autoritatif | thread qui détient la vérité sur l'état muté : "Server thread" côté serveur, thread client principal côté client |
| Fenêtre de commit | intervalle du tick pendant lequel le commit engine applique les command buffers |
| Fingerprint | empreinte stable identifiant l'environnement exact dans lequel une décision d'optimisation reste valide |
| Epoch | numéro de version monotone de l'état autoritatif, utilisé pour détecter les snapshots périmés |
| Shadow execution | exécution parallèle non autoritative d'une stratégie candidate, à seule fin de comparaison |
| Promotion | passage d'une stratégie candidate à l'état actif après validation |
| Démotion | retour d'une stratégie active vers une stratégie plus sûre après anomalie |

## 0.7 Portée et non-portée

Dans la portée :

```text
- mod Forge 1.20.1 chargeable (client + serveur dédié)
- bibliothèque native Rust chargée par la JVM
- observation, analyse, décision, exécution, validation, apprentissage
- optimisation générique fondée sur les propriétés des workloads
- SDK permettant d'écrire des systèmes/mods en Rust au-dessus du runtime
```

Hors portée pour la V1.0 :

```text
- remplacement de la JVM
- réécriture de Minecraft
- traduction universelle de bytecode Java arbitraire vers Rust natif
- support Fabric/NeoForge (prévu comme travail ultérieur, abstraction obligatoire dès maintenant)
- support console/mobile
- redistribution de Minecraft, de Forge ou de mods tiers
```

---

# PARTIE 1 : AUDIT DU V0.3 ET REGISTRE DES CORRECTIONS

Cette partie est **normative**. Elle documente ce qui a été conservé, corrigé, requalifié ou supprimé, conformément à la règle de conservation et d'enrichissement.

## 1.1 Résultat global de l'audit

Le V0.3 est une **vision architecturale correcte et ambitieuse**, mais ce n'est pas une spécification exploitable : il décrit des intentions, pas des contrats.

Bilan quantitatif :

| Catégorie | Nombre | Traitement en V1.0 |
|---|---|---|
| Exigences exprimées ou implicites dans le V0.3 | 312 | conservées et tracées (annexe A.4) |
| Exigences conservées telles quelles | 268 | reprises et approfondies |
| Exigences requalifiées (maturité ou périmètre) | 31 | conservées avec niveau `EXPERIMENTAL` ou `FUTURE` |
| Exigences corrigées (techniquement fausses) | 9 | corrigées, justification ci-dessous |
| Exigences supprimées | 4 | justification ci-dessous |
| Composants absents ajoutés | 21 | voir PARTIE 3 |
| Interfaces absentes ajoutées | 17 | voir PARTIE 6 |
| Formats de données absents ajoutés | 12 | voir PARTIE 4 |
| Machines à états absentes ajoutées | 7 | voir PARTIE 7 |
| Tests absents ajoutés | 190+ | voir PARTIE 20 |

## 1.2 Contradictions détectées et résolution

| ID | Contradiction dans le V0.3 | Résolution V1.0 |
|---|---|---|
| C-01 | §1.1 "n'est pas un mod Forge" mais §73 Phase 1 "Forge mod" comme premier livrable | RUSTFORGE-X **est** distribué comme un mod Forge (conteneur de distribution et point d'ancrage) mais **n'est pas** un mod de gameplay. Formulation corrigée en §2.2. Le mod Forge est le véhicule, pas la nature du système |
| C-02 | §18 "World Mirror : représentation Rust des données" vs §11 "éviter les copies" et §24 "minimiser les copies" | Le World Mirror est redéfini comme **miroir partiel, dérivé, à la demande et gouverné par un budget mémoire** (C-20). Aucune copie intégrale du monde. Le mirror n'est peuplé que par des régions explicitement demandées par un workload validé |
| C-03 | §38 "préserver le déterminisme, y compris RNG" vs §8 opportunité A "parallélisation" et §32 stratégies parallèles | Le parallélisme n'est autorisé que si le workload est classé `DETERMINISTIC` **et** `ORDER_INSENSITIVE`, ou si l'ordre observable est restauré au commit. Tout accès à un RNG partagé (`Level.random`, `Entity.random`) rend le workload `NON_DETERMINISTIC` sauf remplacement par un RNG déterministe indexé (C-13, §8.6) |
| C-04 | §29 niveaux 0 à 6 vs §6.6 états de confiance vs §39 modes | Trois notions distinctes étaient mélangées. V1.0 les sépare formellement : **niveau de transformation** (TL0..TL6, propriété d'une stratégie), **état de cycle de vie** (SM-01, propriété d'un workload), **mode runtime** (politique globale, PARTIE 28) |
| C-05 | §46 "client optimisé <-> serveur vanilla" vs §22 "batching des paquets" | Le batching de paquets modifiant le protocole est interdit en mode compatible. Seul le batching **interne** (préparation, sérialisation, compression) est autorisé. Aucun paquet supplémentaire ni modifié n'est émis sans négociation explicite (C-42) |
| C-06 | §34 "l'optimiseur ne doit jamais devenir le bottleneck" vs §6 analyse statique + dynamique complète de tous les mods | L'analyse est désormais **incrémentale, priorisée et budgétée** (C-05, C-44). Un mod n'est jamais analysé intégralement : seules les méthodes atteignant un seuil de chaleur sont analysées en profondeur |
| C-07 | §15 stratégie 4 "compilation native" présentée au même niveau que les autres vs §72 "limite fondamentale" | La compilation native est marquée `EXPERIMENTAL` et strictement limitée au sous-ensemble RF-IR Niveau 2 (PARTIE 10). Elle n'est jamais un prérequis de release |
| C-08 | §13 "préserver la sémantique observable des événements" vs §14 "handlers parallélisables" | Un handler n'est parallélisable que si l'analyse prouve l'absence d'annulation observable, l'absence d'écriture partagée et l'indépendance d'ordre. Sinon `FORCE_SERIAL`. Voir C-06 et §16.4 |
| C-09 | §63 "GPU" listé dans les objectifs alors que §63 dit lui-même de ne pas l'utiliser artificiellement | Le GPU passe en `FUTURE` avec interface définie (C-47) et aucune implémentation en V1.0 |

## 1.3 Exigences techniquement impossibles ou mal posées, et requalification

| ID | Exigence V0.3 | Problème | Correction V1.0 |
|---|---|---|---|
| I-01 | "transformer automatiquement le bytecode Java arbitraire en Rust natif" | indécidable en général (réflexion, chargement dynamique, JNI, `invokedynamic`, exceptions, GC, identité d'objet) | remplacé par un pipeline à sous-ensemble déclaré : RF-IR Niveaux 0/1/2 avec table `SUPPORTED / PARTIALLY_SUPPORTED / UNSUPPORTED` (PARTIE 10) |
| I-02 | "détecter automatiquement les effets de bord de n'importe quelle méthode" | analyse d'alias inter-procédurale sur code inconnu, non résoluble de façon sûre et complète | remplacé par une analyse **conservatrice, sous-approximée pour la sûreté** : tout ce qui n'est pas prouvé pur est impur. `UNKNOWN = CONSERVATIVE` devient une règle formelle (§8.2) |
| I-03 | "1000+ mods" comme cible de fonctionnement | non vérifiable, non représentatif, la JVM et Forge s'écroulent avant | requalifié en objectif de **scalabilité algorithmique** : complexité au plus O(n log n) en nombre de workloads suivis, plafonds explicites, tests jusqu'à 500 mods synthétiques (PARTIE 22) |
| I-04 | "supprimer le GC" (sous-entendu par §23) | impossible depuis un mod | V0.3 le disait déjà partiellement ; V1.0 le formalise : objectif = réduction du **taux d'allocation** sur les chemins optimisés, mesuré par `allocation_rate_bytes_per_tick` |
| I-05 | "shadow execution comparant systématiquement Java et Rust" | doubler l'exécution d'un workload avec effets de bord duplique ces effets | shadow execution autorisée uniquement pour workloads `PURE` ou exécutés sur snapshot isolé, avec command buffer jeté (C-25) |
| I-06 | "Rust-dominant runtime" (Phase 10) | un mod Forge ne peut pas remplacer les sous-systèmes Minecraft sans réimplémenter le jeu | conservé comme **direction**, requalifié `FUTURE` : Phase 9/10 = sous-systèmes Rust-natifs **optionnels et opt-in** exposés via le SDK, pas un remplacement du jeu |
| I-07 | "hot reload / retransformation de classes à chaud" (implicite §24) | `retransformClasses` ne peut pas changer schéma, hiérarchie ni signatures ; coûteux et risqué en cours de partie | autorisé uniquement pour l'ajout/retrait de sondes d'instrumentation sur corps de méthode, hors tick, budget borné (C-04) |
| I-08 | "profiler tous les accès mémoire" (§5) | instrumentation de tous les accès champs = ralentissement d'un ordre de grandeur | remplacé par l'analyse statique des read/write sets + échantillonnage dynamique de vérification (C-11) |
| I-09 | "cache des résultats de calculs coûteux" (§8 opportunité C) sans condition | mise en cache d'une méthode non pure = corruption | conditionné à `PURE` + `DETERMINISTIC` + clé complète capturable (C-32) |

## 1.4 Exigences supprimées et justification

| Exigence V0.3 | Motif de suppression |
|---|---|
| "Speculative execution" des handlers (§14) | contradictoire avec INV-03 (aucun effet non validé n'atteint l'état autoritatif) et sans mécanisme de rollback d'effets Minecraft partiels. Remplacé par shadow execution (C-25) qui apporte le même bénéfice d'apprentissage sans risque |
| Optimisation "suppression de synchronisations inutiles" dans du code tiers (§17) | supprimer un `synchronized` dans du bytecode de mod change la sémantique mémoire du programme observé et ne peut pas être prouvé sûr. Remplacé par la réduction de contention **dans le code RUSTFORGE-X** et par le sharding des structures propres au runtime (C-31) |
| Dashboard listant les mods par nom avec optimisations nommées (§68) tel que rédigé | conservé sur le fond mais reformulé : le dashboard affiche des mesures et des décisions dérivées de propriétés, jamais des règles codées par mod. Aucune logique conditionnelle par nom de mod dans le moteur (INV-12) |
| "compression réseau accélérée remplaçant zlib côté vanilla" (implicite §22) | changerait le format de trame négocié. Remplacé par une accélération de la compression **conforme au format zlib** uniquement, `EXPERIMENTAL`, avec vérification bit à bit |

## 1.5 Trous techniques comblés

| ID | Manque du V0.3 | Comblé par |
|---|---|---|
| G-01 | aucun modèle de données formel du workload | PARTIE 4, DM-01 à DM-14 |
| G-02 | aucun fingerprint défini malgré le cache | DM-05, §4.5 |
| G-03 | aucune ABI Java/Rust | PARTIE 6, IF-01 à IF-06, ADR-004 |
| G-04 | aucun protocole de commit ni de détection de conflit | C-21, C-22, C-23, PARTIE 8 |
| G-05 | aucune définition de la fenêtre de tick ni des points d'ancrage Forge | C-01, §5.1, SM-04 |
| G-06 | aucune gestion d'erreurs typée | annexe A.2, codes `E-xxxx` |
| G-07 | aucune stratégie de test au-delà d'une liste de sujets | PARTIE 20, 190+ tests identifiés |
| G-08 | aucun protocole de benchmark reproductible | PARTIE 21, §21.3 méthodologie statistique |
| G-09 | aucun build, packaging ni release | PARTIES 23, 24, 25 |
| G-10 | aucune configuration ni CLI | PARTIE 28, C-37, C-38 |
| G-11 | aucun critère de "done" | PARTIE 29 |
| G-12 | aucune gestion des mods multithreadés existants | C-17 §5.17.9 (oversubscription, pools imbriqués) |
| G-13 | aucun modèle de coût de transfert | C-15 §5.15.5 et C-45 |
| G-14 | aucune politique de sécurité | PARTIE 19 |
| G-15 | aucune spécification du SDK Rust | C-39, PARTIE 5 |
| G-16 | aucune définition du comportement en cas de panic Rust | C-27 §5.27.6, FM-11 |
| G-17 | aucune gestion des versions de schéma des bases persistées | C-09, C-32, §14.6 |

## 1.6 Registre des risques

Chaque risque possède un propriétaire (composant), une détection et une mitigation **implémentées**, pas seulement décrites.

| ID | Risque | Gravité | Composant | Détection | Mitigation |
|---|---|---|---|---|---|
| RISK-01 | corruption du monde par commit concurrent | critique | C-22 | conflict detection sur epochs + read set | rejet du command buffer, rollback, démotion, `JAVA_ONLY` permanent pour ce `WorkID` |
| RISK-02 | deadlock entre pool RF-X et locks Minecraft | critique | C-17 | watchdog par tâche + détection de lock inversé | interdiction d'acquérir un lock JVM depuis un worker (INV-06), timeout, annulation |
| RISK-03 | panic Rust traversant la frontière FFI | critique | C-27 | `catch_unwind` obligatoire à chaque point d'entrée | conversion en code d'erreur `E-1xxx`, désactivation du sous-système fautif |
| RISK-04 | surcharge du profiler | élevée | C-05 | mesure d'overhead auto-référencée (§12.4) | réduction de profondeur, puis arrêt, puis désactivation persistante |
| RISK-05 | oversubscription CPU face aux mods multithreadés | élevée | C-17 | comptage des threads runnables système + charge | dimensionnement dynamique du pool, `FORCE_SERIAL` sous pression |
| RISK-06 | invalidation de cache manquée après mise à jour d'un mod | élevée | C-32 | fingerprint complet (DM-05) | refus de charger toute entrée dont le fingerprint diffère, purge automatique |
| RISK-07 | régression de performance introduite par le runtime | élevée | C-36 | benchmark de non-régression en CI + auto-mesure runtime | démotion automatique, seuil de régression configurable |
| RISK-08 | déoptimisation JIT provoquée par l'instrumentation | moyenne | C-04 | comparaison de coût avant/après instrumentation | sondes limitées, retransformation retirée si coût > budget |
| RISK-09 | classes générées dynamiquement rendant les fingerprints instables | moyenne | C-41 | taux d'invalidation par mod | passage en mode échantillonnage, exclusion du mod de l'offload |
| RISK-10 | fuite mémoire native | élevée | C-31 | compteurs d'arènes + RSS natif | plafonds durs, purge, désactivation |
| RISK-11 | incompatibilité avec un autre mod de transformation de bytecode | élevée | C-33 | détection des transformateurs concurrents au chargement | reculer l'ordre de transformation, sinon mode observation seul |
| RISK-12 | reflection/`MethodHandle` contournant les sondes | moyenne | C-04 | divergence entre compteurs d'appels et échantillonnage | marquage `UNKNOWN`, refus d'offload |
| RISK-13 | divergence client/serveur causant un kick | élevée | C-24 | comparaison d'état sur workloads réseau | interdiction d'optimiser tout workload touchant la sérialisation de paquets sans validation bit à bit |
| RISK-14 | corruption de sauvegarde | critique | C-22 | invariants de commit + test de round-trip de sauvegarde | aucun écrit vers le format monde depuis un worker ; commit uniquement via API Minecraft sur thread autoritatif |
| RISK-15 | surconsommation mémoire du World Mirror | moyenne | C-20 | budget mémoire mirror | éviction LRU, refus de nouvelles régions |

## 1.7 Traçabilité

L'annexe A.4 contient la table complète `exigence V0.3 -> section V1.0`. Toute exigence V0.3 y apparaît avec l'un des statuts suivants :

```text
KEPT        conservée à l'identique sur le fond
DEEPENED    conservée et spécifiée en profondeur
CORRECTED   corrigée (voir 1.3)
REQUALIFIED changement de niveau de maturité
REMOVED     supprimée (voir 1.4)
```

---

# PARTIE 2 : VISION, OBJECTIFS, PRINCIPES DIRECTEURS

## 2.1 Résumé exécutif

RUSTFORGE-X est un **runtime adaptatif d'exécution et d'optimisation automatique** pour Minecraft Java 1.20.1 + Forge.

L'objectif n'est pas de profiler des mods pour écrire ensuite manuellement une optimisation propre à chacun. L'objectif est de construire un système capable de :

```text
1.  observer automatiquement Minecraft, Forge et les mods chargés
2.  instrumenter méthodes, événements, tâches et dépendances
3.  comprendre quel travail est réellement exécuté
4.  analyser lectures, écritures, allocations, synchronisations, contraintes de thread
5.  déterminer quels workloads peuvent être parallélisés, batchés, mis en cache
    ou déplacés hors du chemin critique
6.  exécuter ces workloads dans un runtime natif Rust lorsque c'est sûr et rentable
7.  comparer en continu l'ancien chemin et le nouveau chemin
8.  conserver la stratégie la plus performante ET correcte
9.  revenir au chemin Java dès qu'une opération ne peut pas être déplacée sans risque
10. appliquer la même philosophie à Minecraft, à Forge et à un nombre arbitraire de mods
```

Le pipeline canonique, invariant du projet :

```text
OBSERVE -> ANALYZE -> CLASSIFY -> DECIDE -> EXTRACT -> OPTIMIZE
        -> VALIDATE -> EXECUTE -> MEASURE -> LEARN
```

Vue d'ensemble :

```text
              MINECRAFT
                  |
              FORGE / JVM
                  |
        +---------v---------+
        |   RUSTFORGE-X     |
        |  OBSERVE / LEARN  |
        |  ANALYSE / DECIDE |
        |  OPTIMISE / EXEC. |
        +---------+---------+
                  |
        +---------v---------+
        |   RUST RUNTIME    |
        |                   |
        | Scheduler         |
        | Task Graph        |
        | Native Execution  |
        | Memory            |
        | World Mirror      |
        | Entity / Chunk    |
        | Networking        |
        | Client Pipeline   |
        +-------------------+
```

## 2.2 Ce que RUSTFORGE-X est et n'est pas

RUSTFORGE-X **n'est pas** :

```text
- un simple profiler
- une liste d'optimisations préprogrammées par mod
- un remplacement de Minecraft
- un système qui déplace aveuglément du code Java sur d'autres threads
- un système exigeant la réécriture des mods
- une promesse de transformer n'importe quel bytecode Java en Rust natif
```

RUSTFORGE-X **est** :

> un moteur d'analyse, de décision et d'exécution capable de transformer dynamiquement le workload Minecraft/Forge/mods en tâches optimisées, parallélisées et exécutées par un runtime Rust lorsque cela est prouvable et rentable, avec validation comportementale, rollback et fallback Java.

Précision apportée par la V1.0 (résolution C-01) : RUSTFORGE-X **est packagé comme un mod Forge** parce que c'est le seul véhicule de chargement légitime dans l'écosystème cible. Il n'ajoute aucun contenu de jeu et ne dépend d'aucun mod tiers.

Le système raisonne sur le **travail**, pas sur les fichiers ni sur les noms de mods.

## 2.3 Architecture cible

Minecraft classique :

```text
Minecraft
   |
   v
Java / JVM
   |
   v
main thread
   |
   +-- Minecraft
   +-- Forge
   +-- Mod A
   +-- Mod B
   +-- ...
```

Architecture RUSTFORGE-X :

```text
Minecraft / Forge / Mods
          |
          v
  RUSTFORGE-X ANALYZER
          |
          +-- Observe
          +-- Trace
          +-- Understand
          +-- Classify
          +-- Decide
                  |
          +-------+--------+
          v                v
     Java path        Rust path
          |                |
          |          Parallel workers
          |                |
          +--------+-------+
                   v
            Deterministic
                commit
                   v
               Minecraft
```

Java reste le **chemin de compatibilité** et l'**état autoritatif**. Rust devient progressivement le **chemin d'exécution privilégié pour les workloads prouvés compatibles**.

Répartition normative des responsabilités :

```text
Java/Forge = compatibilité + état autoritatif + API Minecraft + instrumentation
Rust       = calcul, ordonnancement, offload, optimisation, mesure, apprentissage
```

## 2.4 Objectifs client (R-100)

Optimiser, avec mesure obligatoire : FPS, frametime, 1 % low, 0,1 % low, temps de game thread, préparation de rendu, préparation des chunks, meshing, particules, entités, IA, pathfinding, animations, synchronisation, allocations, chargements, resource reload, traitement réseau, calculs de mods, pression d'allocation (donc GC indirectement).

## 2.5 Objectifs serveur (R-101)

Optimiser : TPS, MSPT, tick serveur, génération de chunks, chargement/déchargement, lighting, pathfinding, IA, entités, block entities, redstone, automatisation, networking, sauvegardes, sérialisation, allocations, contention, files de tâches, utilisation CPU.

## 2.6 Objectif transversal (R-102)

Exploiter au maximum : tous les cœurs disponibles, les caches CPU, SIMD, la mémoire locale, le parallélisme, le batching, le work stealing, les task graphs, l'exécution data-oriented, la réutilisation mémoire et le traitement par lots, **sans jamais dégrader la correction ni la compatibilité**.

## 2.7 Principes directeurs

| ID | Principe | Conséquence architecturale |
|---|---|---|
| P-01 | Correction avant performance | toute stratégie non validée est inactive |
| P-02 | `UNKNOWN = CONSERVATIVE` | l'absence de preuve vaut preuve d'impossibilité |
| P-03 | Mod-agnostisme | aucune branche conditionnelle sur un nom de mod dans le moteur (INV-12) |
| P-04 | Explicabilité totale | toute décision produit un enregistrement causal lisible (C-35) |
| P-05 | Réversibilité | toute transformation est démotable en un tick |
| P-06 | Budget avant ambition | analyse, mémoire et threads sont bornés par configuration |
| P-07 | Mesure avant croyance | aucune optimisation activée sans mesure reproductible |
| P-08 | Le thread autoritatif est sacré | on ne bloque jamais le thread autoritatif sur un worker |
| P-09 | Le jeu survit au runtime | toute panne RF-X dégrade vers Java, jamais vers un crash |
| P-10 | Pas de fiction | pas de `TODO` ni de faux résultat dans une fonctionnalité déclarée terminée |
| P-11 | Abstraction de la plateforme de mod | Forge est isolé derrière `C-01`, pour permettre d'autres loaders plus tard |
| P-12 | Progressivité | chaque jalon produit un JAR fonctionnel et installable |

## 2.8 Limite fondamentale (conservée du V0.3 §72, renforcée)

RUSTFORGE-X **ne doit jamais prétendre** que toute méthode Java peut être automatiquement transformée en Rust.

```text
Java-only     : la majorité du code au départ
Java + Rust   : workloads offloadables
Rust offload  : calcul pur ou sur snapshot
Rust-native   : sous-systèmes réécrits via le SDK (opt-in)
```

L'objectif est de maximiser automatiquement les deux dernières catégories **sans sacrifier la compatibilité**.

## 2.9 Règle d'or

```text
              NE PAS DEMANDER :
       "Comment optimiser ce mod ?"

              DEMANDER :
 "Comment faire comprendre automatiquement
    son workload au runtime et determiner
       la meilleure maniere de l'executer ?"
```

---

# PARTIE 3 : ARCHITECTURE GLOBALE ET INVENTAIRE DES COMPOSANTS

## 3.1 Vue en couches

```text
+---------------------------------------------------------------+
| L5  OUTILS ET SURFACES                                        |
|     CLI/commandes (C-38)  Dashboard (C-46)  Bench (C-36)      |
+---------------------------------------------------------------+
| L4  GOUVERNANCE                                               |
|     Decision (C-15) Risk (C-14) Compat (C-33) Learning (C-44) |
|     Config (C-37)   Validation (C-24) Rollback (C-26)         |
+---------------------------------------------------------------+
| L3  ANALYSE                                                   |
|     Profiler (C-05) EventObs (C-06) CallGraph (C-07)          |
|     WorkloadProfiler (C-08) Dep (C-10) DataFlow (C-11)        |
|     Thread (C-12) Determinism (C-13) ModDiscovery (C-41)      |
+---------------------------------------------------------------+
| L2  EXECUTION                                                 |
|     TaskGraph (C-16) Scheduler (C-17) Workers (C-18)          |
|     Snapshot (C-19) WorldMirror (C-20) CmdBuffer (C-21)       |
|     Commit (C-22) Conflict (C-23) Shadow (C-25)               |
|     Domain engines (C-49..C-53)                               |
+---------------------------------------------------------------+
| L1  RUNTIME NATIF                                             |
|     RustRuntime (C-27) Memory (C-31) Cache (C-32)             |
|     RF-IR (C-28) Optimizer (C-29) Backend (C-30)              |
|     Telemetry (C-34) Diagnostics (C-35) SDK (C-39)            |
+---------------------------------------------------------------+
| L0  ANCRAGE JVM                                               |
|     ForgeIntegration (C-01) Bootstrap (C-02)                  |
|     NativeLoader (C-03) Instrumentation (C-04)                |
|     HardwareProbe (C-45) WorkloadDB (C-09) Release (C-40)     |
+---------------------------------------------------------------+
```

## 3.2 Inventaire normatif des composants

| ID | Composant | Langage | Couche | Maturité V1.0 | Jalon |
|---|---|---|---|---|---|
| C-01 | Forge Integration | Java | L0 | STABLE | M0 |
| C-02 | Bootstrap | Java | L0 | STABLE | M0 |
| C-03 | Native Loader | Java + Rust | L0 | STABLE | M0 |
| C-04 | JVM Instrumentation | Java (ASM) | L0 | STABLE | M1 |
| C-05 | Profiler | Java + Rust | L3 | STABLE | M1 |
| C-06 | Event Observer | Java | L3 | STABLE | M1 |
| C-07 | Call Graph | Rust | L3 | STABLE | M2 |
| C-08 | Workload Profiler | Rust | L3 | STABLE | M2 |
| C-09 | Workload Database | Rust | L0 | STABLE | M2 |
| C-10 | Dependency Analyzer | Rust | L3 | STABLE | M2 |
| C-11 | Data Flow Analyzer | Rust | L3 | STABLE | M3 |
| C-12 | Thread Analyzer | Rust | L3 | STABLE | M2 |
| C-13 | Determinism Analyzer | Rust | L3 | STABLE | M3 |
| C-14 | Risk Engine | Rust | L4 | STABLE | M3 |
| C-15 | Decision Engine | Rust | L4 | STABLE | M3 |
| C-16 | Task Graph | Rust | L2 | STABLE | M2 |
| C-17 | Scheduler | Rust | L2 | STABLE | M2 |
| C-18 | Worker Runtime | Rust | L2 | STABLE | M2 |
| C-19 | Snapshot System | Java + Rust | L2 | STABLE | M4 |
| C-20 | World Mirror | Rust | L2 | EXPERIMENTAL | M5 |
| C-21 | Command Buffer | Rust | L2 | STABLE | M4 |
| C-22 | Commit Engine | Java + Rust | L2 | STABLE | M4 |
| C-23 | Conflict Detection | Rust | L2 | STABLE | M4 |
| C-24 | Validation Engine | Rust | L4 | STABLE | M4 |
| C-25 | Shadow Execution | Rust | L2 | STABLE | M4 |
| C-26 | Rollback | Rust | L4 | STABLE | M4 |
| C-27 | Rust Runtime core | Rust | L1 | STABLE | M0 |
| C-28 | RF-IR | Rust | L1 | EXPERIMENTAL | M6 |
| C-29 | IR Optimizer | Rust | L1 | EXPERIMENTAL | M6 |
| C-30 | Compiler Backend | Rust | L1 | EXPERIMENTAL | M7 |
| C-31 | Memory Manager | Rust | L1 | STABLE | M1 |
| C-32 | Cache | Rust | L1 | STABLE | M3 |
| C-33 | Compatibility Engine | Rust | L4 | STABLE | M3 |
| C-34 | Telemetry | Rust | L1 | STABLE | M1 |
| C-35 | Diagnostics | Rust + Java | L1 | STABLE | M1 |
| C-36 | Benchmark Harness | Rust + Java + scripts | L5 | STABLE | M1 |
| C-37 | Configuration | Rust + Java | L4 | STABLE | M0 |
| C-38 | CLI / commandes | Java + Rust | L5 | STABLE | M1 |
| C-39 | Rust Mod SDK | Rust | L1 | EXPERIMENTAL | M8 |
| C-40 | Release System | scripts + CI | L0 | STABLE | M1 |
| C-41 | Mod Discovery | Java | L3 | STABLE | M1 |
| C-42 | Network Subsystem | Rust | L2 | EXPERIMENTAL | M7 |
| C-43 | Explainability Engine | Rust | L4 | STABLE | M3 |
| C-44 | Learning / Auto-tuner | Rust | L4 | STABLE | M5 |
| C-45 | Hardware Probe | Rust | L0 | STABLE | M0 |
| C-46 | Dashboard UI | Java | L5 | STABLE | M5 |
| C-47 | GPU Offload | Rust | L1 | FUTURE | - |
| C-48 | Persistence / Diagnostics store | Rust | L1 | STABLE | M2 |
| C-49 | Entity Engine | Rust | L2 | EXPERIMENTAL | M5 |
| C-50 | Chunk Engine | Rust | L2 | EXPERIMENTAL | M6 |
| C-51 | Pathfinding Engine | Rust | L2 | EXPERIMENTAL | M6 |
| C-52 | Lighting Engine | Rust | L2 | FUTURE | - |
| C-53 | Client Render Prep | Rust | L2 | EXPERIMENTAL | M7 |

## 3.3 Graphe de dépendances entre composants

```text
C-01 Forge Integration
 +-> C-02 Bootstrap
      +-> C-03 Native Loader ---------> C-27 Rust Runtime
      +-> C-37 Configuration            |
      +-> C-04 Instrumentation          +-> C-31 Memory
      +-> C-41 Mod Discovery            +-> C-34 Telemetry
      +-> C-06 Event Observer           +-> C-45 Hardware Probe
                                        +-> C-17 Scheduler -> C-18 Workers
C-04/C-06 --(events)--> C-05 Profiler --> C-08 Workload Profiler
                                            |
                    +-----------------------+
                    v
        C-07 CallGraph   C-10 Dep   C-11 DataFlow   C-12 Thread   C-13 Determinism
                    \        \          |            /             /
                     +--------+---------+-----------+-------------+
                                        v
                                  C-14 Risk Engine
                                        v
                                  C-15 Decision Engine <---- C-33 Compatibility
                                        |                    C-32 Cache
                                        |                    C-44 Learning
                                        v
                                  C-16 Task Graph
                                        v
                                  C-17 Scheduler
                                        v
                                  C-18 Workers
                             +----------+-----------+
                             v                      v
                     C-19 Snapshot / C-20 Mirror   C-25 Shadow
                             v                      v
                       C-21 Command Buffer     C-24 Validation
                             v                      v
                       C-23 Conflict           C-26 Rollback
                             v
                       C-22 Commit Engine  -> état Minecraft
                             v
                       C-34 Telemetry -> C-44 Learning -> C-09 Workload DB
                             v
                       C-43 Explainability -> C-35 Diagnostics -> C-38 CLI / C-46 UI
```

Règle de couplage (INV-13) : aucun composant de couche N ne DOIT dépendre d'un composant de couche supérieure, sauf par injection d'interface définie en PARTIE 6.

## 3.4 Modèle de threads

```text
+----------------------------------------------------------+
| THREAD AUTORITATIF (Server thread | Client main thread)   |
|  - exécute Minecraft, Forge, mods                         |
|  - exécute les hooks RF-X (submit, drain, commit)         |
|  - SEUL autorisé à muter l'état Minecraft                 |
+----------------------------------------------------------+
        | submit (lock-free MPSC)         ^ commit
        v                                 |
+----------------------------------------------------------+
| RF-X WORKER POOL (Rust, N = f(cores, budget))             |
|  - workers de calcul, work stealing                       |
|  - NE DOIT PAS appeler l'API Minecraft mutante            |
|  - NE DOIT PAS acquérir de lock JVM                       |
|  - produit des command buffers                            |
+----------------------------------------------------------+
+----------------------------------------------------------+
| RF-X ANALYSIS THREAD (1, priorité basse)                  |
|  - analyse statique, décision, apprentissage, persistance |
|  - jamais dans le chemin critique du tick                 |
+----------------------------------------------------------+
+----------------------------------------------------------+
| RF-X WATCHDOG THREAD (1)                                  |
|  - détecte dépassements, deadlocks, overhead              |
|  - peut déclencher démotion et arrêt d'urgence            |
+----------------------------------------------------------+
+----------------------------------------------------------+
| THREADS TIERS (mods multithreadés, Netty, chunk workers)  |
|  - observés, jamais pilotés                               |
|  - comptés dans le budget CPU global                      |
+----------------------------------------------------------+
```

## 3.5 Cycle de vie d'un tick serveur instrumenté

```text
TICK_BEGIN
  |  C-01 ouvre la fenêtre de tick, incrémente l'epoch
  |  C-05 arme les compteurs
  v
PHASE_PRE            (Forge TickEvent.ServerTickEvent PRE)
  |  C-15 publie les décisions applicables à ce tick
  |  C-19 prépare les snapshots des workloads planifiés
  |  C-17 soumet les tâches prêtes
  v
PHASE_VANILLA        (tick Minecraft + mods, code Java)
  |  sondes C-04 actives, workers Rust en parallèle
  v
PHASE_DRAIN          (fin de tick, avant POST)
  |  C-17 attend les tâches à deadline "ce tick" (avec timeout)
  |  C-21 remet les command buffers
  |  C-23 vérifie les conflits (read set vs epoch)
  |  C-22 applique les commandes valides sur le thread autoritatif
  v
PHASE_POST           (Forge TickEvent.ServerTickEvent POST)
  |  C-34 collecte les métriques du tick
  |  C-24 traite les résultats de shadow execution
  |  C-26 applique les démotions décidées
  v
TICK_END
  |  budget restant transmis à l'analyse (C-44) hors chemin critique
```

Côté client, la structure est identique avec `TickEvent.ClientTickEvent` et une fenêtre supplémentaire alignée sur la préparation de frame (voir PARTIE 15).

## 3.6 Arborescence du dépôt

```text
rustforge-x/
├── settings.gradle
├── build.gradle
├── gradle.properties
├── Cargo.toml                       # workspace Rust
├── rust-toolchain.toml
├── LICENSE
├── NOTICE
├── CHANGELOG.md
├── README.md
├── ARCHITECTURE.md
├── BUILDING.md
├── INSTALLATION.md
├── CONFIGURATION.md
├── COMPATIBILITY.md
├── TROUBLESHOOTING.md
├── BENCHMARKS.md
├── RELEASING.md
├── SECURITY.md
├── RUST_MOD_SDK.md
├── docs/
│   ├── AGENT.md
│   ├── decisions/                   # ADR-001.md ...
│   ├── spec/                        # copie versionnée de ce cahier des charges
│   └── diagrams/
├── crates/
│   ├── rfx-core/                    # C-27 runtime, erreurs, types communs
│   ├── rfx-ffi/                     # C-27 points d'entrée JNI, ABI
│   ├── rfx-memory/                  # C-31
│   ├── rfx-telemetry/               # C-34
│   ├── rfx-scheduler/               # C-17, C-18
│   ├── rfx-taskgraph/               # C-16
│   ├── rfx-model/                   # DM-* structures de données + sérialisation
│   ├── rfx-profiler/                # C-05 partie native
│   ├── rfx-analyzer/                # C-07, C-08, C-10, C-11, C-12, C-13
│   ├── rfx-bytecode/                # lecture/parse bytecode côté Rust
│   ├── rfx-decision/                # C-14, C-15, C-33, C-43
│   ├── rfx-exec/                    # C-19, C-21, C-22, C-23, C-25
│   ├── rfx-validate/                # C-24, C-26
│   ├── rfx-cache/                   # C-32, C-09, C-48
│   ├── rfx-learn/                   # C-44
│   ├── rfx-ir/                      # C-28
│   ├── rfx-opt/                     # C-29
│   ├── rfx-backend/                 # C-30 (feature "jit")
│   ├── rfx-world/                   # C-20
│   ├── rfx-entity/                  # C-49
│   ├── rfx-chunk/                   # C-50
│   ├── rfx-pathfinding/             # C-51
│   ├── rfx-lighting/                # C-52 (FUTURE, squelette + tests de contrat)
│   ├── rfx-net/                     # C-42
│   ├── rfx-client/                  # C-53
│   ├── rfx-sdk/                     # C-39
│   ├── rfx-bench/                   # C-36 partie native (criterion)
│   └── rfx-cli/                     # C-38 binaire hors-jeu (analyse de dumps)
├── java/
│   ├── src/main/java/dev/rustforgex/
│   │   ├── bootstrap/               # C-02, C-03
│   │   ├── forge/                   # C-01, C-06, C-41
│   │   ├── instrument/              # C-04
│   │   ├── bridge/                  # IF-01..IF-06 côté Java
│   │   ├── snapshot/                # C-19 côté Java
│   │   ├── commit/                  # C-22 côté Java
│   │   ├── config/                  # C-37
│   │   ├── command/                 # C-38
│   │   ├── ui/                      # C-46
│   │   └── diag/                    # C-35
│   └── src/test/java/...
├── tools/
│   ├── bench/                       # harnais de lancement, parsing de logs
│   ├── modpacks/                    # manifestes de profils de test (sans mods)
│   └── ci/
├── tests/
│   ├── integration/
│   ├── gameplay/
│   └── fixtures/
└── benchmarks/
    └── results/                     # résultats versionnés, format JSON
```

## 3.7 Contenu de l'artefact final

```text
rustforgex-<version>.jar
├── META-INF/
│   ├── MANIFEST.MF
│   └── mods.toml                    # métadonnées Forge
├── dev/rustforgex/**/*.class
├── rustforgex.mixins.json           # si mixins utilisés
├── pack.mcmeta
└── natives/
    ├── windows-x86_64/rfx_native.dll
    ├── linux-x86_64/librfx_native.so
    └── linux-aarch64/librfx_native.so     (best effort)
```

Aucun asset tiers, aucun mod tiers, aucun binaire Minecraft n'est inclus.

---

# PARTIE 4 : MODÈLE DE DONNÉES CANONIQUE

Toutes les structures de cette partie sont définies une seule fois, dans le crate `rfx-model`, et projetées vers Java par génération de code (`tools/ci/gen-java-model`). Toute divergence entre les deux côtés est une erreur de build (test T-004).

## 4.1 DM-01 : WorkID

Identifiant stable d'une unité de travail.

```rust
/// DM-01. Stable sur la durée de vie d'une installation donnée
/// (mêmes mods, même version MC/Forge, même bytecode).
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct WorkId(pub u64);
```

Calcul (normatif, algorithme `WORKID_V1`) :

```text
WorkId = xxh3_64( concat(
    owner_id,              // identifiant du mod propriétaire ou "minecraft"/"forge"/"unknown"
    class_internal_name,   // ex: net/minecraft/world/entity/Mob
    method_name,
    method_descriptor,     // ex: (Lnet/minecraft/world/level/Level;)V
    call_context_hash,     // DM-02, 0 si contexte non discriminant
    side                   // CLIENT | SERVER | COMMON
) )
```

Contraintes :

- R-200 : `WorkId` DOIT être reproductible entre deux lancements identiques.
- R-201 : `WorkId` NE DOIT PAS dépendre d'une adresse mémoire, d'un `identityHashCode`, ni d'un ordre de chargement.
- R-202 : collision détectée (deux descripteurs différents pour un même `WorkId`) => les deux workloads sont marqués `UNKNOWN` et exclus de l'offload ; incident journalisé `E-2101`.

## 4.2 DM-02 : CallContext

Un même code peut avoir des profils radicalement différents selon son appelant. Le contexte d'appel est donc une dimension de première classe, bornée.

```rust
pub struct CallContext {
    /// k derniers frames significatifs, k = config.profiler.context_depth (défaut 3)
    pub frames: SmallVec<[FrameRef; 4]>,
    pub phase: TickPhase,        // PRE, VANILLA, DRAIN, POST, OFF_TICK
    pub thread_role: ThreadRole, // AUTHORITATIVE, RFX_WORKER, THIRD_PARTY, NETTY, UNKNOWN
}
```

- R-203 : la profondeur de contexte DOIT être bornée (défaut 3, maximum 8). Au-delà, les contextes sont fusionnés dans un contexte `TRUNCATED`.
- R-204 : le contexte est capturé par les sondes d'entrée (C-04), pas par `Thread.currentThread().getStackTrace()` dans le chemin chaud (coût prohibitif). Une pile de contexte thread-local maintenue par les sondes est utilisée.

## 4.3 DM-03 : WorkloadDescriptor

Structure centrale du système. C'est ce que produisent les analyseurs et ce que consomme le moteur de décision.

```rust
pub struct WorkloadDescriptor {
    // identité
    pub id: WorkId,
    pub owner: OwnerId,               // mod id, "minecraft", "forge", "unknown"
    pub target: MethodRef,            // classe, nom, descripteur, accès
    pub context: CallContext,         // DM-02
    pub kind: WorkloadKind,           // METHOD | EVENT_HANDLER | TASK | TICKABLE | PACKET_HANDLER
    pub side: Side,                   // CLIENT | SERVER | COMMON
    pub fingerprint: Fingerprint,     // DM-05

    // observation dynamique (DM-04)
    pub dynamics: WorkloadDynamics,

    // analyse
    pub reads: AccessSet,             // DM-06
    pub writes: AccessSet,            // DM-06
    pub side_effects: SideEffectSet,  // DM-07
    pub deps: DependencySet,          // DM-08
    pub thread: ThreadFacts,          // DM-09
    pub determinism: DeterminismFacts,// DM-10
    pub flags: ClassificationFlags,   // DM-11
    pub confidence: Confidence,       // DM-12

    // gouvernance
    pub lifecycle: LifecycleState,    // SM-01
    pub strategy: StrategyRef,        // DM-13, stratégie active
    pub history: HistoryRef,          // index dans C-09
}
```

- R-205 : un `WorkloadDescriptor` DOIT être sérialisable, versionné (`schema_version`), et rechargeable depuis le cache disque.
- R-206 : toute lecture d'un descripteur dont un champ d'analyse est `UNKNOWN` DOIT être traitée par le moteur de décision comme le pire cas possible pour ce champ (P-02).

## 4.4 DM-04 : WorkloadDynamics

Mesures dynamiques, agrégées en fenêtres glissantes, jamais en moyennes non bornées.

```rust
pub struct WorkloadDynamics {
    pub calls_per_tick: Ewma,          // moyenne mobile exponentielle
    pub cpu_ns: Histogram,             // HDR-like, buckets log2, p50/p95/p99/max
    pub wall_ns: Histogram,
    pub alloc_bytes: Ewma,
    pub self_ns_share: f32,            // part du temps propre / temps total du tick
    pub last_seen_tick: u64,
    pub total_calls: u64,
    pub heat: Heat,                    // COLD | WARM | HOT | CRITICAL
    pub variance_class: VarianceClass, // STABLE | NOISY | BIMODAL
    pub observation_quality: f32,      // 0..1, dépend du nombre d'échantillons
}
```

Classification de chaleur (normative, seuils configurables) :

| Heat | Condition par défaut |
|---|---|
| `COLD` | `cpu_ns.p50 * calls_per_tick < 50_000 ns` |
| `WARM` | entre 50 µs et 250 µs par tick |
| `HOT` | entre 250 µs et 1 ms par tick |
| `CRITICAL` | > 1 ms par tick, ou > 5 % du budget de tick |

- R-207 : les histogrammes DOIVENT être bornés en mémoire (buckets fixes) et sans allocation dans le chemin chaud.
- R-208 : `variance_class = BIMODAL` DOIT interdire toute décision fondée sur la moyenne seule ; la décision utilise alors p95.

## 4.5 DM-05 : Fingerprint

Le fingerprint définit l'environnement dans lequel une décision reste valide. C'est la clé de tous les caches.

```rust
pub struct Fingerprint {
    pub schema_version: u16,      // version du modèle de données RF-X
    pub rfx_version: [u8; 3],     // version du runtime
    pub mc_version: u64,          // hash de la version Minecraft
    pub forge_version: u64,
    pub mod_set_hash: u64,        // hash trié de (modid, version, jar sha256 tronqué)
    pub owner_mod_hash: u64,      // hash du jar du mod propriétaire
    pub bytecode_hash: u64,       // hash du bytecode de la méthode APRÈS transformations
    pub deps_bytecode_hash: u64,  // hash combiné des méthodes appelées analysées
    pub jvm_hash: u64,            // vendor + version + flags GC pertinents
    pub hw_class_hash: u64,       // DM-14, classe de matériel, pas identité machine
    pub config_hash: u64,         // options RF-X influençant la décision
}
```

Règles d'invalidation :

| Changement | Effet |
|---|---|
| `schema_version`, `rfx_version` majeur | purge complète des caches de décision |
| `mc_version`, `forge_version` | purge complète |
| `mod_set_hash` | invalide les décisions dépendant d'interactions inter-mods (toutes les décisions de niveau TL3+) |
| `owner_mod_hash` ou `bytecode_hash` | invalide toutes les décisions du `WorkId` concerné |
| `deps_bytecode_hash` | invalide les décisions de niveau TL2+ du `WorkId` |
| `jvm_hash` | invalide les mesures de coût, conserve les faits d'analyse statique |
| `hw_class_hash` | invalide les mesures et les paramètres de parallélisme, conserve la sûreté |
| `config_hash` | invalide les décisions, conserve mesures et analyse |

- R-209 : `bytecode_hash` DOIT être calculé après application de toutes les transformations (Mixin, coremods tiers, RF-X), c'est-à-dire dans le dernier transformateur de la chaîne.
- R-210 : une entrée de cache dont le fingerprint ne correspond pas exactement NE DOIT PAS être utilisée, même partiellement.

## 4.6 DM-06 : AccessSet et domaines mémoire

L'analyse ne raisonne pas sur des adresses mais sur des **domaines abstraits**, ce qui rend la détection de conflits décidable.

```rust
pub struct AccessSet {
    pub domains: BitSet<Domain>,          // domaines touchés
    pub regions: SmallVec<[Region; 8]>,   // granularité fine quand connue
    pub precision: Precision,             // EXACT | OVER_APPROX | UNKNOWN
}

pub enum Domain {
    WorldBlocks, WorldFluids, WorldLighting, ChunkData, ChunkStatus,
    EntityState, EntityInventory, BlockEntityState,
    LevelGlobals, Registries, Capabilities,
    ModStatics, JavaStatics, ThreadLocals,
    Network, FileSystem, NativeExternal, Random, Time, Unknown,
}

pub enum Region {
    Chunk { dim: DimId, cx: i32, cz: i32 },
    Block { dim: DimId, pos: BlockPos },
    Entity { dim: DimId, entity: EntityKey },
    Field  { class: ClassId, field: FieldId },
    Whole(Domain),
}
```

- R-211 : `precision = UNKNOWN` DOIT être traité comme `Whole(Unknown)` pour la détection de conflits, c'est-à-dire en conflit avec tout.
- R-212 : `OVER_APPROX` est acceptable pour la sûreté (surensemble des accès réels) ; une sous-approximation est **interdite** pour les écritures.
- R-213 : le domaine `Random` DOIT être marqué en lecture ET en écriture pour tout accès à un générateur pseudo-aléatoire partagé.

## 4.7 DM-07 : SideEffectSet

```rust
pub struct SideEffectSet {
    pub mutates_world: bool,
    pub spawns_or_removes_entity: bool,
    pub sends_packets: bool,
    pub schedules_tasks: bool,
    pub fires_events: bool,
    pub io: bool,
    pub native_call: bool,
    pub throws_observable: bool,   // exception qui remonte hors du workload
    pub cancels_event: bool,
    pub unknown: bool,             // au moins un appel non analysable
}
```

- R-214 : si `unknown == true`, le workload NE PEUT PAS dépasser le niveau de transformation TL1.

## 4.8 DM-08 : DependencySet

```rust
pub struct DependencySet {
    pub calls: Vec<MethodRef>,          // appels sortants résolus
    pub unresolved_calls: u32,          // appels virtuels non résolus, reflection, indy
    pub data_deps: Vec<WorkId>,         // workloads produisant des données consommées
    pub order_deps: Vec<WorkId>,        // workloads dont l'ordre relatif est observable
    pub external_locks: Vec<LockRef>,   // moniteurs/verrous rencontrés
    pub depth: u16,                     // profondeur d'analyse atteinte
    pub truncated: bool,                // analyse arrêtée par budget
}
```

- R-215 : `truncated == true` ou `unresolved_calls > 0` DOIT positionner `flags.UNKNOWN` (P-02).
- R-216 : les dépendances inconnues NE DOIVENT JAMAIS être considérées comme indépendantes.

## 4.9 DM-09 : ThreadFacts

```rust
pub struct ThreadFacts {
    pub observed_threads: SmallVec<[ThreadRole; 4]>,
    pub main_thread_required: Tri,     // YES | NO | UNKNOWN
    pub uses_synchronized: bool,
    pub uses_locks: bool,
    pub uses_thread_locals: bool,
    pub touches_non_threadsafe_api: bool,
    pub reentrant: Tri,
}
```

Détermination de `main_thread_required` :

```text
YES si :
   - le workload appelle une API connue comme confinée (liste dérivée de propriétés,
     pas de noms de mods : classes du package net.minecraft.* marquées non thread-safe
     par l'analyse de leurs champs mutables non synchronisés)
   - OU il écrit dans un domaine autoritatif (WorldBlocks, EntityState, ...)
   - OU il a été observé exclusivement sur le thread autoritatif ET écrit quelque chose
NO si :
   - PURE ou lectures uniquement sur snapshot fourni
UNKNOWN sinon  -> traité comme YES pour toute décision (P-02)
```

## 4.10 DM-10 : DeterminismFacts

```rust
pub struct DeterminismFacts {
    pub deterministic: Tri,
    pub sources_of_nondeterminism: BitSet<NonDetSource>,
    pub order_sensitive: Tri,
    pub float_sensitive: bool,        // arithmétique flottante non associative
    pub verified_runs: u32,           // exécutions comparées identiques
    pub mismatches: u32,
}

pub enum NonDetSource {
    SharedRandom, SystemTime, IdentityHash, HashMapIteration,
    ThreadScheduling, ExternalIo, NativeCall, Reflection, Unknown,
}
```

- R-217 : `float_sensitive` DOIT être vrai dès qu'une réduction flottante est parallélisée ; dans ce cas, la parallélisation DOIT préserver l'ordre de réduction (réduction déterministe par arbre fixe).

## 4.11 DM-11 : ClassificationFlags

```rust
bitflags! {
    pub struct ClassificationFlags: u32 {
        const READ_ONLY            = 1 << 0;
        const PURE                 = 1 << 1;
        const CPU_INTENSIVE        = 1 << 2;
        const MEMORY_INTENSIVE     = 1 << 3;
        const PARALLELIZABLE       = 1 << 4;
        const BATCHABLE            = 1 << 5;
        const DETERMINISTIC        = 1 << 6;
        const THREAD_SAFE          = 1 << 7;
        const MAIN_THREAD_REQUIRED = 1 << 8;
        const ORDER_SENSITIVE      = 1 << 9;
        const CACHEABLE            = 1 << 10;
        const RUST_CANDIDATE       = 1 << 11;
        const UNSAFE               = 1 << 12;
        const UNKNOWN              = 1 << 13;
        const CLIENT_ONLY          = 1 << 14;
        const SERVER_ONLY          = 1 << 15;
        const STATEFUL             = 1 << 16;
        const SIDE_EFFECT          = 1 << 17;
        const WRITE                = 1 << 18;
        const HOT                  = 1 << 19;
        const IR_ELIGIBLE          = 1 << 20;
    }
}
```

Règles de cohérence vérifiées par test unitaire T-011 :

```text
PURE                => READ_ONLY && DETERMINISTIC && !SIDE_EFFECT
MAIN_THREAD_REQUIRED => !PARALLELIZABLE
UNKNOWN             => !RUST_CANDIDATE && !PARALLELIZABLE && !CACHEABLE && !IR_ELIGIBLE
CACHEABLE           => PURE
IR_ELIGIBLE         => PURE && !UNKNOWN
ORDER_SENSITIVE     => !PARALLELIZABLE (sauf ordre restauré au commit)
```

## 4.12 DM-12 : Confidence

```rust
pub struct Confidence {
    pub static_analysis: f32,   // 0..1
    pub dynamic_evidence: f32,  // 0..1, croît avec les observations
    pub validation: f32,        // 0..1, croît avec les shadow runs réussis
    pub combined: f32,          // min pondéré, jamais une moyenne optimiste
}
```

- R-218 : `combined = min(static_analysis, max(dynamic_evidence, validation))`. La confiance globale ne peut jamais dépasser la confiance statique : une preuve d'exécution ne remplace pas une preuve de structure.

## 4.13 DM-13 : Strategy

```rust
pub struct Strategy {
    pub kind: StrategyKind,
    pub level: TransformationLevel, // TL0..TL6
    pub params: StrategyParams,     // batch_size, worker_count, chunk_size, ...
    pub decided_at_tick: u64,
    pub decision_id: DecisionId,    // trace vers l'explication (C-43)
    pub expected_gain_ns: i64,
    pub measured_gain_ns: Option<i64>,
}

pub enum StrategyKind {
    JavaOnly, JavaBatch, JavaParallel,
    RustOffload, RustBatch, RustParallel,
    RustTransformed, RustNative,
    Reject,
}
```

Correspondance stratégie / niveau de transformation :

| StrategyKind | TL | Prérequis minimum |
|---|---|---|
| `JavaOnly` | TL0 | aucun |
| `JavaBatch` | TL2 | `BATCHABLE`, ordre préservé |
| `JavaParallel` | TL3 | `THREAD_SAFE`, `PARALLELIZABLE`, pas d'écriture partagée |
| `RustOffload` | TL4 | `PURE` ou snapshot complet, coût de transfert amorti |
| `RustBatch` | TL4 | idem + `BATCHABLE` |
| `RustParallel` | TL4 | idem + `PARALLELIZABLE` |
| `RustTransformed` | TL5 | `IR_ELIGIBLE`, IR vérifié, validation shadow réussie |
| `RustNative` | TL6 | implémentation native fournie par le SDK ou par RF-X, validée |
| `Reject` | - | motif obligatoire (DM-15) |

## 4.14 DM-14 : HardwareClass

```rust
pub struct HardwareClass {
    pub physical_cores: u16,
    pub logical_cores: u16,
    pub core_kinds: CoreTopology,   // homogène | P/E cores, avec comptes
    pub l3_bytes: u64,
    pub numa_nodes: u8,
    pub simd: SimdCaps,             // sse2, avx2, avx512, neon
    pub mem_total_bytes: u64,
    pub jni_call_ns: u32,           // mesuré au démarrage (C-45)
    pub ffi_batch_ns_per_kb: u32,   // mesuré
}
```

`hw_class_hash` DOIT hacher une **classe** (paliers de cœurs, présence AVX2, palier de RAM), pas une identité machine, pour que les caches restent partageables entre machines similaires sans identifier l'utilisateur.

## 4.15 DM-15 : RejectReason

```rust
pub enum RejectReason {
    UnknownSideEffect, UnsafeSharedState, OrderDependency, ThreadAffinity,
    NonDeterministicBehavior, HighTransferCost, HighSynchronizationCost,
    UnresolvedJniDependency, UnmodeledNativeCall, InsufficientConfidence,
    BudgetExceeded, CompatibilityRule, ValidationFailed, RegressionDetected,
    HardwareUnsuitable, CacheInvalidated, UserDisabled,
}
```

- R-219 : tout rejet DOIT porter un `RejectReason` et une explication structurée exploitable par C-43. Un rejet sans motif est une erreur `E-2003`.

## 4.16 DM-16 : Command (contenu des command buffers)

```rust
#[repr(C)]
pub enum Command {
    SetBlock { dim: DimId, pos: BlockPos, state: BlockStateId, flags: u8 },
    UpdateEntity { entity: EntityKey, patch: EntityPatch },
    SpawnEntity { dim: DimId, proto: EntityProtoId, pos: Vec3d, nbt: Option<BlobRef> },
    RemoveEntity { entity: EntityKey, reason: u8 },
    SendPacket { target: PacketTarget, payload: BlobRef },
    ScheduleTick { dim: DimId, pos: BlockPos, delay: u32, priority: i32 },
    MarkChunkDirty { dim: DimId, cx: i32, cz: i32 },
    CallbackJava { handle: JavaCallbackId, arg: BlobRef },
    Custom { id: CustomCommandId, payload: BlobRef },
}
```

- R-220 : chaque `Command` DOIT être applicable par un applicateur enregistré côté Java, capable de refuser l'application (validation) sans effet partiel.
- R-221 : `Custom` est réservé au SDK Rust (C-39) et exige un applicateur fourni par le module qui l'émet.
- R-222 : aucun `Command` NE DOIT écrire directement dans les structures internes de Minecraft ; l'application passe toujours par l'API publique/mixinée appropriée sur le thread autoritatif.

## 4.17 DM-17 : Enregistrements persistés

| Fichier | Contenu | Format | Versionné |
|---|---|---|---|
| `workload.db` | descripteurs, dynamiques agrégées, historique | binaire append-only + index | oui |
| `optimization.db` | décisions, stratégies, gains mesurés | binaire | oui |
| `compatibility.db` | règles dérivées, quarantaines | binaire | oui |
| `benchmark.db` | résultats de benchmarks internes | binaire | oui |
| `optimization.log` | journal humain des décisions | texte | non |
| `rfx-crash-<ts>.json` | dump de workload et d'état lors d'un incident | JSON | oui |

- R-223 : tous les fichiers persistés DOIVENT commencer par un en-tête `magic + schema_version + fingerprint`. Un schéma inconnu entraîne le renommage du fichier en `.bak` et la recréation, jamais une lecture partielle.
- R-224 : les fichiers DOIVENT être écrits par écriture atomique (fichier temporaire + `rename`).
- R-225 : la taille cumulée des fichiers RF-X DOIT être bornée par configuration (défaut 256 Mio) avec éviction LRU.

## 4.18 Emplacement sur disque

```text
<gameDir>/rustforgex/
├── config/rustforgex.toml
├── cache/
│   ├── workload.db
│   ├── optimization.db
│   ├── compatibility.db
│   ├── benchmark.db
│   └── ir/                      # artefacts IR et code compilé (EXPERIMENTAL)
├── logs/optimization.log
├── reports/                     # exports d'analyse, benchmarks
└── crash/
```

---

# PARTIE 5 : SPÉCIFICATIONS COMPOSANT PAR COMPOSANT

Chaque composant suit le même gabarit :

```text
Purpose / Inputs / Outputs / State / Dependencies / API / Algorithm
/ Failure modes / Fallback / Metrics / Tests / Acceptance
```

## 5.1 C-01 : Forge Integration

**Purpose.** Fournir l'unique point d'ancrage entre l'écosystème Forge et RUSTFORGE-X : cycle de vie, phases de tick, accès aux registres, découverte des mods, enregistrement des commandes. Isoler tout le reste du système de l'API Forge (P-11).

**Inputs.** Événements Forge (`FMLConstructModEvent`, `FMLCommonSetupEvent`, `FMLLoadCompleteEvent`, `ServerStartingEvent`, `ServerStoppingEvent`, `TickEvent.ServerTickEvent`, `TickEvent.ClientTickEvent`, `LevelEvent.Load/Unload`, `RegisterCommandsEvent`), configuration (C-37).

**Outputs.** Appels aux hooks du runtime (`rfx_tick_begin`, `rfx_phase`, `rfx_tick_end`), inventaire de mods (vers C-41), enregistrement des commandes (C-38).

**State.**

```text
UNLOADED -> CONSTRUCTED -> SETUP -> LOAD_COMPLETE -> RUNNING_(CLIENT|SERVER)
         -> STOPPING -> UNLOADED
```

**Dependencies.** C-02, C-37, C-41.

**API (Java).**

```java
public interface PlatformAdapter {          // IF-10, abstraction du loader
    String platformName();                  // "forge-47"
    String minecraftVersion();
    List<ModInfo> mods();
    boolean isAuthoritativeThread();
    long currentTick();
    Side side();
    void registerCommand(CommandSpec spec);
    void runOnAuthoritativeThread(Runnable r);   // exécute au prochain point sûr
}
```

**Algorithm.** L'intégration DOIT s'attacher aux deux bus (`MinecraftForge.EVENT_BUS` et le mod event bus) avec la priorité la plus haute possible pour PRE et la plus basse pour POST, afin d'encadrer strictement le travail des autres mods.

```text
onServerTickPre:  rfx_tick_begin(tick, side); rfx_phase(PRE)
                  publication des décisions, préparation snapshots, submit
onServerTickPost: rfx_phase(DRAIN); drain + commit; rfx_phase(POST); rfx_tick_end()
```

**Failure modes.**

| ID | Défaillance | Détection | Réaction |
|---|---|---|---|
| FM-01 | version de Forge non supportée | comparaison de plage au `CONSTRUCT` | log `E-1001`, mode `OBSERVE_ONLY`, aucune transformation |
| FM-02 | exception dans un hook RF-X | `try/catch` obligatoire autour de chaque hook | log, incrément `hook_errors`, désactivation du hook après N=5 échecs |
| FM-03 | tick sans POST (crash d'un autre mod) | watchdog de fenêtre de tick | fermeture forcée de la fenêtre, invalidation des snapshots ouverts |

**Fallback.** Toute défaillance de C-01 ramène le système à `OBSERVE_ONLY` puis à `DISABLED` sans empêcher le jeu de tourner.

**Metrics.** `rfx.hook.duration_ns{phase}`, `rfx.hook.errors`, `rfx.tick.window_open_ns`.

**Tests.** T-100 (chargement du mod sur serveur dédié), T-101 (chargement client), T-102 (version de Forge hors plage => OBSERVE_ONLY), T-103 (exception injectée dans un hook n'interrompt pas le tick).

**Acceptance.** Le JAR se charge sur Forge 47.x client et serveur, la partie démarre, aucun hook ne dépasse 50 µs en mode observation seule sur le profil `light`.

## 5.2 C-02 : Bootstrap

**Purpose.** Initialiser le runtime dans un ordre strict et vérifiable, ou refuser proprement de s'activer.

**Inputs.** Configuration, propriétés système (`-Drustforgex.*`), résultat de C-45, présence des binaires natifs.

**Outputs.** Handle de runtime natif, état d'activation, rapport d'initialisation.

**State.** `INIT -> PROBE -> LOAD_NATIVE -> HANDSHAKE -> CONFIGURE -> READY | DEGRADED | DISABLED`

**Dependencies.** C-03, C-37, C-45, C-27.

**Algorithm (normatif).**

```text
1. lire la configuration (fichier + surcharges système)
2. si config.enabled == false            -> DISABLED
3. détecter plateforme (OS, arch, libc)  -> sélectionner le natif
4. extraire le natif dans <gameDir>/rustforgex/native/<hash>/  (idempotent)
5. charger la bibliothèque (C-03)
6. handshake ABI : rfx_abi_version() doit == ABI_VERSION attendu (IF-01)
7. rfx_init(config_blob) -> RuntimeHandle
8. sonder le matériel (C-45), calibrer coût JNI
9. vérifier l'intégrité des caches (fingerprint)
10. -> READY
en cas d'échec d'une étape 3..9 : journaliser, -> DEGRADED (mode Java pur, observation
   éventuellement désactivée) et JAMAIS lever une exception non capturée
```

**Failure modes.**

| ID | Défaillance | Réaction |
|---|---|---|
| FM-04 | binaire natif absent pour la plateforme | `DEGRADED`, message clair, le jeu tourne sans RF-X |
| FM-05 | `UnsatisfiedLinkError` | idem + diagnostic (libc, glibc version, permissions `noexec`) |
| FM-06 | mismatch de version ABI | `DISABLED` + `E-1002`, jamais d'appel natif |
| FM-07 | cache corrompu | rotation en `.bak`, recréation |

**Fallback.** `DEGRADED` DOIT rester parfaitement jouable : aucune instrumentation, aucun thread supplémentaire.

**Metrics.** `rfx.boot.duration_ms`, `rfx.boot.state`, `rfx.boot.native_path`.

**Tests.** T-110 (démarrage nominal), T-111 (natif supprimé => DEGRADED), T-112 (ABI incompatible simulé => DISABLED), T-113 (répertoire non inscriptible => DEGRADED), T-114 (double init impossible).

**Acceptance.** Sur les trois plateformes cibles, `rfx.boot.state == READY` et durée de boot inférieure à 300 ms hors extraction initiale.

## 5.3 C-03 : Native Loader

**Purpose.** Extraire et charger la bibliothèque native de façon sûre, reproductible et vérifiable.

**Algorithm.**

```text
resource = "/natives/" + os + "-" + arch + "/" + libname
sha256   = lire "/natives/<os>-<arch>/<libname>.sha256"
target   = <gameDir>/rustforgex/native/<sha256>/<libname>
si target existe et sha256(target) == sha256 attendu -> charger
sinon extraire dans <tmp> puis renommer atomiquement -> charger
System.load(target.absolutePath)
```

- R-300 : la vérification SHA-256 est **obligatoire** avant chargement (`E-1003` sinon).
- R-301 : le chemin d'extraction DOIT être versionné par hash pour éviter tout conflit entre versions.
- R-302 : si le système de fichiers est monté `noexec` ou en lecture seule, le loader DOIT tenter `java.io.tmpdir` puis échouer proprement.

**Failure modes.** FM-04, FM-05 ci-dessus ; FM-08 : hash invalide (fichier corrompu ou altéré) => refus de chargement, `DISABLED`.

**Tests.** T-120 (extraction et hash), T-121 (fichier altéré rejeté), T-122 (répertoire en lecture seule), T-123 (deux versions coexistent).

**Acceptance.** Chargement réussi sur Windows x64, Linux x64 glibc, et rejet effectif d'un binaire altéré.

## 5.4 C-04 : JVM Instrumentation

**Purpose.** Poser, retirer et ajuster des sondes dans le bytecode des classes chargées, avec un coût borné et un impact nul sur la sémantique.

**Inputs.** Flux de transformation de classes (Mixin/ModLauncher pour les classes du jeu et des mods), demandes de sondage émises par C-05.

**Outputs.** Classes transformées, table `probeId -> WorkId`, notifications d'événements de sonde vers les buffers de profilage.

**State.** Par méthode : `UNPROBED -> COUNTER_ONLY -> TIMED -> DEEP -> UNPROBED`.

**Dependencies.** C-01, C-05, C-37.

**API (Java).**

```java
public interface Instrumentation {          // IF-11
    ProbeHandle probe(MethodRef m, ProbeLevel level);   // COUNTER | TIMED | DEEP
    void unprobe(ProbeHandle h);
    boolean supportsRetransform();
    InstrumentationStats stats();
}
```

**Algorithm.**

Deux mécanismes complémentaires, tous deux `STABLE` :

```text
A. Transformation au chargement (obligatoire)
   - transformateur enregistré en DERNIER dans la chaîne, de sorte que
     bytecode_hash (DM-05) reflète l'état final vu par la JVM
   - injection d'un préambule/postambule minimal :
       RfxProbes.enter(probeId)  ... try/finally ... RfxProbes.exit(probeId)
   - le finally est OBLIGATOIRE pour que les exceptions ne faussent pas les compteurs

B. Retransformation à chaud (optionnelle, EXPERIMENTAL)
   - via java.lang.instrument si un agent est disponible
   - autorisée uniquement hors fenêtre de tick, budget max 5 classes / seconde
   - jamais utilisée pour modifier la logique, seulement le niveau de sonde
```

Niveaux de sonde et coût cible :

| Niveau | Contenu injecté | Coût cible par appel |
|---|---|---|
| `COUNTER` | incrément d'un compteur thread-local | < 5 ns |
| `TIMED` | compteur + `System.nanoTime()` entrée/sortie | < 40 ns |
| `DEEP` | TIMED + pile de contexte + compteur d'allocation approché | < 150 ns |

- R-310 : l'instrumentation NE DOIT PAS modifier la sémantique observable : pas de capture d'exception, pas de changement de signature, pas de réordonnancement.
- R-311 : le sondage DOIT être refusé pour les méthodes trop courtes (moins de 12 instructions bytecode hors sondes) sauf si `calls_per_tick` est très élevé et le niveau `COUNTER`.
- R-312 : les constructeurs, `<clinit>`, méthodes natives, méthodes synchronisées de moins de 20 instructions et méthodes de classes du chargeur de démarrage NE DOIVENT PAS être sondés en V1.0.
- R-313 : l'impact JIT DOIT être surveillé : si le p50 d'une méthode augmente de plus de `probe_regression_threshold` (défaut 15 %) après sondage sans changement de charge, la sonde est retirée (RISK-08).

**Failure modes.**

| ID | Défaillance | Réaction |
|---|---|---|
| FM-09 | échec de transformation (bytecode inattendu, version de classe) | méthode marquée `UNPROBEABLE`, aucune exception propagée |
| FM-10 | conflit avec un autre transformateur | détection au chargement (checksum inattendu) => quarantaine de la classe |
| FM-11 | débordement du buffer de sondes | perte d'échantillons comptabilisée, jamais de blocage |

**Fallback.** Absence totale de sondes => C-05 bascule en échantillonnage pur (stack sampling), moins précis mais sans instrumentation.

**Metrics.** `rfx.instr.classes_transformed`, `rfx.instr.methods_probed{level}`, `rfx.instr.transform_failures`, `rfx.instr.overhead_ns_per_call`.

**Tests.** T-130 (sémantique préservée sur un corpus de méthodes de référence, y compris exceptions et boucles), T-131 (coût par niveau mesuré sous seuil), T-132 (méthode non transformable ignorée proprement), T-133 (retransformation hors tick uniquement), T-134 (pas de sondes sur `<clinit>`).

**Acceptance.** Sur le profil `medium`, l'instrumentation complète en mode `WARM` coûte moins de 1,5 % du MSPT, et aucun test de gameplay ne diverge (T-4xx).

## 5.5 C-05 : Profiler

**Purpose.** Produire une image fidèle et budgetée de l'activité réelle, et adapter automatiquement sa profondeur.

**Inputs.** Événements de sonde (C-04), échantillonnage périodique, événements Forge (C-06), compteurs JVM (`ThreadMXBean`, `GarbageCollectorMXBean`, `com.sun.management` pour l'allocation par thread quand disponible).

**Outputs.** `WorkloadDynamics` (DM-04) par `WorkId`, temps par phase de tick, profil d'allocation, arbre d'appel échantillonné.

**State.** `OFF -> LIGHT -> NORMAL -> DEEP -> THROTTLED -> OFF`

**Dependencies.** C-04, C-06, C-34, C-37.

**Algorithm.**

```text
1. Collecte
   - anneau de tampons thread-local (lock-free, taille fixe, 64 Kio par thread)
   - flush vers le natif en fin de tick (une seule traversée FFI par tick et par thread)
2. Échantillonnage
   - thread d'échantillonnage à 100 Hz par défaut
   - capture des piles des threads d'intérêt uniquement (autoritatif + threads chauds)
3. Agrégation (côté Rust)
   - mise à jour des EWMA et histogrammes par WorkId
   - calcul de la chaleur (DM-04)
4. Adaptation
   - COLD    -> COUNTER only, échantillonnage seul
   - WARM    -> TIMED
   - HOT     -> TIMED + contexte
   - CRITICAL-> DEEP + demande d'analyse statique prioritaire
5. Auto-mesure
   - le profiler mesure son propre coût : temps passé dans enter/exit + flush + sampling
   - si overhead > budget (défaut 2 % d'un cœur ou 1,5 % du MSPT) :
        réduire d'un niveau ; si déjà LIGHT -> THROTTLED (échantillonnage seul) ;
        si toujours au-dessus -> OFF + log E-1201
```

Budgets par défaut :

| Budget | Valeur par défaut | Configurable |
|---|---|---|
| CPU profiling | 2 % d'un cœur | `profiler.cpu_budget_pct` |
| Mémoire profiling | 64 Mio | `profiler.memory_budget_mb` |
| Fréquence d'échantillonnage | 100 Hz | `profiler.sample_hz` |
| Workloads suivis | 20 000 | `profiler.max_workloads` |
| Profondeur de contexte | 3 | `profiler.context_depth` |

- R-320 : le profiler NE DOIT JAMAIS allouer dans le chemin chaud (pas de `new` par appel sondé).
- R-321 : dépassement du nombre maximal de workloads => éviction LRU des `COLD`, jamais de croissance non bornée.
- R-322 : le profiler DOIT fonctionner en l'absence totale de sondes (mode échantillonnage).

**Failure modes.** FM-12 : dérive d'horloge ou `nanoTime` non monotone sur certaines plateformes => détection, bascule sur compteur d'appels seul. FM-13 : perte d'échantillons > 5 % => réduction de niveau.

**Fallback.** `THROTTLED` puis `OFF` ; les décisions existantes restent actives mais aucune nouvelle décision n'est prise sans mesure.

**Metrics.** `rfx.profiler.overhead_pct`, `rfx.profiler.level`, `rfx.profiler.workloads_tracked`, `rfx.profiler.samples_lost`.

**Tests.** T-140 (overhead mesuré < budget sur `heavy`), T-141 (adaptation de niveau sous charge), T-142 (aucune allocation dans le chemin chaud, vérifié par compteur d'allocation), T-143 (éviction LRU correcte), T-144 (fonctionnement sans sondes).

**Acceptance.** Sur profil `heavy`, overhead mesuré et rapporté < 2 %, et le classement des 20 workloads les plus coûteux est stable entre deux exécutions identiques (corrélation de rang > 0,9).

## 5.6 C-06 : Event Observer

**Purpose.** Modéliser le système d'événements Forge comme un graphe de handlers exploitable, et préserver strictement sa sémantique observable.

**Inputs.** Registres de listeners des bus Forge, invocations d'événements.

**Outputs.** `HandlerInstance` par listener, ordre effectif par type d'événement, statistiques par handler, détection d'annulation et de résultat.

**Modèle.**

```text
HandlerInstance
 ├── owner            (modid déduit du chargeur de classe / du package enregistré)
 ├── event_type
 ├── bus              (FORGE | MOD)
 ├── priority         (HIGHEST..LOWEST)
 ├── receive_canceled (bool)
 ├── work_id          (DM-01)
 ├── reads/writes     (via C-11)
 ├── observable_effects (cancel, setResult, mutation de l'événement)
 └── execution_plan   (stratégie active)
```

**Algorithm.**

```text
1. Au LOAD_COMPLETE, énumérer les listeners par réflexion sur les bus Forge.
2. Envelopper chaque listener par un proxy RF-X qui :
     - conserve l'ordre et la priorité d'origine
     - mesure le temps et compte les appels
     - détecte cancel()/setResult() et toute mutation des champs de l'événement
3. Construire, par type d'événement, la séquence ordonnée des handlers.
4. Marquer ORDER_SENSITIVE tout handler qui :
     - annule l'événement, OU
     - modifie l'événement, OU
     - est précédé par un handler pouvant annuler l'événement
5. Les handlers restants sont candidats à l'exécution parallèle SI leurs
   read/write sets sont disjoints (C-23).
```

- R-330 : l'ordre observable NE DOIT JAMAIS changer. Une exécution parallèle est autorisée uniquement si les résultats sont ensuite appliqués dans l'ordre d'origine et si aucun handler n'observe l'effet d'un autre.
- R-331 : si un handler annule un événement, tous les handlers suivants qui ne déclarent pas `receiveCanceled` NE DOIVENT PAS être exécutés, y compris en exécution anticipée. Toute exécution anticipée d'un tel handler DOIT être une shadow execution sans effet.
- R-332 : les événements dont le type porte un résultat (`Event.Result`) sont traités comme `ORDER_SENSITIVE` par défaut.

**Failure modes.** FM-14 : le proxy modifie l'identité d'un listener et casse un mod qui compare des références => enregistrement du proxy conservant `equals/hashCode` délégués ; si le mod inspecte le type concret, désenveloppement automatique et quarantaine du handler.

**Fallback.** Désenveloppement complet, retour à l'observation par échantillonnage.

**Metrics.** `rfx.events.handlers_wrapped`, `rfx.events.dispatch_ns{event_type}`, `rfx.events.cancels`, `rfx.events.unwrapped_due_to_conflict`.

**Tests.** T-150 (ordre préservé sur 1000 événements avec priorités mixtes), T-151 (annulation respectée), T-152 (résultat d'événement inchangé), T-153 (mod hostile inspectant le listener => désenveloppement), T-154 (surcoût du proxy < 30 ns).

**Acceptance.** Suite gameplay complète sans divergence, ordre d'événements identique à la référence non instrumentée (comparaison de traces).

## 5.7 C-07 : Call Graph

**Purpose.** Construire le graphe d'appel partiel nécessaire à l'analyse de dépendances, en combinant appels statiquement résolus et arêtes observées dynamiquement.

**Inputs.** Bytecode analysé (C-04 fournit les octets), contextes d'appel observés (C-05).

**Outputs.** Graphe orienté `MethodRef -> MethodRef` avec attribut `resolved | virtual_unresolved | dynamic_observed`, composantes fortement connexes (récursion), profondeur.

**Algorithm.**

```text
1. Extraire les instructions invoke* de la méthode cible.
2. invokestatic / invokespecial      -> arête résolue
3. invokevirtual / invokeinterface   -> résolution par hiérarchie de classes chargées :
      - si une seule implémentation chargée -> arête résolue (monomorphe)
      - si <= 4 implémentations -> arêtes multiples (polymorphe borné)
      - sinon -> unresolved_calls += 1
4. invokedynamic                     -> unresolved sauf lambda dont la cible statique
                                        est identifiable dans le pool de constantes
5. Réflexion détectée (Method.invoke, MethodHandle.invoke*) -> unresolved + drapeau
6. Fusion avec les arêtes observées dynamiquement (contexte d'appel réel)
7. Détection des cycles (Tarjan) -> marquage RECURSIVE
```

- R-340 : l'exploration est bornée par `analysis.max_call_depth` (défaut 12) et `analysis.max_nodes_per_workload` (défaut 2000). Un dépassement positionne `DependencySet.truncated`.
- R-341 : le polymorphisme non borné est toujours traité comme `unresolved` (P-02).

**Failure modes.** FM-15 : classe non trouvée lors de la résolution => arête `unresolved`, jamais d'exception.

**Metrics.** `rfx.callgraph.nodes`, `rfx.callgraph.unresolved_ratio`, `rfx.callgraph.build_ns`.

**Tests.** T-160 (résolution monomorphe correcte), T-161 (lambda résolue), T-162 (réflexion => unresolved), T-163 (récursion détectée), T-164 (respect des bornes).

**Acceptance.** Sur un corpus de 200 méthodes de référence, taux de résolution supérieur à 80 % et zéro faux "résolu" (aucune arête déclarée résolue qui ne l'est pas).

## 5.8 C-08 : Workload Profiler

**Purpose.** Fusionner observation dynamique et analyse statique en un `WorkloadDescriptor` cohérent, et maintenir sa fraîcheur.

**Inputs.** `WorkloadDynamics` (C-05), résultats de C-07, C-10, C-11, C-12, C-13.

**Outputs.** `WorkloadDescriptor` complet, publication d'événements de changement vers C-15.

**Algorithm.**

```text
Boucle sur le thread d'analyse (priorité basse), budget par cycle :
 1. sélectionner les N workloads les plus "rentables à analyser" :
        score_analyse = heat * (1 - confidence.combined) / cout_analyse_estime
 2. pour chaque workload : lancer les analyses manquantes dans l'ordre
        C-07 -> C-10 -> C-11 -> C-12 -> C-13
    chaque étape peut être interrompue par épuisement du budget (résultat partiel
    marqué truncated, jamais un résultat optimiste)
 3. calculer flags (DM-11) et confidence (DM-12)
 4. si le descripteur change de manière significative -> notifier C-15
 5. persister (C-09) de façon asynchrone
```

- R-350 : un descripteur DOIT être recalculé si son `bytecode_hash` change, si son mod est rechargé, ou si `mismatches` augmente.
- R-351 : le calcul des flags DOIT être une fonction pure des faits (testable sans runtime).

**Failure modes.** FM-16 : analyse trop longue => interruption coopérative, résultat partiel conservateur.

**Metrics.** `rfx.analysis.queue_depth`, `rfx.analysis.completed`, `rfx.analysis.truncated`, `rfx.analysis.cpu_ms`.

**Tests.** T-170 (fonction de flags pure : table de vérité complète), T-171 (interruption produit un résultat conservateur), T-172 (recalcul sur changement de hash).

**Acceptance.** Sur profil `heavy`, 95 % des workloads `HOT` disposent d'un descripteur complet en moins de 5 minutes de jeu, sans dépasser le budget d'analyse.

## 5.9 C-09 : Workload Database

**Purpose.** Stocker, indexer et faire vieillir les descripteurs, leurs mesures et leur historique de décisions, en mémoire et sur disque.

**Structure en mémoire.**

```text
slab de descripteurs        : Vec<WorkloadDescriptor>, index dense
index par WorkId            : HashMap<WorkId, Idx> (ahash)
index par owner             : HashMap<OwnerId, SmallVec<Idx>>
index par heat              : 4 files (COLD/WARM/HOT/CRITICAL)
LRU des COLD                : liste chaînée intrusive
historique                  : anneau borné par workload (défaut 32 entrées)
```

**API.**

```rust
pub trait WorkloadStore {                                    // IF-12
    fn upsert(&mut self, d: WorkloadDescriptor) -> Idx;
    fn get(&self, id: WorkId) -> Option<&WorkloadDescriptor>;
    fn get_mut(&mut self, id: WorkId) -> Option<&mut WorkloadDescriptor>;
    fn by_owner(&self, o: OwnerId) -> &[Idx];
    fn hot(&self) -> &[Idx];
    fn record(&mut self, id: WorkId, event: HistoryEvent);
    fn evict(&mut self, budget: MemoryBudget) -> usize;
    fn persist(&self, path: &Path, fp: &Fingerprint) -> Result<()>;
    fn load(path: &Path, fp: &Fingerprint) -> Result<Self>;
}
```

- R-360 : `load` DOIT rejeter tout fichier dont le fingerprint global diffère et DOIT filtrer par entrée les descripteurs dont le fingerprint local diffère.
- R-361 : l'écriture disque DOIT être asynchrone, hors tick, et atomique.
- R-362 : la base DOIT rester bornée : `max_workloads` en mémoire, `max_db_size_mb` sur disque.

**Failure modes.** FM-17 : fichier corrompu => rotation et recréation ; FM-18 : disque plein => désactivation de la persistance, runtime continue en mémoire.

**Metrics.** `rfx.db.entries`, `rfx.db.evictions`, `rfx.db.persist_ms`, `rfx.db.load_rejected_entries`.

**Tests.** T-180 (round-trip persistance), T-181 (rejet sur fingerprint différent), T-182 (corruption détectée), T-183 (éviction sous budget), T-184 (aucun blocage du tick lors de la persistance).

**Acceptance.** 20 000 workloads tenus en moins de 64 Mio, chargement du cache en moins de 200 ms.

## 5.10 C-10 : Dependency Analyzer

**Purpose.** Déterminer de quoi dépend un workload et ce qui dépend de lui, pour autoriser ou interdire le parallélisme et le réordonnancement.

**Inputs.** Graphe d'appel (C-07), read/write sets (C-11), ordre d'événements (C-06), observations dynamiques.

**Outputs.** `DependencySet` (DM-08), arêtes du task graph (C-16).

**Algorithm.**

```text
Pour un workload W :
 1. Dépendances de données :
      pour chaque autre workload X du même tick :
         si writes(X) ∩ reads(W) != ∅   -> arête RAW  X -> W
         si writes(X) ∩ writes(W) != ∅  -> arête WAW  (conflit)
         si reads(X)  ∩ writes(W) != ∅  -> arête WAR
 2. Dépendances d'ordre :
      si W et X appartiennent au même flux d'événement et que l'un peut annuler
      ou modifier l'événement -> arête ORDER
 3. Dépendances de contrôle :
      si W est exécuté conditionnellement d'après le résultat de X -> arête CONTROL
 4. Dépendances externes :
      locks partagés, files partagées, ressources IO -> arête RESOURCE
 5. Propagation conservatrice :
      si precision(reads|writes) == UNKNOWN -> W dépend de TOUT le tick
```

- R-370 : l'intersection des domaines DOIT être calculée d'abord au niveau `Domain` (bitset, O(1)), puis affinée au niveau `Region` seulement si les domaines se croisent.
- R-371 : deux workloads dont les régions sont des chunks distincts et dont les domaines sont limités à `ChunkData`/`EntityState` de ces chunks SONT indépendants, sauf si l'un d'eux touche `LevelGlobals`.
- R-372 : toute dépendance ajoutée par mesure de sûreté DOIT être explicable (motif enregistré) pour C-43.

**Failure modes.** FM-19 : explosion combinatoire du nombre de paires => limitation par partitionnement spatial (grille de chunks) et plafond `max_pairs_per_tick`, au-delà duquel le parallélisme est refusé pour le tick.

**Fallback.** Sérialisation complète du tick (comportement identique au jeu non optimisé).

**Metrics.** `rfx.dep.edges`, `rfx.dep.conflicts`, `rfx.dep.conservative_edges`, `rfx.dep.analysis_ns`.

**Tests.** T-190 (indépendance de chunks disjoints), T-191 (RAW détecté), T-192 (UNKNOWN => dépendance totale), T-193 (plafond respecté), T-194 (résultat identique au séquentiel sur scénario déterministe).

**Acceptance.** Aucun faux négatif de dépendance sur la suite de conflits synthétiques T-19x, coût d'analyse inférieur à 0,3 ms par tick sur `heavy`.

## 5.11 C-11 : Data Flow Analyzer

**Purpose.** Calculer les ensembles de lecture et d'écriture (DM-06) d'un workload, avec une précision explicite.

**Inputs.** Bytecode de la méthode et de ses appelées (bornées par C-07), signatures connues, observations dynamiques d'échantillonnage.

**Outputs.** `AccessSet` reads/writes, `SideEffectSet`, précision.

**Algorithm (analyse abstraite sur bytecode).**

```text
Domaine abstrait : pile et variables locales étiquetées par un ensemble de
"sources" abstraites :
   THIS, PARAM(i), STATIC(class.field), NEW, WORLD, CHUNK, ENTITY, BLOCK_ENTITY,
   UNKNOWN

Transfert :
   getfield  obj.f    -> lit  Field(class,f) ; source(result) = f(source(obj))
   putfield  obj.f    -> écrit Field(class,f) ; si source(obj) = WORLD|ENTITY ...
                          -> écrit le domaine correspondant
   getstatic/putstatic-> lit/écrit ModStatics ou JavaStatics
   aaload/aastore     -> lit/écrit le domaine de la source du tableau
   invoke*            -> union des effets de la cible si analysée,
                          sinon effets = UNKNOWN (tout)
   monitorenter/exit  -> uses_locks = true
   athrow             -> throws_observable si non capturé localement

Reconnaissance de domaine :
   la correspondance "classe Minecraft -> domaine" est établie par une table
   dérivée automatiquement au démarrage à partir de la hiérarchie de classes
   (ex : toute sous-classe de net.minecraft.world.entity.Entity -> EntityState),
   jamais par une liste de mods.
```

- R-380 : une méthode dont un appel est `UNKNOWN` a `precision = UNKNOWN` et `SideEffectSet.unknown = true`.
- R-381 : la précision `EXACT` n'est atteignable que si toutes les cibles d'appel sont résolues et analysées, sans réflexion ni `invokedynamic` non résolu.
- R-382 : la vérification dynamique DOIT échantillonner les accès réels d'un sous-ensemble de workloads `HOT` pour détecter les sous-approximations ; toute divergence rétrograde le workload et incrémente `dataflow_mismatch`.

**Failure modes.** FM-20 : bytecode non analysable (version future, obfuscation extrême) => `UNKNOWN`.

**Metrics.** `rfx.dataflow.exact_ratio`, `rfx.dataflow.mismatch`, `rfx.dataflow.analysis_ns`.

**Tests.** T-200 (méthodes pures reconnues), T-201 (écriture de champ détectée), T-202 (appel non résolu => UNKNOWN), T-203 (tableau et alias), T-204 (aucune sous-approximation sur corpus annoté), T-205 (détection de divergence dynamique).

**Acceptance.** Zéro sous-approximation d'écriture sur le corpus annoté de 150 méthodes ; au moins 60 % des méthodes du corpus classées avec `precision = EXACT`.

## 5.12 C-12 : Thread Analyzer

**Purpose.** Déterminer les contraintes de thread d'un workload.

**Algorithm.**

```text
1. Observation : sur quels rôles de thread le workload a-t-il été vu ?
2. Statique : appelle-t-il une API confinée ? utilise-t-il ThreadLocal ?
   déclare-t-il synchronized ? accède-t-il à des champs mutables non volatiles
   d'objets partagés ?
3. Confinement dérivé : une classe est réputée non thread-safe si elle possède
   au moins un champ mutable non final, non volatile, écrit hors constructeur,
   sans synchronisation, et si elle est atteignable depuis l'état autoritatif.
4. Conclusion :
     main_thread_required = YES | NO | UNKNOWN (défaut UNKNOWN)
```

- R-390 : un workload observé sur plusieurs rôles de thread et écrivant un état partagé est marqué `UNSAFE`.
- R-391 : `THREAD_SAFE` exige soit `PURE`, soit une preuve d'isolation (écritures uniquement dans des structures possédées par la tâche).

**Metrics.** `rfx.thread.main_required_ratio`, `rfx.thread.unknown_ratio`.

**Tests.** T-210 (détection de ThreadLocal), T-211 (confinement dérivé), T-212 (multi-thread + écriture => UNSAFE).

**Acceptance.** Aucun workload classé `THREAD_SAFE` ne provoque de `ConcurrentModificationException` ni de divergence dans la suite de stress T-6xx.

## 5.13 C-13 : Determinism Analyzer

**Purpose.** Déterminer si un workload produit le même résultat observable pour les mêmes entrées, et si son ordre est observable.

**Algorithm.**

```text
Sources de non-déterminisme recherchées statiquement :
   - appels à un RNG partagé (champs de type RandomSource atteignables)
   - System.currentTimeMillis / nanoTime
   - System.identityHashCode, Object.hashCode non surchargé
   - itération sur HashMap/HashSet dont l'ordre influence la sortie
   - accès IO / réseau
   - appels natifs
   - réflexion

Vérification dynamique :
   - pour un workload PURE candidat, exécuter en shadow N fois (défaut 32)
     sur les mêmes entrées et comparer les sorties normalisées
   - toute divergence -> NON_DETERMINISTIC définitif pour ce fingerprint

RNG :
   - un workload utilisant un RNG peut redevenir DETERMINISTIC si et seulement si
     le RNG est remplaçable par un flux dérivé déterministe indexé
     (seed = f(world_seed, tick, WorkId, index)) ET que ce remplacement ne change
     pas la séquence observée par le reste du jeu.
     En V1.0, ce remplacement est EXPERIMENTAL et interdit sur le serveur en
     multijoueur (risque de divergence client/serveur).
```

- R-400 : l'arithmétique flottante réordonnée DOIT être considérée comme changeant le résultat ; une réduction parallèle DOIT utiliser un arbre de réduction fixe et déterministe.
- R-401 : `deterministic = UNKNOWN` interdit TL3 et au-delà.

**Metrics.** `rfx.det.deterministic_ratio`, `rfx.det.shadow_mismatches`, `rfx.det.nondet_sources{source}`.

**Tests.** T-220 (détection RNG), T-221 (détection itération de HashMap influente), T-222 (réduction flottante déterministe), T-223 (32 shadow runs identiques requis pour promotion).

**Acceptance.** Aucun workload promu en TL4+ n'échoue ensuite en validation pour non-déterminisme dans la suite longue T-7xx.

## 5.14 C-14 : Risk Engine

**Purpose.** Transformer les faits d'analyse en un risque quantifié, indépendant du bénéfice attendu. Le risque et le gain sont évalués séparément puis confrontés par C-15.

**Modèle.**

```rust
pub struct RiskAssessment {
    pub correctness_risk: f32,   // 0..1 : probabilité de changer le comportement
    pub stability_risk: f32,     // 0..1 : crash, deadlock, timeout
    pub compat_risk: f32,        // 0..1 : conflit avec un autre mod / transformateur
    pub data_risk: f32,          // 0..1 : corruption d'état ou de sauvegarde
    pub max_blast_radius: Blast, // TASK | WORKLOAD | SUBSYSTEM | WORLD
    pub reasons: SmallVec<[RiskReason; 8]>,
}
```

**Algorithm (règles, évaluées dans l'ordre, la première qui s'applique fixe un plancher).**

```text
R1  si flags.UNKNOWN                      -> correctness_risk = 1.0
R2  si side_effects.unknown               -> correctness_risk = max(_, 0.9)
R3  si !determinism.deterministic         -> correctness_risk = max(_, 0.8)
R4  si thread.main_thread_required != NO  -> stability_risk  = max(_, 0.8)
R5  si writes touche WorldBlocks|EntityState hors snapshot -> data_risk = 1.0
R6  si deps.truncated || unresolved > 0   -> correctness_risk = max(_, 0.7)
R7  si compat rule active pour ce owner   -> compat_risk selon la règle
R8  si historique contient un rollback    -> stability_risk += 0.3 par occurrence
R9  si validation.mismatches > 0          -> correctness_risk = 1.0 (définitif)
R10 sinon, risque de base fonction de (1 - confidence.combined)
```

Le **blast radius** détermine le niveau maximal autorisé :

| Blast | Niveau max autorisé |
|---|---|
| `TASK` | TL6 |
| `WORKLOAD` | TL5 |
| `SUBSYSTEM` | TL4 |
| `WORLD` | TL2 |

**Metrics.** `rfx.risk.distribution`, `rfx.risk.blocked_by_rule{rule}`.

**Tests.** T-230 (table de vérité des règles R1..R10), T-231 (monotonie : ajouter un fait défavorable n'abaisse jamais le risque), T-232 (mismatch => risque définitif).

**Acceptance.** La fonction de risque est pure, testée exhaustivement, et aucune stratégie TL4+ n'est jamais proposée avec `correctness_risk > 0.2`.

## 5.15 C-15 : Decision Engine

**Purpose.** Choisir, pour chaque workload, la stratégie d'exécution optimale sous contrainte de risque, de budget et de compatibilité. C'est le cœur du système.

**Inputs.** `WorkloadDescriptor`, `RiskAssessment`, règles de compatibilité (C-33), état matériel (C-45), historique et modèle appris (C-44), budgets courants (C-05, C-17), configuration (mode).

**Outputs.** `Decision` (stratégie + justification + conditions de révision).

```rust
pub struct Decision {
    pub id: DecisionId,
    pub work: WorkId,
    pub strategy: Strategy,
    pub score: Score,
    pub risk: RiskAssessment,
    pub reasons: Vec<Reason>,       // pour C-43
    pub revisit_after_ticks: u32,
    pub requires_validation: bool,
}
```

**Machine de décision (SM-02).**

```text
                     +--------------------+
                     |  CANDIDATE POOL    |
                     +---------+----------+
                               |
                    filtre de faisabilité (dur)
                               |
        +----------------------+----------------------+
        |                                             |
   INFAISABLE                                     FAISABLE
        |                                             |
     REJECT(motif)                        calcul du score par stratégie
                                                      |
                                        +-------------+-------------+
                                        |                           |
                                gain net <= seuil            gain net > seuil
                                        |                           |
                                    JAVA_ONLY              stratégie de score max
                                                                    |
                                            +-----------------------+
                                            |
                                 requires_validation ?
                                     oui -> SHADOW puis CANARY puis ACTIVE
                                     non -> ACTIVE (TL <= 2 uniquement)
```

**Filtre de faisabilité (dur, non score).**

```text
interdit si :
  - flags.UNKNOWN et stratégie != JAVA_ONLY
  - main_thread_required != NO et stratégie ∈ {parallel, offload}
  - !DETERMINISTIC et stratégie ∈ {parallel, offload, transformed}
  - ORDER_SENSITIVE et stratégie ∈ {parallel} sans restauration d'ordre prouvée
  - compat rule = FORCE_JAVA | DISABLE
  - risk.correctness_risk > mode.max_correctness_risk
  - TL demandé > TL max autorisé par blast radius
  - hardware insuffisant (ex : parallel 8 sur 4 cœurs logiques)
```

**Modèle de score.**

```text
Gain estimé (ns par tick) :
  G_cpu        = calls_per_tick * cpu_ns.p50 * speedup_estime(strategie)
  G_mainthread = part du temps retirée du thread autoritatif * poids_mainthread
  G_alloc      = alloc_bytes * cout_gc_par_octet
  G_cache      = (taux de réutilisation estimé) * cpu_ns.p50 * calls_per_tick

Coûts estimés (ns par tick) :
  C_transfer   = calls_effectifs * (jni_call_ns + payload_bytes / ffi_bandwidth)
  C_sync       = nb_barrieres * cout_barriere
  C_sched      = calls_effectifs * cout_soumission_tache
  C_snapshot   = octets_snapshot / debit_copie
  C_validation = amorti sur la fenêtre de validation

Score :
  S = (G_cpu + G_mainthread + G_alloc + G_cache)
      - (C_transfer + C_sync + C_sched + C_snapshot + C_validation)
      - penalite_risque(risk, mode)
      - penalite_historique(rollbacks, regressions)

Décision retenue :
  argmax_strategie S, sous contrainte S > seuil_activation(mode)
```

Poids par défaut (configurables, valeurs de départ à recalibrer par C-44) :

| Paramètre | Défaut | Note |
|---|---|---|
| `poids_mainthread` | 3.0 | une ns retirée du thread autoritatif vaut 3 ns ailleurs |
| `cout_gc_par_octet` | 0.02 ns/octet | recalibré par mesure GC réelle |
| `cout_soumission_tache` | 120 ns | mesuré par C-45 |
| `seuil_activation` SAFE | 200 000 ns/tick | 0,2 ms |
| `seuil_activation` BALANCED | 50 000 ns/tick | |
| `seuil_activation` PERFORMANCE | 20 000 ns/tick | |
| `max_correctness_risk` SAFE | 0.05 | |
| `max_correctness_risk` BALANCED | 0.15 | |
| `max_correctness_risk` PERFORMANCE | 0.30 | |

- R-410 : `speedup_estime` NE DOIT PAS être une constante inventée. Il provient (dans cet ordre) : d'une mesure réelle antérieure pour ce `WorkId` et ce fingerprint ; sinon d'un modèle appris (C-44) sur des workloads de profil comparable ; sinon d'une borne prudente (1,0 = aucun gain) qui empêche l'activation tant qu'une mesure n'existe pas.
- R-411 : toute décision DOIT être re-évaluée après `revisit_after_ticks` ou immédiatement en cas de changement de fingerprint, de mode, de matériel ou d'anomalie.
- R-412 : la décision DOIT être **stable** : un changement de stratégie ne peut pas survenir plus souvent que `decision.min_hold_ticks` (défaut 600 ticks) sauf démotion pour sûreté. Cela interdit l'oscillation.
- R-413 : le moteur NE DOIT JAMAIS lire le nom d'un mod pour décider (INV-12). Les règles de compatibilité (C-33) s'expriment en propriétés et fingerprints.

**Failure modes.** FM-21 : oscillation détectée (plus de 3 changements en 10 minutes) => verrouillage sur la stratégie la plus sûre observée et log `E-2201`. FM-22 : modèle de coût faux (gain mesuré négatif) => démotion immédiate et pénalisation du modèle (C-44).

**Fallback.** `JAVA_ONLY` est toujours une décision valide et disponible.

**Metrics.** `rfx.decision.count{strategy}`, `rfx.decision.rejected{reason}`, `rfx.decision.expected_gain_ns`, `rfx.decision.measured_gain_ns`, `rfx.decision.oscillations`.

**Tests.** T-240 (filtre de faisabilité exhaustif), T-241 (score monotone en gain et en coût), T-242 (aucune activation sans mesure ou modèle), T-243 (anti-oscillation), T-244 (mode SAFE ne dépasse jamais TL3), T-245 (aucune référence à un nom de mod : test statique sur le code source).

**Acceptance.** Sur profil `heavy`, au moins 20 stratégies non triviales activées, gain mesuré agrégé positif, zéro rollback pour cause de correction sur 4 heures de jeu.

## 5.16 C-16 : Task Graph

**Purpose.** Représenter le travail d'un tick sous forme de DAG exécutable, avec dépendances explicites, priorités et deadlines.

**Modèle.**

```rust
pub struct TaskNode {
    pub id: TaskId,
    pub work: WorkId,
    pub kind: TaskKind,          // COMPUTE | SNAPSHOT | COMMIT_PREP | SHADOW | ANALYSIS
    pub deps: SmallVec<[TaskId; 4]>,
    pub priority: Priority,      // CRITICAL | HIGH | NORMAL | LOW | BACKGROUND
    pub deadline: Deadline,      // THIS_TICK | TICK(+n) | NONE
    pub estimated_ns: u64,
    pub payload: TaskPayload,
    pub cancel: CancelToken,
}
```

**Invariants.**

- INV-01 : le graphe est acyclique. Toute arête créant un cycle est refusée et journalisée (`E-2301`).
- INV-02 : une tâche `THIS_TICK` non terminée à la fenêtre de drain est annulée et son workload retombe sur le chemin Java pour ce tick.

**Algorithm.**

```text
construction :
  pour chaque décision active applicable ce tick :
     créer les nœuds (snapshot -> compute -> commit_prep)
     ajouter les arêtes issues de C-10
  fusion de tâches (task fusion) :
     si deux nœuds consécutifs sont du même workload, sans autre dépendant,
     et que leur coût cumulé < fusion_threshold -> fusionner
  découpage (task splitting) :
     si estimated_ns > split_threshold et payload divisible (batch) ->
     découper en k morceaux, k = min(workers_libres, taille/grain_min)
  tri topologique par niveaux -> vagues d'exécution
```

**Metrics.** `rfx.taskgraph.nodes{tick}`, `rfx.taskgraph.depth`, `rfx.taskgraph.fusions`, `rfx.taskgraph.splits`, `rfx.taskgraph.cycles_rejected`.

**Tests.** T-250 (acyclicité forcée), T-251 (ordre topologique respecté), T-252 (fusion et découpage n'altèrent pas le résultat), T-253 (annulation à la deadline).

**Acceptance.** Construction du graphe en moins de 100 µs pour 500 nœuds, aucun cycle accepté, résultats identiques à l'exécution séquentielle sur la suite déterministe.

## 5.17 C-17 : Scheduler

**Purpose.** Exécuter le task graph en respectant dépendances, priorités et deadlines, sans jamais dégrader le thread autoritatif ni saturer la machine.

### 5.17.1 Architecture

```text
+-------------------------------------------------------------+
| Submission (thread autoritatif, lock-free)                  |
|   push -> file globale d'injection (MPSC bornée)            |
+-------------------------------------------------------------+
                 |
        +--------v---------+
        | Dispatcher       |  vagues topologiques, budget par tick
        +--------+---------+
                 |
   +-------------+--------------+---------------+
   v             v              v               v
Worker 0     Worker 1       Worker 2  ...   Worker N-1
 deque        deque          deque           deque       (Chase-Lev, LIFO local,
   ^            ^              ^               ^          FIFO au vol)
   +------------+--------------+---------------+
                     work stealing (victime aléatoire, 2 tentatives puis parking)
```

### 5.17.2 Dimensionnement

```text
workers_par_defaut = clamp(logical_cores - reserve, 1, 32)
reserve = 2 (thread autoritatif + render/netty) si logical_cores >= 6
          1 sinon
si core_kinds contient des E-cores : les workers BACKGROUND y sont affinés
si la charge système externe (threads runnables tiers) dépasse
   oversubscription_threshold (défaut 1.5 * logical_cores) :
   réduire dynamiquement le nombre de workers actifs (parking)
```

- R-420 : le scheduler NE DOIT JAMAIS créer plus de `logical_cores` threads actifs simultanés.
- R-421 : le scheduler DOIT détecter les pools de threads tiers (mods multithreadés) via le nombre de threads runnables du processus et réduire son parallélisme en conséquence (RISK-05).
- R-422 : aucun worker NE DOIT bloquer sur un lock détenu par le thread autoritatif (INV-06).

### 5.17.3 Politique de vol

```text
- deque local : LIFO (localité de cache)
- vol : FIFO depuis la tête de la victime (tâches les plus anciennes, souvent
  les plus grosses après découpage)
- choix de victime : aléatoire avec biais vers le même nœud NUMA / même cluster L3
- après 2 échecs : yield ; après 8 : parking avec réveil par condvar
```

### 5.17.4 Priorités et deadlines

| Priorité | Usage | Politique |
|---|---|---|
| `CRITICAL` | tâches à deadline `THIS_TICK` nécessaires au commit | exécutées avant tout, jamais volées vers un worker en veille profonde |
| `HIGH` | offloads actifs du tick | file prioritaire |
| `NORMAL` | batch, préparation | standard |
| `LOW` | shadow execution | uniquement si des workers sont libres |
| `BACKGROUND` | analyse, persistance, apprentissage | uniquement hors fenêtre critique |

- R-423 : une tâche `BACKGROUND` NE DOIT JAMAIS retarder une tâche `CRITICAL` : préemption coopérative par points de contrôle (`should_yield()`) obligatoires dans toute boucle d'analyse.

### 5.17.5 Backpressure

```text
si file d'injection > high_watermark (défaut 4096) :
    - refuser les nouvelles soumissions LOW/BACKGROUND (E-2401, non fatal)
    - le workload concerné exécute son chemin Java pour ce tick
si file > hard_limit (8192) :
    - refuser aussi NORMAL
    - déclencher une réévaluation des décisions (trop d'offload activé)
```

### 5.17.6 Batching et fusion

```text
- les tâches d'un même WorkId marquées BATCHABLE soumises dans le même tick
  sont agrégées jusqu'à batch_max (défaut 1024 éléments) ou batch_window (0 tick,
  agrégation intra-tick uniquement)
- grain minimal : une tâche dont le coût estimé est < min_task_ns (défaut 20 µs)
  DOIT être fusionnée ou exécutée en ligne, jamais soumise seule
```

### 5.17.7 Annulation et timeout

```text
CancelToken : atomique partagé, vérifié aux points de contrôle
timeout par tâche : deadline_ns, vérifié par le watchdog
à l'expiration :
   1. cancel coopératif
   2. si non terminé après grace_ns (défaut 2 ms) : la tâche est abandonnée,
      son command buffer est jeté, le workload est démoté d'un niveau
   3. jamais de kill de thread (impossible proprement)
```

### 5.17.8 Inversion de priorité

Le scheduler n'utilise aucun verrou partagé entre priorités différentes dans le chemin d'exécution. Les seules structures partagées sont lock-free (deques, files MPSC, compteurs atomiques). Les allocateurs sont par worker (C-31). Cela élimine par construction l'inversion de priorité classique.

### 5.17.9 Parallélisme imbriqué

```text
- une tâche RF-X PEUT soumettre des sous-tâches ; elles rejoignent le deque local
- profondeur d'imbrication bornée (défaut 4)
- une tâche qui attend ses enfants exécute du travail au lieu de bloquer
  (help-first), garantissant l'absence de deadlock par famine
```

**Failure modes.** FM-23 : worker en panic => voir C-27, le worker est remplacé, la tâche échoue proprement. FM-24 : famine détectée (tâche `CRITICAL` non démarrée après 1 ms) => escalade, exécution en ligne sur le thread appelant. FM-25 : sur-souscription persistante => réduction du pool puis mode `FORCE_SERIAL`.

**Fallback.** `FORCE_SERIAL` : toutes les tâches s'exécutent en ligne sur le thread appelant. Le système reste correct, seulement plus lent.

**Metrics.** `rfx.sched.workers_active`, `rfx.sched.queue_depth`, `rfx.sched.steals`, `rfx.sched.park_ns`, `rfx.sched.task_latency_ns{priority}`, `rfx.sched.deadline_misses`, `rfx.sched.overhead_ns`.

**Tests.** T-260 (correction du work stealing sous stress, 10^7 tâches), T-261 (aucune tâche perdue), T-262 (deadline respectée à 99 % sur `heavy`), T-263 (annulation effective), T-264 (pas de deadlock : test dédié avec parallélisme imbriqué profond), T-265 (adaptation à la sur-souscription simulée), T-266 (mode FORCE_SERIAL produit des résultats identiques), T-267 (overhead de soumission < 200 ns).

**Acceptance.** 10^7 tâches exécutées sans perte ni deadlock ; overhead scheduler < 3 % du temps CPU total du runtime ; aucun `deadline_miss` critique sur profil `medium`.

## 5.18 C-18 : Worker Runtime

**Purpose.** Fournir le contexte d'exécution d'une tâche : allocateur, tampons, accès en lecture seule aux snapshots, production de commandes.

**Contrat d'exécution d'une tâche (IF-13).**

```rust
pub trait Task: Send {
    fn run(&mut self, cx: &mut TaskContext) -> TaskOutcome;
}

pub struct TaskContext<'a> {
    pub arena: &'a mut Arena,          // allocation bump, remise à zéro après la tâche
    pub snapshot: Option<&'a Snapshot>,// lecture seule
    pub commands: &'a mut CommandBuffer,
    pub cancel: &'a CancelToken,
    pub deadline: Instant,
    pub metrics: &'a mut TaskMetrics,
}

pub enum TaskOutcome { Success, Rejected(RejectReason), Timeout, Invalid, Panicked }
```

**Règles absolues pour tout code exécuté dans un worker.**

- R-430 : interdiction d'appeler l'API Minecraft mutante.
- R-431 : interdiction d'appeler la JVM sauf via les points de sortie explicitement autorisés (aucun en V1.0 pour les workers de calcul ; seuls les threads de commit et d'analyse peuvent traverser la frontière).
- R-432 : interdiction d'acquérir un verrou global du runtime.
- R-433 : interdiction d'allouer via l'allocateur système dans le chemin chaud (utiliser l'arène).
- R-434 : toute boucle doit vérifier `cancel` et `deadline` au moins toutes les 10 µs de travail estimé.

**Failure modes.** panic (voir C-27), dépassement d'arène (=> `Rejected(BudgetExceeded)`), dépassement de deadline.

**Metrics.** `rfx.worker.tasks{outcome}`, `rfx.worker.arena_peak_bytes`, `rfx.worker.run_ns`.

**Tests.** T-270 (panic contenue), T-271 (dépassement d'arène propre), T-272 (respect de cancel), T-273 (aucune allocation système dans le chemin chaud, vérifié par allocateur instrumenté).

**Acceptance.** Aucune tâche ne peut corrompre l'état du runtime, quel que soit son échec.

## 5.19 C-19 : Snapshot System

**Purpose.** Fournir aux workers une vue cohérente, en lecture seule et bornée, des données dont ils ont besoin, sans copier le monde.

**Principe.** Un snapshot est **demandé**, **typé** et **borné**. Il n'existe pas de snapshot global.

```rust
pub struct Snapshot {
    pub epoch: Epoch,                 // version de l'état autoritatif au moment de la capture
    pub regions: SmallVec<[Region; 8]>,
    pub layout: SnapshotLayout,       // SoA par type de donnée
    pub data: ArenaSlice,             // mémoire native, immuable
    pub taken_at_tick: u64,
}
```

**Types de snapshot supportés en V1.0.**

| Type | Contenu | Coût typique | Maturité |
|---|---|---|---|
| `PARAMS_ONLY` | valeurs primitives et tableaux copiés des paramètres du workload | O(taille) | STABLE |
| `ENTITY_SET` | SoA : positions, vitesses, boîtes, drapeaux, cibles d'un ensemble d'entités | 64 à 128 octets/entité | STABLE |
| `BLOCK_REGION` | palette + indices d'une boîte de blocs (max 32x32x32 par défaut) | jusqu'à 128 Kio | STABLE |
| `CHUNK_META` | statut, hauteurs, biomes d'un ensemble de chunks | faible | STABLE |
| `CUSTOM` | fourni par un module SDK avec sérialiseur déclaré | variable | EXPERIMENTAL |

**Algorithm.**

```text
Prise de snapshot (uniquement sur le thread autoritatif, en PHASE_PRE) :
 1. vérifier le budget snapshot du tick (défaut 4 Mio/tick, configurable)
 2. allouer dans l'arène de snapshot (double buffer par tick)
 3. copier les données demandées en SoA, sans allocation Java
 4. enregistrer epoch courant et la liste exacte des régions couvertes
 5. publier le snapshot (immuable) aux tâches concernées

Copy-on-write : NON utilisé sur les structures Minecraft (impossible sans
contrôle du moteur). Le CoW s'applique uniquement aux structures internes RF-X
(World Mirror C-20).
```

- R-440 : un snapshot NE DOIT JAMAIS contenir de référence à un objet Java. Uniquement des données valeurs.
- R-441 : un snapshot est invalide dès que `current_epoch != snapshot.epoch` pour une région qu'il couvre. La validation au commit (C-23) DOIT le vérifier.
- R-442 : le coût de snapshot DOIT être compté dans le modèle de décision (C-15) ; si `C_snapshot > G`, la stratégie est refusée.
- R-443 : le budget snapshot dépassé DOIT provoquer le repli du workload sur Java pour ce tick, jamais une attente.

**Failure modes.** FM-26 : entité supprimée pendant la copie => l'entrée est marquée invalide et exclue ; FM-27 : région non chargée => snapshot partiel refusé (pas de lecture de chunk non chargé, qui provoquerait une génération synchrone).

**Metrics.** `rfx.snapshot.bytes{type}`, `rfx.snapshot.take_ns`, `rfx.snapshot.rejected{reason}`, `rfx.snapshot.stale_at_commit`.

**Tests.** T-280 (cohérence : snapshot == état au moment de la capture), T-281 (budget respecté), T-282 (aucune référence Java), T-283 (détection de péremption), T-284 (chunk non chargé jamais forcé).

**Acceptance.** Prise de snapshot `ENTITY_SET` de 2000 entités en moins de 200 µs, aucune allocation Java, aucun chargement de chunk déclenché.

## 5.20 C-20 : World Mirror

**Purpose.** Maintenir, pour les régions chaudes uniquement, une représentation native persistante entre ticks, afin d'éviter de re-snapshoter les mêmes données à chaque tick.

**Maturité : `EXPERIMENTAL` en V1.0.** Désactivé par défaut. Le système est pleinement fonctionnel sans lui (les snapshots suffisent).

**Modèle.**

```text
WorldMirror
 ├── régions suivies (chunks) : au plus mirror.max_chunks (défaut 256)
 ├── par région : palette de blocs, index, version, epoch de dernière sync
 ├── entités miroir : SoA par chunk, version
 └── journal d'invalidation alimenté par les hooks d'écriture Minecraft
```

**Cohérence.**

```text
- le miroir est TOUJOURS dérivé : Minecraft reste la source de vérité
- toute écriture Minecraft observée (via hook de setBlock / d'update d'entité)
  invalide la région correspondante
- une lecture miroir d'une région invalidée est un défaut : la donnée est
  re-synchronisée en PHASE_PRE ou la tâche est refusée
- aucune écriture ne part du miroir vers Minecraft sans passer par C-21/C-22
```

- R-450 : le miroir NE DOIT JAMAIS être utilisé comme source pour une écriture sans revalidation d'epoch au commit.
- R-451 : le budget mémoire du miroir est dur (défaut 128 Mio) ; l'éviction est LRU par chunk.
- R-452 : si le taux d'invalidation d'une région dépasse `mirror.max_invalidation_rate` (défaut 30 % des ticks), la région est retirée du miroir (elle coûte plus qu'elle ne rapporte).

**Failure modes.** FM-28 : écriture Minecraft non observée (mod écrivant par un chemin non hooké) => détection par vérification périodique par sondage aléatoire (`mirror.audit_rate`, défaut 1 région par seconde) ; toute divergence désactive le miroir globalement et journalise `E-2501`.

**Fallback.** Désactivation du miroir, retour aux snapshots par tick.

**Metrics.** `rfx.mirror.chunks`, `rfx.mirror.bytes`, `rfx.mirror.invalidations`, `rfx.mirror.audit_mismatch`, `rfx.mirror.hit_ratio`.

**Tests.** T-290 (audit détecte une divergence injectée), T-291 (budget mémoire respecté), T-292 (éviction correcte), T-293 (aucune écriture directe depuis le miroir), T-294 (désactivation propre en cours de partie).

**Acceptance.** Sur 2 heures de jeu, `audit_mismatch == 0` ou désactivation automatique effective ; gain mesurable sur les workloads de lecture répétée, sinon le miroir reste désactivé.

## 5.21 C-21 : Command Buffer

**Purpose.** Permettre aux workers de produire des effets sans jamais toucher l'état autoritatif.

**Modèle.**

```rust
pub struct CommandBuffer {
    pub owner_task: TaskId,
    pub work: WorkId,
    pub epoch: Epoch,                 // epoch du snapshot utilisé
    pub read_set: AccessSet,          // ce que la tâche a effectivement lu
    pub commands: Vec<Command>,       // DM-16, ordonnées
    pub order_key: OrderKey,          // pour restaurer un ordre déterministe
    pub state: BufferState,           // OPEN | SEALED | VALIDATED | APPLIED | DISCARDED
}
```

**Règles.**

- R-460 : un command buffer est **append-only** puis scellé. Aucune modification après `SEALED`.
- R-461 : `order_key` DOIT être dérivé d'une clé stable (index d'entité, position de bloc, index de batch), jamais de l'ordre d'achèvement des tâches. C'est ce qui rend le commit déterministe malgré l'exécution parallèle.
- R-462 : la taille d'un buffer est bornée (`commit.max_commands_per_buffer`, défaut 4096). Dépassement => `Rejected(BudgetExceeded)`, repli Java.
- R-463 : chaque buffer DOIT enregistrer son read set réel pour permettre la détection de conflit (C-23).

**Metrics.** `rfx.cmdbuf.commands{type}`, `rfx.cmdbuf.bytes`, `rfx.cmdbuf.discarded{reason}`.

**Tests.** T-300 (append-only respecté), T-301 (ordre déterministe indépendant de l'ordre d'achèvement, vérifié sur 1000 exécutions), T-302 (borne respectée).

**Acceptance.** Deux exécutions du même scénario produisent des séquences de commandes identiques bit à bit.

## 5.22 C-22 : Commit Engine

**Purpose.** Appliquer les effets produits hors du thread autoritatif, dans l'ordre, après validation, sans jamais laisser d'effet partiel.

**Où.** Exclusivement sur le thread autoritatif, pendant `PHASE_DRAIN`.

**Algorithm (protocole de commit, normatif).**

```text
1. COLLECTE
     rassembler tous les CommandBuffer SEALED du tick
2. TRI
     trier par (priority, order_key) : ordre total déterministe
3. VALIDATION (C-23)
     pour chaque buffer :
        si buffer.epoch != epoch_courant_des_regions_lues -> DISCARD (stale)
        si read_set du buffer intersecte les writes déjà appliqués ce tick
           -> DISCARD (conflit)
        si une commande est invalide (bloc hors monde, entité disparue)
           -> DISCARD du buffer entier
4. APPLICATION
     pour chaque commande, appeler l'applicateur enregistré
     l'application est ATOMIQUE PAR BUFFER : si une commande échoue,
     les commandes déjà appliquées de ce buffer sont compensées si possible,
     sinon l'incident est fatal pour ce workload : démotion définitive + E-2601
5. PUBLICATION
     mettre à jour les epochs des régions écrites
     notifier C-34 (mesure) et C-24 (validation)
```

- R-470 : le budget de commit par tick DOIT être borné (`commit.max_ns_per_tick`, défaut 2 ms). Les buffers non appliqués dans le budget sont rejetés, pas reportés (le report créerait des effets décalés d'un tick, donc observables).
- R-471 : aucune commande NE DOIT provoquer un chargement de chunk synchrone. Une commande visant un chunk déchargé est rejetée.
- R-472 : les commandes DOIVENT être appliquées via les API publiques du jeu (ex : `Level.setBlock` avec les bons flags) pour préserver les notifications, les événements et la cohérence client/serveur.
- R-473 : la compensation (rollback intra-buffer) DOIT être définie pour chaque type de commande, ou le type de commande DOIT être déclaré non compensable et alors interdit dans un buffer contenant plus d'une commande à effet.

**Failure modes.** FM-29 : dépassement du budget => rejet et démotion. FM-30 : échec d'application non compensable => `JAVA_ONLY` définitif pour le `WorkId` + alerte. FM-31 : exception d'un applicateur => capture, rejet du buffer, aucun effet partiel visible au-delà de la compensation.

**Metrics.** `rfx.commit.buffers{outcome}`, `rfx.commit.commands_applied`, `rfx.commit.ns`, `rfx.commit.discard{reason}`, `rfx.commit.budget_exceeded`.

**Tests.** T-310 (déterminisme du commit), T-311 (stale rejeté), T-312 (conflit rejeté), T-313 (atomicité par buffer), T-314 (budget respecté), T-315 (aucun chargement de chunk provoqué), T-316 (round-trip de sauvegarde identique avec et sans RF-X sur scénario scripté).

**Acceptance.** Sur 4 heures de jeu en profil `heavy`, zéro incohérence détectée, sauvegarde chargeable par un Minecraft vanilla+Forge sans RF-X.

## 5.23 C-23 : Conflict Detection

**Purpose.** Détecter, avant application, toute violation de cohérence entre ce qu'une tâche a lu et ce que le monde est devenu.

**Mécanisme : versionnement par epoch et par région.**

```text
epoch global    : incrémenté à chaque tick
epoch de région : incrémenté à chaque écriture appliquée sur la région
                  (granularité : chunk pour les blocs, entité pour les entités,
                   domaine entier pour LevelGlobals)
```

**Matrice de conflits.**

| Tâche A / Tâche B | READ | WRITE |
|---|---|---|
| READ | compatible | compatible si B est appliquée après A et A n'observe pas B |
| WRITE | conflit si A lit ce que B écrit dans le même tick | conflit systématique |

Règles opérationnelles :

```text
READ/READ    : jamais de conflit
READ/WRITE   : conflit si le lecteur a produit des commandes dépendant de la valeur lue
               ET que l'écrivain est ordonné avant lui au commit
WRITE/WRITE  : conflit toujours ; le buffer de plus faible order_key gagne,
               l'autre est rejeté
STALE        : epoch de région lue != epoch courant -> rejet
```

- R-480 : la détection DOIT être conservatrice : en cas de doute sur la granularité, considérer le domaine entier.
- R-481 : le coût de détection DOIT être O(nb_buffers * taille_moyenne_read_set) avec des bitsets ; plafonné par `commit.max_conflict_checks`.

**Metrics.** `rfx.conflict.detected{kind}`, `rfx.conflict.check_ns`.

**Tests.** T-320 (WRITE/WRITE toujours détecté), T-321 (stale détecté), T-322 (aucun faux négatif sur suite synthétique de 10 000 cas), T-323 (coût borné).

**Acceptance.** Zéro faux négatif sur la suite synthétique ; taux de faux positifs inférieur à 5 % sur le profil `heavy` (au-delà, les décisions sont revues car l'offload devient inutile).

## 5.24 C-24 : Validation Engine

**Purpose.** Décider si une stratégie candidate est **correcte** et **rentable**, avant de l'activer, et surveiller en continu les stratégies actives.

**Pipeline de promotion (SM-03).**

```text
CANDIDATE
   | analyse de sûreté (C-14) OK
   v
SHADOW            : exécution non autoritative, N runs (défaut 64)
   | 100 % de correspondance ET gain estimé confirmé
   v
CANARY            : stratégie active mais surveillée, échantillon de trafic
   |                (défaut 10 % des invocations pendant 300 ticks)
   | aucun mismatch, aucun timeout, gain mesuré > 0
   v
ACTIVE            : stratégie pleinement active
   | anomalie
   v
SUSPENDED -> JAVA_FALLBACK (démotion, C-26)
```

**Oracle de correction.**

```text
Comparaison de sortie :
  - valeurs de retour : égalité exacte pour les entiers, égalité bit à bit pour
    les flottants sauf si tolerance déclarée (interdite par défaut)
  - command buffers : égalité de la séquence après tri par order_key
  - mutations d'état : comparaison des régions écrites (hash des régions)
  - exceptions : même type et même condition de levée
  - ordre observable : même séquence d'événements émis
Normalisation autorisée :
  - identifiants d'objets internes RF-X
  - horodatages internes RF-X
  Rien d'autre. Toute autre normalisation est une faute.
```

- R-490 : un mismatch, même unique, DOIT être définitif pour le fingerprint courant : la stratégie est bannie et l'incident est journalisé avec le dump des entrées (C-35).
- R-491 : la validation NE DOIT PAS être exécutable pour un workload à effets de bord non isolables (I-05) : dans ce cas la seule promotion possible est TL<=2.
- R-492 : le gain doit être mesuré, pas estimé, pour passer de `CANARY` à `ACTIVE`.

**Metrics.** `rfx.validate.shadow_runs`, `rfx.validate.mismatches`, `rfx.validate.promotions`, `rfx.validate.demotions`, `rfx.validate.canary_traffic_pct`.

**Tests.** T-330 (mismatch injecté => bannissement), T-331 (pas de promotion sans gain mesuré), T-332 (comparaison flottante stricte), T-333 (exception comparée), T-334 (canary limité en trafic).

**Acceptance.** Aucune stratégie n'atteint `ACTIVE` sans 64 shadow runs conformes et un gain mesuré positif.

## 5.25 C-25 : Shadow Execution

**Purpose.** Exécuter une stratégie candidate sans effet, pour apprendre et valider.

**Contraintes.**

```text
- autorisée uniquement si : PURE, ou exécution sur snapshot isolé avec
  command buffer systématiquement DISCARDED
- priorité LOW : jamais au détriment du jeu
- budget : shadow.max_cpu_pct (défaut 5 % d'un cœur)
- désactivée automatiquement si MSPT > tick_budget * 0.9
```

**Algorithm.**

```text
1. le chemin Java s'exécute normalement et produit le résultat autoritatif
2. les entrées (snapshot + paramètres) sont capturées
3. la tâche shadow est soumise en LOW
4. à l'achèvement, C-24 compare
5. le command buffer shadow est TOUJOURS jeté
6. si la tâche shadow dépasse sa deadline, elle est annulée sans conséquence
```

- R-500 : la shadow execution NE DOIT JAMAIS augmenter le MSPT de plus de 1 % en moyenne ; le watchdog la désactive sinon.
- R-501 : la capture des entrées ne doit pas doubler le coût du chemin Java : si `C_snapshot > 20 %` du coût du workload, la shadow execution est refusée pour ce workload.

**Metrics.** `rfx.shadow.runs`, `rfx.shadow.cpu_pct`, `rfx.shadow.cancelled`, `rfx.shadow.mismatch`.

**Tests.** T-340 (aucun effet observable), T-341 (budget respecté), T-342 (désactivation sous charge).

**Acceptance.** Le jeu se comporte de façon identique, mesurée, avec shadow execution activée ou non (différence de MSPT < 1 %).

## 5.26 C-26 : Rollback

**Purpose.** Ramener un workload à un état d'exécution sûr, immédiatement et sans perte de continuité de jeu.

**Déclencheurs.**

```text
- mismatch de validation
- exception ou panic dans le chemin optimisé
- timeout répété (défaut 3 en 100 ticks)
- conflit de commit répété (défaut 5 % des buffers rejetés)
- régression de performance mesurée (gain mesuré < 0 sur 300 ticks)
- corruption suspectée (audit du miroir, invariant violé)
- demande utilisateur ou changement de configuration
```

**Algorithm.**

```text
1. marquer la décision comme révoquée (atomique, visible au prochain point de
   soumission ; jamais au milieu d'une tâche en cours)
2. annuler les tâches en vol de ce WorkId
3. jeter leurs command buffers
4. restaurer la stratégie précédente sûre (au minimum JAVA_ONLY)
5. enregistrer l'incident (C-09 history, C-35 diagnostics)
6. appliquer une pénalité durable dans le modèle (C-44)
7. si le motif est correctness : bannir la stratégie pour ce fingerprint
```

- R-510 : le rollback DOIT être effectif en au plus 1 tick.
- R-511 : le rollback NE DOIT PAS annuler des effets déjà commités et validés (ils sont, par construction, corrects) ; il empêche les effets futurs.
- R-512 : trois rollbacks du même `WorkId` pour des motifs de correction => `JAVA_ONLY` permanent jusqu'à changement de fingerprint.

**Metrics.** `rfx.rollback.count{reason}`, `rfx.rollback.latency_ticks`, `rfx.rollback.permanent_bans`.

**Tests.** T-350 (rollback en 1 tick), T-351 (tâches en vol annulées), T-352 (bannissement après 3 incidents), T-353 (jeu jouable pendant un rollback massif de 200 workloads).

**Acceptance.** Un rollback global (toutes stratégies) s'effectue sans hoquet visible : pas plus de 2 ticks au-delà du budget.

## 5.27 C-27 : Rust Runtime Core

**Purpose.** Héberger l'état global du runtime natif, les points d'entrée FFI, la gestion des erreurs et l'isolation des pannes.

**État global.**

```rust
pub struct Runtime {
    pub config: Config,
    pub hw: HardwareClass,
    pub store: WorkloadStore,
    pub scheduler: Scheduler,
    pub decision: DecisionEngine,
    pub telemetry: Telemetry,
    pub caches: Caches,
    pub state: RuntimeState,   // INIT | RUNNING | DEGRADED | HALTED
}
```

- R-520 : une seule instance de `Runtime` par processus. Double initialisation => `E-1004`.
- R-521 : le handle exposé à Java est un entier opaque validé à chaque appel (pas de pointeur brut transmis sans vérification de génération).

**Gestion des panics (RISK-03).**

```rust
// Tout point d'entrée FFI :
#[no_mangle]
pub extern "C" fn rfx_xxx(...) -> i32 {
    match std::panic::catch_unwind(AssertUnwindSafe(|| { /* ... */ })) {
        Ok(code) => code,
        Err(_) => { record_panic(); E_PANIC }
    }
}
```

- R-522 : `panic = "unwind"` est OBLIGATOIRE pour la bibliothèque (pas `abort`), sinon un panic tue la JVM.
- R-523 : tout panic DOIT être compté, journalisé avec contexte, et provoquer la désactivation du sous-système concerné après `panic_threshold` (défaut 3).
- R-524 : un panic dans un worker NE DOIT PAS empoisonner le pool : le worker est recyclé, la tâche renvoie `Panicked`.

**Halt d'urgence.**

```text
Conditions : plus de 10 panics en 60 s, corruption détectée, invariant violé
Action : RuntimeState = HALTED
         - toutes les décisions -> JAVA_ONLY
         - workers arrêtés après drain
         - profiler arrêté
         - le jeu continue en Java pur
         - un rapport est écrit dans crash/
```

**Metrics.** `rfx.runtime.state`, `rfx.runtime.panics`, `rfx.runtime.halt`, `rfx.runtime.native_rss_bytes`.

**Tests.** T-360 (panic capturée en FFI), T-361 (worker recyclé), T-362 (halt d'urgence laisse le jeu jouable), T-363 (double init refusée), T-364 (handle invalide rejeté).

**Acceptance.** Aucune panic Rust ne peut faire tomber la JVM ; test d'injection de panic à chaque point d'entrée.

## 5.28 C-31 : Memory Manager

**Purpose.** Fournir une gestion mémoire native prévisible, bornée et sans contention.

**Structures.**

| Allocateur | Usage | Politique |
|---|---|---|
| Arena par worker | données temporaires d'une tâche | bump, reset après la tâche, capacité initiale 1 Mio, croissance x2 jusqu'à `arena_max` (défaut 16 Mio) |
| Arena de snapshot | double buffer par tick | reset en fin de tick |
| Slab de descripteurs | `WorkloadDescriptor` | taille fixe, index dense |
| Pool de buffers | command buffers, tampons FFI | free-list par taille, réutilisation |
| Ring buffers | flux de profilage | taille fixe, écrasement des plus anciens si saturé |
| Allocateur global | structures longue durée | allocateur système, jamais dans le chemin chaud |

**Règles.**

- R-530 : aucune allocation système dans une tâche de calcul (R-433).
- R-531 : `false sharing` évité : toute structure partagée entre workers est alignée sur 64 octets (`#[repr(align(64))]`).
- R-532 : disposition SoA obligatoire pour les données traitées en lot (entités, blocs).
- R-533 : budget mémoire natif total borné (`memory.max_native_mb`, défaut 512). Dépassement => refus de nouvelles allocations non critiques, éviction des caches, puis `DEGRADED`.
- R-534 : la mémoire native DOIT être rapportée dans les diagnostics et comparée au RSS pour détecter les fuites (RISK-10).

**Metrics.** `rfx.mem.native_bytes{pool}`, `rfx.mem.arena_peak`, `rfx.mem.evictions`, `rfx.mem.alloc_failures`.

**Tests.** T-370 (aucune fuite sur 10^6 cycles de tâches), T-371 (budget respecté), T-372 (alignement vérifié), T-373 (croissance et reset d'arène corrects).

**Acceptance.** RSS natif stable (dérive < 1 % par heure) sur test long T-700.

## 5.29 C-32 : Cache

**Purpose.** Éviter de refaire ce qui a déjà été fait : analyses, décisions, résultats de calculs purs, artefacts compilés, résultats de compatibilité, mesures.

**Les cinq caches normatifs.**

| Cache | Clé | Valeur | Invalidation | Éviction | Taille par défaut |
|---|---|---|---|---|---|
| `workload_cache` | `WorkId + Fingerprint` | descripteur + faits d'analyse | changement de fingerprint | LRU | 64 Mio |
| `optimization_cache` | `WorkId + Fingerprint + mode` | décision + gain mesuré | changement de fingerprint, mode, matériel | LRU | 16 Mio |
| `compiled_cache` | `hash(IR) + target ISA + rfx_version` | code natif compilé | changement d'IR, de version, d'ISA | LRU + taille | 128 Mio |
| `compatibility_cache` | `mod_set_hash + owner_mod_hash` | verdicts de compatibilité | changement du set de mods | jamais évincé tant que valide | 8 Mio |
| `benchmark_cache` | `WorkId + Fingerprint + strategy` | mesures agrégées | changement de fingerprint | LRU | 16 Mio |
| `result_cache` (mémoïsation) | `WorkId + hash(entrées normalisées)` | sortie | toute écriture dans un domaine lu | LRU + TTL en ticks | 32 Mio |

**Règles.**

- R-540 : `result_cache` (mémoïsation d'un calcul) est autorisé UNIQUEMENT pour `PURE && DETERMINISTIC && CACHEABLE`, avec une clé couvrant **toutes** les entrées lues. Si le read set est `UNKNOWN` ou `OVER_APPROX`, la mémoïsation est interdite.
- R-541 : toute entrée DOIT porter son fingerprint ; une entrée sans fingerprint valide est ignorée et purgée.
- R-542 : détection de corruption par CRC32C par entrée ; entrée corrompue => purge silencieuse et compteur.
- R-543 : les caches DOIVENT être bornés en taille ET en nombre d'entrées, avec éviction LRU et un plancher de rétention pour les entrées `HOT`.
- R-544 : le taux de succès de chaque cache DOIT être mesuré ; un cache dont le taux de succès est inférieur à 5 % pendant 30 minutes est désactivé automatiquement (il coûte plus qu'il ne rapporte).

**Metrics.** `rfx.cache.hits{cache}`, `rfx.cache.misses{cache}`, `rfx.cache.evictions{cache}`, `rfx.cache.bytes{cache}`, `rfx.cache.corrupt`.

**Tests.** T-380 (invalidation par fingerprint), T-381 (mémoïsation interdite si non pure), T-382 (corruption détectée), T-383 (éviction sous pression), T-384 (démarrage à froid vs à chaud : gain de temps d'analyse mesuré).

**Acceptance.** Second démarrage avec le même modpack : au moins 80 % des décisions restaurées depuis le cache, temps avant première optimisation active divisé par au moins 5.

## 5.30 C-28 / C-29 / C-30 : RF-IR, Optimizer, Compiler Backend

Ces trois composants forment la chaîne de transformation Java vers natif. Ils sont spécifiés en détail en **PARTIE 10**. Leur fiche synthétique :

| Composant | Purpose | Maturité | Fallback |
|---|---|---|---|
| C-28 RF-IR | représentation intermédiaire vérifiable d'un sous-ensemble déclaré de bytecode | EXPERIMENTAL | aucun IR produit => TL <= 4 |
| C-29 IR Optimizer | passes d'optimisation préservant la sémantique de l'IR | EXPERIMENTAL | IR non optimisé, exécuté par l'interpréteur IR |
| C-30 Compiler Backend | génération de code natif (Cranelift) à partir de l'IR optimisé | EXPERIMENTAL, désactivé par défaut (`feature = "jit"`) | interpréteur IR, puis Java |

Invariant commun INV-04 : **aucune méthode ne peut être exécutée via C-28/29/30 sans avoir passé la validation shadow complète (C-24) et sans que l'interpréteur IR et le chemin Java aient produit des résultats identiques sur au moins 64 exécutions.**

## 5.31 C-33 : Compatibility Engine

**Purpose.** Produire, pour chaque workload et chaque propriétaire, un verdict de compatibilité fondé sur des **propriétés et des fingerprints**, jamais sur des noms de mods.

**Verdicts.**

```text
ALLOW           aucune restriction
FORCE_JAVA      exécution Java uniquement (TL0)
FORCE_SERIAL    pas de parallélisme (TL <= 2)
FORCE_BATCH     batching autorisé, offload interdit
FORCE_PARALLEL  parallélisme autorisé mais offload Rust interdit
DISABLE         RF-X n'observe ni ne transforme ce workload
```

**Sources de verdict (par ordre de priorité décroissante).**

```text
1. configuration utilisateur explicite (par WorkId ou par owner)
2. quarantaine automatique issue d'un incident (rollback, panic, mismatch)
3. propriétés détectées :
     - présence d'un autre transformateur de bytecode sur la même classe
     - usage massif de réflexion dans le workload
     - présence de threads propriétaires du mod touchant les mêmes domaines
     - version de classe non supportée
4. règles dérivées apprises (C-44) : profils de workloads ayant échoué
5. défaut : ALLOW
```

- R-550 : une règle de compatibilité DOIT être exprimable sans nommer un mod. Un test statique (T-245) vérifie l'absence de littéral de nom de mod dans le code du moteur. La configuration **utilisateur** peut, elle, cibler un mod : ce n'est pas le moteur qui décide.
- R-551 : toute quarantaine DOIT porter une date, un motif, un fingerprint et une condition de levée (changement de fingerprint, action utilisateur).
- R-552 : la détection d'un autre transformateur de bytecode sur une classe DOIT au minimum imposer `FORCE_SERIAL` sur les workloads de cette classe tant que la stabilité n'est pas démontrée.

**Metrics.** `rfx.compat.verdicts{verdict}`, `rfx.compat.quarantines`, `rfx.compat.conflicting_transformers`.

**Tests.** T-390 (priorité des sources respectée), T-391 (quarantaine levée au changement de fingerprint), T-392 (aucun nom de mod dans le moteur).

**Acceptance.** Le système reste stable en présence d'un mod de transformation concurrent simulé (T-8xx).

## 5.32 C-34 : Telemetry

**Purpose.** Mesurer tout ce que le système décide, fait et coûte, en local, sans transmission réseau.

**Modèle de métriques.**

```text
Types  : counter, gauge, histogram (buckets log2), ratio
Nommage: rfx.<domaine>.<mesure>{labels}
Labels : cardinalité bornée (jamais un WorkId brut en label de métrique agrégée)
Export : en mémoire (dashboard, commandes), fichier JSON à la demande,
         journal de tick optionnel (mode DEBUG)
```

- R-560 : aucune télémétrie externe. Aucune requête réseau sortante. Ce point est vérifié par un test de build (`no-net`) et documenté dans `SECURITY.md`.
- R-561 : le coût de la télémétrie DOIT être inférieur à 0,2 % du MSPT.
- R-562 : toute métrique DOIT être documentée dans `BENCHMARKS.md` (nom, unité, sémantique).

**Tests.** T-400 (aucune socket ouverte par RF-X), T-401 (coût mesuré), T-402 (cardinalité bornée).

## 5.33 C-35 : Diagnostics

**Purpose.** Répondre, à tout moment, aux questions d'observabilité du V0.3 §54 : pourquoi ce workload existe, pourquoi il est lent, pourquoi il est sur ce thread, pourquoi il n'est pas parallélisé, pourquoi il n'est pas envoyé à Rust, pourquoi l'optimisation a été refusée.

**Sorties.**

```text
- commande in-game / console : /rfx why <workid|selector>
- rapport texte : reports/rfx-report-<ts>.txt
- rapport machine : reports/rfx-report-<ts>.json
- dump d'incident : crash/rfx-crash-<ts>.json (entrées, descripteur, décision, trace)
```

**Format d'explication (exemple normatif de rendu).**

```text
WORKID 0x1827A44C
Owner            : <modid>
Method           : com.example.Machine#tickMachine(Lnet/minecraft/world/level/Level;)V
Context          : ServerTick -> BlockEntityTicker -> tickMachine   (depth 3)
Side             : SERVER

MEASURED
  calls/tick     : 412
  cpu p50/p95    : 14.2 us / 51.8 us
  share of MSPT  : 8.1 %
  alloc          : 2.1 MB/s
  heat           : CRITICAL

ANALYSIS
  reads          : ChunkData(3 chunks), BlockEntityState, ModStatics  [EXACT]
  writes         : BlockEntityState                                   [EXACT]
  side effects   : schedules_tasks=true, fires_events=false
  thread         : main_thread_required = YES (writes authoritative state)
  determinism    : DETERMINISTIC (verified 64 runs)
  flags          : CPU_INTENSIVE | STATEFUL | WRITE | HOT

DECISION
  strategy       : JAVA_BATCH (TL2)
  rejected       : RUST_OFFLOAD  -> ThreadAffinity
                   RUST_PARALLEL -> ThreadAffinity, OrderDependency
  reasons        :
     - writes BlockEntityState which is authoritative and not snapshot-backed
     - order relative to neighbouring block entities is observable
  alternative    : batching of 412 calls into 4 batches
  expected gain  : 1.9 ms/tick
  measured gain  : 1.6 ms/tick (last 300 ticks)
  confidence     : static 0.82 / dynamic 0.94 / validation 1.00 -> 0.82
  revisit        : in 600 ticks
```

- R-570 : toute décision active DOIT pouvoir produire cette fiche en moins de 50 ms, sans figer le jeu.
- R-571 : aucune information personnelle ni chemin utilisateur complet ne doit apparaître dans les rapports exportables (anonymisation du chemin de jeu).

**Tests.** T-410 (fiche complète pour toute décision), T-411 (dump d'incident exploitable pour reproduction), T-412 (anonymisation).

**Acceptance.** Pour 20 workloads pris au hasard, la fiche explique la décision sans mention "inconnu" non justifiée.

## 5.34 C-36 : Benchmark Harness

Spécifié en détail en **PARTIE 21**. Fiche synthétique :

**Purpose.** Mesurer, de façon reproductible, l'impact de RUSTFORGE-X, et détecter les régressions en CI.

**Deux niveaux.**

```text
A. micro-benchmarks Rust (criterion) : scheduler, snapshot, conflit, IR, mémoire
B. macro-benchmarks in-game (serveur dédié scripté, client scripté) :
   MSPT, TPS, FPS, frametime, 1 % low, allocations, latence de chunk
```

- R-580 : aucun résultat de benchmark ne DOIT être écrit à la main dans la documentation. Tous les chiffres publiés proviennent d'un fichier `benchmarks/results/*.json` généré.
- R-581 : un benchmark sans intervalle de confiance ni nombre de répétitions est invalide.

## 5.35 C-37 : Configuration

Spécifié en détail en **PARTIE 28**.

**Purpose.** Exposer un contrôle complet, sûr par défaut, avec rechargement à chaud borné.

- R-590 : toute option DOIT avoir un défaut sûr, une plage validée et une description.
- R-591 : une option inconnue dans le fichier DOIT être conservée et signalée, jamais supprimée silencieusement.
- R-592 : le rechargement à chaud est autorisé pour les seuils et budgets ; il est interdit pour les options structurelles (nombre de workers au-delà du maximum initial, activation du JIT), qui exigent un redémarrage.

## 5.36 C-38 : CLI et commandes

**Purpose.** Piloter et interroger le runtime depuis le jeu, la console serveur et hors ligne.

**Commandes in-game (permission niveau 3 sur serveur).**

```text
/rfx status                          état global, mode, budgets, overhead
/rfx top [n]                         n workloads les plus coûteux
/rfx why <workid>                    fiche d'explication (C-35)
/rfx mods                            tableau par mod : CPU, workloads chauds, actifs
/rfx workload <workid>               détail complet
/rfx mode <safe|balanced|performance|experimental|debug>
/rfx set <key> <value>               option rechargeable à chaud
/rfx disable <workid|owner>          force JAVA_ONLY
/rfx enable <workid|owner>
/rfx bench <profile> [ticks]         lance un benchmark interne
/rfx validate <workid>               force un cycle de validation
/rfx cache <stats|purge|purge <cache>>
/rfx report                          génère un rapport dans reports/
/rfx panic-test                      (DEBUG uniquement) vérifie le confinement
```

**CLI hors ligne (`rfx-cli`).**

```text
rfx-cli inspect <workload.db>        liste et filtre les workloads
rfx-cli explain <workload.db> <id>   fiche d'explication hors jeu
rfx-cli bench-report <results/*.json> compare deux séries, calcule les écarts
rfx-cli verify-jar <jar>             vérifie natifs, hashes, métadonnées
```

- R-600 : toute commande mutante DOIT être journalisée avec l'auteur et l'horodatage.
- R-601 : les commandes NE DOIVENT PAS bloquer le thread serveur plus de 5 ms ; les traitements longs sont asynchrones avec message de fin.

**Tests.** T-420 (chaque commande répond), T-421 (aucune commande ne dépasse 5 ms sur le thread serveur), T-422 (permissions respectées).

## 5.37 C-39 : Rust Mod SDK

**Purpose.** Permettre d'écrire des systèmes natifs par-dessus le runtime, sans passer par la découverte automatique. C'est le chemin "Rust-native" du V0.3 §40, requalifié `EXPERIMENTAL`.

**Modèle de programmation.**

```rust
use rfx_sdk::prelude::*;

#[rfx_system(
    name = "example_entity_pass",
    stage = Stage::ServerTickPre,
    reads  = [Domain::EntityState],
    writes = [Domain::EntityState],
    determinism = Determinism::Deterministic,
    parallel = Parallel::ByChunk,
)]
fn tick_entities(ctx: &mut SystemContext) -> SystemResult {
    for chunk in ctx.entities().chunks() {          // vue SoA en lecture seule
        for e in chunk.iter() {
            if e.needs_update() {
                ctx.commands().update_entity(e.key(), EntityPatch::velocity(/* ... */));
            }
        }
    }
    Ok(())
}
```

**Contrat du SDK.**

- R-610 : un système SDK DOIT déclarer statiquement ses read/write sets, son déterminisme et son mode de parallélisme. Ces déclarations sont **vérifiées à l'exécution** par échantillonnage : toute violation désactive le système (`E-2701`).
- R-611 : un système SDK n'a accès qu'aux snapshots et aux command buffers. Aucun accès direct à la JVM.
- R-612 : le SDK expose : monde (lecture via snapshot/mirror), chunks, entités, événements (abonnement en lecture), networking (émission via commande), scheduler (sous-tâches), ressources, mémoire (arène), tâches.
- R-613 : la compatibilité binaire du SDK est garantie à l'intérieur d'une version mineure ; les systèmes SDK sont compilés dans le même artefact que le runtime en V1.0 (pas de chargement dynamique de plugins natifs tiers, pour des raisons de sûreté et d'ABI).

**Tests.** T-430 (violation de déclaration détectée), T-431 (système SDK d'exemple produit un résultat identique à une implémentation Java de référence), T-432 (aucun accès JVM depuis un système SDK).

**Acceptance.** Un système d'exemple complet (pass entités) est fourni, testé, benchmarké et documenté dans `RUST_MOD_SDK.md`.

## 5.38 C-40 : Release System

Spécifié en détail en **PARTIE 25**. Fiche : produit l'artefact reproductible, les binaires natifs, les sommes SHA-256, le changelog et les métadonnées, à partir d'un dépôt propre et d'un tag.

## 5.39 C-41 : Mod Discovery

**Purpose.** Découvrir automatiquement les mods, leurs classes, leurs méthodes, leurs handlers, leurs threads et leurs hotspots, sans intégration manuelle.

**Algorithm.**

```text
1. À LOAD_COMPLETE, énumérer les conteneurs de mods via l'API du loader (C-01) :
      modid, version, chemin du jar, classe principale
2. Calculer owner_mod_hash = sha256 tronqué du jar (ou du répertoire pour un mod
   en développement)
3. Construire la table classloader -> modid pour attribuer un propriétaire à toute
   classe transformée (attribution par chargeur, puis par package, puis "unknown")
4. Ne PAS scanner exhaustivement les classes au démarrage (coût prohibitif).
   L'indexation est paresseuse : une classe est indexée quand elle est
   effectivement chargée ou sondée.
5. Enregistrer les threads créés dont le nom ou le créateur appartient à un mod
   (via un hook sur la création de thread quand disponible, sinon par
   échantillonnage des piles) -> alimente RISK-05.
6. Détecter les mods sans identifiant clair -> owner = "unknown", traité comme
   un propriétaire unique et conservatif.
```

- R-620 : la découverte NE DOIT PAS charger de classes qui ne le seraient pas autrement (pas de scan forcé), afin de ne pas modifier le comportement du jeu ni allonger le démarrage.
- R-621 : le temps total de découverte DOIT être inférieur à 500 ms pour 250 mods.

**Metrics.** `rfx.discovery.mods`, `rfx.discovery.classes_indexed`, `rfx.discovery.unknown_owner_ratio`, `rfx.discovery.duration_ms`.

**Tests.** T-440 (attribution correcte de propriétaire sur un jeu de tests), T-441 (aucune classe chargée en plus), T-442 (durée respectée à 250 mods synthétiques).

**Acceptance.** Sur profil `very heavy`, tous les mods sont inventoriés, moins de 5 % des classes chaudes ont `owner = unknown`.

## 5.40 C-42 : Network Subsystem

**Purpose.** Réduire le coût CPU du traitement réseau **sans modifier le protocole**.

**Maturité : `EXPERIMENTAL`.**

**Périmètre autorisé.**

```text
AUTORISÉ
  - déplacement hors thread principal de la sérialisation/désérialisation
    de charges utiles indépendantes, quand l'ordre d'émission est préservé
  - compression/décompression conformes au format existant, exécutées en parallèle
  - regroupement des opérations internes (préparation, copies) sans changer
    le nombre ni le contenu des paquets émis
INTERDIT
  - fusionner, réordonner, supprimer ou ajouter des paquets
  - changer le format de compression ou ses paramètres négociés
  - introduire un protocole propriétaire sans négociation explicite et repli
```

- R-630 : toute transformation réseau DOIT être validée par comparaison **bit à bit** du flux produit avec le flux de référence (T-450).
- R-631 : la compatibilité DOIT être garantie dans les trois configurations : client vanilla / serveur RF-X, client RF-X / serveur vanilla, client RF-X / serveur RF-X.

**Tests.** T-450 (égalité bit à bit du flux), T-451 (les trois configurations se connectent et jouent), T-452 (ordre d'émission préservé sous charge).

**Acceptance.** Aucune déconnexion ni désynchronisation sur 1 heure de test à 20 joueurs simulés.

## 5.41 C-43 : Explainability Engine

**Purpose.** Enregistrer la chaîne causale de chaque décision et la restituer (C-35).

**Modèle.**

```rust
pub struct Reason {
    pub code: ReasonCode,          // énuméré, stable, traduisible
    pub subject: Subject,          // Domain | Region | MethodRef | Rule | Metric
    pub weight: f32,               // contribution au score ou au blocage
    pub evidence: Evidence,        // mesure, fait statique, incident historique
}
```

- R-640 : chaque décision DOIT contenir au moins un `Reason` par contrainte bloquante et les trois plus fortes contributions positives et négatives au score.
- R-641 : les codes de raison DOIVENT être stables entre versions (ils sont utilisés par les tests et par l'UI).

**Tests.** T-460 (toute décision a des raisons), T-461 (stabilité des codes vérifiée par test de snapshot).

## 5.42 C-44 : Learning / Auto-tuner

**Purpose.** Améliorer les estimations et les paramètres à partir des mesures réelles, localement, de façon versionnée et réversible.

**Ce que le système apprend.**

```text
1. speedup réel par (profil de workload, stratégie, classe de matériel)
2. coûts unitaires : appel FFI, soumission de tâche, copie mémoire, barrière
3. paramètres optimaux : taille de batch, nombre de workers, grain de découpage
4. fiabilité : probabilité de mismatch ou de rollback selon le profil
```

**Modèle.** Un modèle **simple, inspectable et borné** est obligatoire en V1.0 :

```text
- régression linéaire par morceaux sur des caractéristiques normalisées
  (coût CPU, taille du read set, nombre d'appels, taille de payload, cœurs)
- ou moyenne bayésienne par bucket (profil x stratégie x classe matérielle)
Aucun modèle opaque, aucun réseau de neurones, aucune dépendance externe.
Le modèle DOIT être exportable en JSON et lisible par un humain.
```

**Boucle.**

```text
OBSERVE -> ANALYZE -> CLASSIFY -> DECIDE -> EXTRACT -> OPTIMIZE
        -> VALIDATE -> EXECUTE -> MEASURE -> LEARN -> (retour à DECIDE)
```

**Exploration.** Une part bornée du trafic (`learning.exploration_pct`, défaut 2 %, uniquement en mode BALANCED ou plus) peut être consacrée à tester une stratégie alternative en canary, afin d'éviter de rester bloqué sur un optimum local. L'exploration est interdite en mode SAFE.

- R-650 : l'apprentissage DOIT être local (aucune donnée ne quitte la machine).
- R-651 : le modèle DOIT être versionné, horodaté et réversible (`/rfx learn reset`).
- R-652 : une mise à jour du modèle NE DOIT JAMAIS activer directement une stratégie : elle ne fait que modifier des estimations, la décision reste soumise à validation.
- R-653 : la dérive DOIT être surveillée : si l'erreur de prédiction moyenne dépasse 50 % sur 1000 décisions, le modèle est réinitialisé aux priors et l'incident journalisé.

**Metrics.** `rfx.learn.prediction_error`, `rfx.learn.updates`, `rfx.learn.exploration_pct`, `rfx.learn.model_version`.

**Tests.** T-470 (convergence sur données synthétiques), T-471 (réinitialisation sur dérive), T-472 (aucune activation directe), T-473 (export/import du modèle).

**Acceptance.** Après 2 heures sur profil `heavy`, l'erreur médiane de prédiction du gain est inférieure à 30 %.

## 5.43 C-45 : Hardware Probe

**Purpose.** Mesurer la machine, pas la deviner.

**Mesures effectuées au démarrage (durée totale < 150 ms).**

```text
- topologie : cœurs physiques/logiques, clusters, NUMA, tailles de cache
- capacités SIMD (cpuid / equivalent)
- coût d'un appel FFI aller-retour minimal (moyenne sur 10 000 appels)
- débit de copie mémoire Java->natif pour 1 Kio, 64 Kio, 1 Mio
- coût de soumission et de réveil d'une tâche
- latence de barrière entre N workers
```

- R-660 : ces mesures alimentent directement le modèle de coût de C-15. Aucune constante de coût codée en dur ne DOIT être utilisée en production.
- R-661 : les mesures DOIVENT être refaites si la charge de la machine au démarrage était anormale (variance élevée détectée) ou sur demande (`/rfx probe`).

**Tests.** T-480 (mesures stables entre exécutions, écart < 20 %), T-481 (détection correcte des cœurs), T-482 (durée < 150 ms).

## 5.44 C-46 : Dashboard UI

**Purpose.** Rendre l'état du runtime lisible en jeu.

**Sections obligatoires.**

```text
Overview | Mods | Workloads | Scheduler | CPU | Memory | Chunks | Entities
| Client | Server | Optimizations | Compatibility | Benchmarks | Diagnostics
```

Exemple de rendu serveur :

```text
TPS                 20.0
MSPT                31.4
Main Thread         17.8 ms
RF-X Workers        14.2 ms   (parallèle, hors chemin critique)
Java Workers         4.1 ms
GC                   0.8 ms
FFI                  0.6 ms
Profiler overhead    0.4 ms   (1.3 %)

Potential offload    9.7 ms   (estimé, non validé)
Active offload       6.2 ms   (mesuré)
```

Exemple de rendu client :

```text
FPS                  165
Frame                6.0 ms
Game                 1.8 ms
Render prep          2.7 ms
RF-X                 1.1 ms
GC                   0.1 ms
1% low               121 FPS
```

Vue mods (colonnes fixes, tri par CPU) :

```text
MOD                  CPU     HOT WORKLOADS   ACTIVE   REJECTED
<modid>             18.4 %        14            8         6
<modid>              9.8 %         9            5         4
```

- R-670 : le dashboard NE DOIT PAS coûter plus de 0,5 ms par frame lorsqu'il est ouvert, et rien du tout lorsqu'il est fermé.
- R-671 : le dashboard affiche uniquement des mesures et des décisions dérivées de propriétés. Aucune "optimisation nommée par mod".

**Tests.** T-490 (coût mesuré), T-491 (aucune donnée fictive affichée : toute valeur provient de C-34).

## 5.45 C-47 : GPU Offload

**Maturité : `FUTURE`.** Aucune implémentation en V1.0.

Ce composant existe uniquement comme interface et comme point d'extension :

```rust
pub trait ComputeDevice {                    // IF-14
    fn kind(&self) -> DeviceKind;            // CPU | GPU
    fn suitability(&self, w: &WorkloadDescriptor) -> f32;
    fn submit(&self, job: ComputeJob) -> Result<JobHandle>;
}
```

- R-680 : aucune décision ne DOIT jamais sélectionner un `ComputeDevice` GPU en V1.0 ; `suitability` renvoie 0 et l'implémentation par défaut refuse toute soumission (`E-2801`).
- R-681 : un travail futur sur GPU DOIT respecter le même pipeline de validation que le CPU.

## 5.46 C-48 : Persistence et Diagnostics Store

**Purpose.** Écrire et faire tourner les fichiers persistés (DM-17), sans jamais bloquer le jeu.

- R-690 : toutes les écritures se font sur le thread d'analyse ou un thread d'IO dédié, jamais sur le thread autoritatif.
- R-691 : rotation par taille et par âge ; conservation par défaut : 7 jours ou 256 Mio.
- R-692 : en cas d'échec d'écriture (disque plein, permission), la persistance se désactive et le runtime continue.

**Tests.** T-500 (aucune IO sur le thread autoritatif, vérifié par instrumentation), T-501 (rotation), T-502 (disque plein simulé).

---

# PARTIE 6 : CONTRATS TECHNIQUES ET ABI

## 6.1 ADR-004 : choix du mécanisme d'interopérabilité

| Option | Avantages | Inconvénients | Verdict |
|---|---|---|---|
| JNI classique | disponible partout, stable, bien outillé | coût par appel, API verbeuse | **retenu** comme mécanisme principal |
| `DirectByteBuffer` + JNI | zéro copie côté natif, un seul appel pour des lots entiers | gestion manuelle de la durée de vie | **retenu** comme mode de transfert par défaut |
| Panama / FFM (`java.lang.foreign`) | ergonomie, performance | non stable sur Java 17 (H-01) | `FUTURE`, prévu derrière l'interface `Bridge` |
| JNA / JNR | simplicité | surcoût, dépendance supplémentaire | rejeté |
| IPC (processus séparé) | isolation forte des pannes | latence, complexité, sérialisation | rejeté pour le chemin chaud, envisageable pour l'analyse hors ligne |

**Principe de conception FFI (R-700)** : la frontière est franchie **par lot et par tick**, jamais par élément. Le budget par défaut est de **moins de 50 traversées FFI par tick** en régime établi, quelle que soit la charge.

```text
INTERDIT                          IMPOSÉ
1000 appels JNI                   1 appel avec un buffer de 1000 éléments
par entité                        par lot d'entités
par bloc                          par région de blocs
```

## 6.2 IF-01 : ABI et versionnement

```c
/* ABI stable, C, sans exceptions, sans allocation implicite */
#define RFX_ABI_VERSION 1u

int32_t  rfx_abi_version(void);
int32_t  rfx_init(const uint8_t* config_cbor, size_t len, uint64_t* out_handle);
int32_t  rfx_shutdown(uint64_t handle);
```

Règles :

- R-701 : toute fonction exportée renvoie un `int32_t` : `0` = succès, négatif = code d'erreur `E-xxxx`. Aucune fonction ne renvoie de pointeur brut sans code d'état associé.
- R-702 : `rfx_abi_version()` DOIT être appelée avant toute autre fonction. Un écart de version majeure interdit tout autre appel.
- R-703 : l'ABI est versionnée indépendamment de la version du produit. Une modification incompatible incrémente `RFX_ABI_VERSION` et le JAR refuse un natif incompatible.
- R-704 : aucune structure Rust `repr(Rust)` ne traverse la frontière. Uniquement `repr(C)`, des entiers, ou des tampons CBOR/binaires à schéma versionné.
- R-705 : toutes les chaînes traversant la frontière sont UTF-8 avec longueur explicite. Aucun `char*` terminé par zéro sans longueur.

## 6.3 IF-02 : cycle de tick

```c
int32_t rfx_tick_begin(uint64_t h, uint64_t tick, int32_t side);
int32_t rfx_phase(uint64_t h, int32_t phase);          /* PRE|VANILLA|DRAIN|POST */
int32_t rfx_tick_end(uint64_t h, uint64_t* out_flags);
```

- Contrat d'appel : `tick_begin` puis `phase(PRE)` ... `phase(POST)` puis `tick_end`, toujours depuis le thread autoritatif, toujours appariés.
- R-706 : si `tick_end` manque (crash d'un autre mod), le prochain `tick_begin` DOIT fermer implicitement le tick précédent, invalider les snapshots ouverts et incrémenter `rfx.tick.unbalanced`.
- R-707 : `rfx_tick_*` NE DOIT JAMAIS bloquer plus de `tick.max_hook_ns` (défaut 500 µs). Dépassement mesuré => réduction automatique de l'activité.

## 6.4 IF-03 : flux de profilage

```c
/* Le Java écrit dans un DirectByteBuffer alloué par le natif, puis notifie. */
int32_t rfx_probe_buffer_acquire(uint64_t h, int32_t thread_id,
                                 void** out_addr, size_t* out_cap);
int32_t rfx_probe_buffer_flush(uint64_t h, int32_t thread_id, size_t used);
```

Format d'enregistrement (binaire, little-endian, aligné 8) :

```text
struct ProbeRecord {          // 32 octets
    u32 probe_id;
    u8  kind;                 // ENTER | EXIT | ALLOC | EVENT | SAMPLE
    u8  flags;
    u16 context_hash16;
    u64 timestamp_ns;         // horloge monotone
    u64 value;                // durée, taille d'allocation, ...
    u64 reserved;
}
```

- R-708 : le buffer est possédé par le natif, sa durée de vie est liée au handle ; Java ne DOIT jamais le libérer.
- R-709 : un flush NE DOIT PAS allouer ni bloquer ; en cas de saturation, les enregistrements les plus anciens sont perdus et comptés.

## 6.5 IF-04 : soumission et récupération de tâches

```c
int32_t rfx_submit(uint64_t h, const uint8_t* task_desc, size_t len, uint64_t* out_task);
int32_t rfx_drain(uint64_t h, uint64_t deadline_ns, void** out_buffers, size_t* out_count);
int32_t rfx_cancel(uint64_t h, uint64_t task);
```

- R-710 : `rfx_drain` DOIT retourner au plus tard à `deadline_ns`, même si des tâches sont en cours. Les tâches non terminées sont annulées et signalées.
- R-711 : les command buffers retournés sont valides jusqu'au prochain `rfx_tick_end`. Java NE DOIT PAS conserver ces pointeurs au-delà.

## 6.6 IF-05 : snapshots

```c
int32_t rfx_snapshot_begin(uint64_t h, int32_t type, const uint8_t* spec, size_t len,
                           void** out_addr, size_t* out_cap, uint64_t* out_snap);
int32_t rfx_snapshot_commit(uint64_t h, uint64_t snap, size_t used, uint64_t epoch);
int32_t rfx_snapshot_release(uint64_t h, uint64_t snap);
```

Le Java remplit directement la mémoire native fournie (écriture SoA), ce qui évite toute copie intermédiaire.

- R-712 : `snapshot_begin` NE DOIT PAS allouer si l'arène du tick a de la place ; sinon il échoue avec `E-1501` (budget) et l'appelant retombe sur Java.

## 6.7 IF-06 : application de commandes

```c
/* Le natif expose la liste des commandes à appliquer ; Java les applique. */
int32_t rfx_commit_next(uint64_t h, CommandView* out);   /* itérateur, sans allocation */
int32_t rfx_commit_result(uint64_t h, uint64_t buffer_id, int32_t outcome);
```

- R-713 : Java DOIT rapporter le résultat de chaque buffer, y compris en cas d'exception ; l'absence de rapport est traitée comme un échec.

## 6.8 Contrats logiques entre composants

| Contrat | Producteur | Consommateur | Garanties |
|---|---|---|---|
| IF-20 Profiler -> Analyzer | C-05 | C-08 | dynamiques cohérentes par tick, jamais partielles au sein d'un tick |
| IF-21 Analyzer -> Decision | C-08 | C-15 | descripteur complet ou explicitement `truncated` |
| IF-22 Decision -> Scheduler | C-15 | C-17 | décision immuable pendant sa durée de vie, révocation atomique |
| IF-23 Scheduler -> Workers | C-17 | C-18 | une tâche s'exécute au plus une fois ; annulation observable |
| IF-24 Workers -> Snapshot | C-18 | C-19 | lecture seule, epoch fourni |
| IF-25 Workers -> CommandBuffer | C-18 | C-21 | append-only, scellé à la fin de la tâche |
| IF-26 CommandBuffer -> Commit | C-21 | C-22 | ordre total déterministe par `order_key` |
| IF-27 Execution -> Validation | C-18/C-22 | C-24 | toute exécution optimisée produit un enregistrement de mesure |
| IF-28 Validation -> Rollback | C-24 | C-26 | une décision de démotion est appliquée en un tick |
| IF-29 Cache -> Fingerprint | C-32 | tous | aucune entrée servie sans correspondance exacte de fingerprint |

## 6.9 Propriété, durée de vie, sûreté

| Ressource | Propriétaire | Durée de vie | Thread safety |
|---|---|---|---|
| `RuntimeHandle` | natif | de `rfx_init` à `rfx_shutdown` | appels concurrents autorisés sauf init/shutdown |
| Snapshot | natif (arène de tick) | jusqu'à `tick_end` | immuable, partageable entre workers |
| CommandBuffer | worker puis natif | jusqu'à l'application ou le rejet | mutable par un seul worker, puis immuable |
| Probe buffer | natif | durée du handle | un buffer par thread, aucun partage |
| Descripteur | natif | durée du handle | accès via `WorkloadStore`, verrouillage granulaire |
| Modèle appris | natif | persisté | mis à jour uniquement par le thread d'analyse |

Règles de sûreté FFI :

- R-720 : aucun objet Java (`jobject`) n'est conservé côté natif au-delà d'un appel, sauf référence globale explicitement gérée et libérée (uniquement pour les applicateurs de commandes).
- R-721 : aucun thread natif ne DOIT s'attacher à la JVM en V1.0, sauf le thread de commit qui n'existe pas (le commit se fait sur le thread autoritatif). Conséquence : les workers n'ont jamais besoin de `AttachCurrentThread` (INV-07).
- R-722 : toute donnée reçue de Java DOIT être validée (bornes, tailles, indices) avant usage. Aucune confiance implicite.

## 6.10 Erreurs, annulation, timeouts, corruption

```text
Codes d'erreur : voir annexe A.2
Toute fonction FFI :
   - ne panique jamais (catch_unwind)
   - ne bloque jamais indéfiniment (toute attente a un délai maximal)
   - est réentrante pour les appels de lecture
Annulation :
   - coopérative uniquement
   - un token annulé rend toute soumission ultérieure de la tâche inopérante
Timeout :
   - chaque tâche a une deadline absolue
   - le dépassement est un résultat normal (Timeout), pas une erreur fatale
Corruption :
   - toute structure persistée est vérifiée (magic, version, CRC)
   - toute incohérence interne détectée déclenche HALT (C-27) et un dump
```

---

# PARTIE 7 : MACHINES À ÉTATS

## 7.1 SM-01 : cycle de vie d'un workload

```text
        découverte
            |
            v
        UNKNOWN
            | première observation
            v
        OBSERVED
            | chaleur >= WARM
            v
      UNDER_ANALYSIS
            | analyse complète, risque acceptable
            v
       SAFE_CANDIDATE
            | shadow OK (C-24)
            v
        VALIDATED
            | canary OK
            v
          ACTIVE
            |
   +--------+---------+
   | anomalie          | changement de fingerprint
   v                   v
REGRESSION          INVALIDATED -> UNKNOWN
   |
   v
SUSPENDED
   |
   v
JAVA_FALLBACK  (peut revenir à UNDER_ANALYSIS après cooldown)
```

Transitions normatives :

| De | Vers | Condition | Action |
|---|---|---|---|
| UNKNOWN | OBSERVED | au moins 100 appels observés | création du descripteur |
| OBSERVED | UNDER_ANALYSIS | `heat >= WARM` et budget d'analyse disponible | file d'analyse |
| UNDER_ANALYSIS | SAFE_CANDIDATE | analyse complète, `risk.correctness < seuil`, stratégie candidate avec gain estimé | décision provisoire |
| UNDER_ANALYSIS | JAVA_FALLBACK | infaisable | motif enregistré |
| SAFE_CANDIDATE | VALIDATED | 64 shadow runs conformes | promotion |
| VALIDATED | ACTIVE | canary 300 ticks, gain mesuré > 0 | activation |
| ACTIVE | REGRESSION | mismatch, timeout répété, gain < 0 | alerte |
| REGRESSION | SUSPENDED | immédiat | annulation des tâches |
| SUSPENDED | JAVA_FALLBACK | immédiat | stratégie sûre |
| JAVA_FALLBACK | UNDER_ANALYSIS | cooldown écoulé (défaut 30 min) et fingerprint inchangé et moins de 3 incidents | nouvelle tentative |
| tout état | INVALIDATED | fingerprint changé | purge des décisions |

- R-730 : un workload NE PEUT PAS passer de `UNDER_ANALYSIS` à `ACTIVE` sans traverser `SAFE_CANDIDATE` et `VALIDATED`, sauf pour les stratégies TL<=2 qui peuvent passer directement à `ACTIVE` avec surveillance renforcée (elles ne changent pas la sémantique d'exécution).

## 7.2 SM-02 : machine de décision

Voir §5.15. États : `EVALUATING -> INFEASIBLE | FEASIBLE -> SCORED -> SELECTED -> PUBLISHED -> REVOKED`.

- R-731 : une décision `PUBLISHED` est immuable ; toute modification passe par `REVOKED` puis une nouvelle décision.

## 7.3 SM-03 : cycle de validation

```text
CANDIDATE -> SHADOW -> CANARY -> ACTIVE
     ^          |         |         |
     |          v         v         v
     +----- BANNED <--- FAILED <- REGRESSED
```

- `BANNED` est lié à un fingerprint : un changement de fingerprint réhabilite la stratégie.

## 7.4 SM-04 : fenêtre de tick

```text
CLOSED --tick_begin--> OPEN_PRE --phase(VANILLA)--> OPEN_VANILLA
   ^                                                     |
   |                                              phase(DRAIN)
   |                                                     v
   +---tick_end--- OPEN_POST <--phase(POST)-- DRAINING --+
```

- R-732 : les snapshots ne peuvent être pris qu'en `OPEN_PRE`. Les commits ne peuvent avoir lieu qu'en `DRAINING`. Toute violation est une erreur interne (`E-2901`).

## 7.5 SM-05 : cycle de vie d'une tâche

```text
CREATED -> QUEUED -> RUNNING -> COMPLETED
             |          |            |
             |          +-> TIMEOUT  +-> BUFFER_SEALED -> VALIDATED -> APPLIED
             +-> CANCELLED                                    |
                                                              +-> DISCARDED
RUNNING -> PANICKED (worker recyclé, tâche marquée échouée)
```

## 7.6 SM-06 : état du runtime

```text
INIT -> RUNNING -> DEGRADED -> HALTED
          ^  |        |
          |  +--------+  (rétablissement possible si la cause disparaît)
          +-----------+
```

| État | Profilage | Décisions | Exécution optimisée |
|---|---|---|---|
| `INIT` | non | non | non |
| `RUNNING` | oui | oui | oui |
| `DEGRADED` | réduit ou non | non (gel des décisions) | seules les décisions TL<=2 déjà actives |
| `HALTED` | non | non | non, tout en Java |

## 7.7 SM-07 : cycle de vie d'une entrée de cache

```text
ABSENT -> WRITING -> VALID -> STALE -> PURGED
                       |        ^
                       +--------+ (revalidation par fingerprint)
CORRUPT détecté à la lecture -> PURGED
```

---

# PARTIE 8 : CONCURRENCE, CORRECTNESS ET INVARIANTS

## 8.1 Invariants globaux

Ces invariants sont **vérifiables** et **vérifiés** (tests dédiés, assertions en mode DEBUG).

| ID | Invariant | Vérification |
|---|---|---|
| INV-01 | le task graph est acyclique | assertion à l'insertion + T-250 |
| INV-02 | l'état autoritatif n'est muté que sur le thread autoritatif | assertion dans les applicateurs + T-310 |
| INV-03 | aucun effet non validé n'atteint l'état autoritatif | protocole de commit + T-311/312 |
| INV-04 | aucune exécution IR/native sans validation shadow complète | C-24 + T-330 |
| INV-05 | pour les mêmes entrées et le même fingerprint, la séquence de commandes commitées est identique | T-301, T-310 |
| INV-06 | aucun worker n'acquiert de verrou JVM ni de moniteur Java | R-431, T-264 |
| INV-07 | aucun thread natif ne s'attache à la JVM | revue + T-364 |
| INV-08 | le thread autoritatif n'attend jamais un worker au-delà de la deadline de drain | R-710, T-262 |
| INV-09 | tout snapshot utilisé pour produire des commandes est revalidé au commit | C-23 + T-311 |
| INV-10 | l'overhead total de RF-X est mesuré et borné | C-05, C-34 + T-140 |
| INV-11 | toute décision est explicable | C-43 + T-460 |
| INV-12 | aucune logique du moteur ne dépend d'un nom de mod | T-245 (analyse statique du code source) |
| INV-13 | aucune dépendance de couche inférieure vers couche supérieure | test d'architecture sur les dépendances de crates (T-005) |
| INV-14 | aucune allocation Java dans le chemin de sonde | T-142 |
| INV-15 | aucun accès réseau initié par RF-X | T-400 |

## 8.2 Principe conservateur formalisé

```text
Soit P une propriété nécessaire à une transformation T.
Si l'analyse ne peut pas prouver P, alors P est considérée fausse.

Conséquences directes :
   dépendance inconnue        -> dépendance maximale
   effet de bord inconnu      -> effet de bord total
   thread inconnu             -> thread autoritatif requis
   déterminisme inconnu       -> non déterministe
   coût inconnu               -> coût maximal, gain nul
```

Ce principe n'est pas une politique de prudence : c'est une **règle de correction**. Toute implémentation qui l'assouplit est un défaut.

## 8.3 Matrice de conflits complète

| Situation | Détection | Décision |
|---|---|---|
| READ / READ, mêmes régions | non applicable | autorisé |
| READ / WRITE, régions disjointes | domaines et régions | autorisé |
| READ / WRITE, régions croisées, lecteur ordonné avant | epoch + order_key | autorisé si le lecteur ne dépend pas de l'écriture |
| READ / WRITE, régions croisées, lecteur ordonné après | epoch | conflit : le lecteur est rejeté (snapshot périmé) |
| WRITE / WRITE, régions croisées | epoch + read/write sets | conflit : le buffer de plus grand `order_key` est rejeté |
| Snapshot périmé | epoch de région | rejet |
| Version de schéma incompatible | en-tête de structure | rejet + purge |
| Course sur structure interne RF-X | impossible par conception (structures lock-free ou possédées) | test de stress T-6xx |
| Violation d'ordre d'événement | C-06 | interdit par construction (R-330) |
| Deadlock | watchdog | annulation + `FORCE_SERIAL` |
| Timeout | deadline | annulation + démotion progressive |
| Rollback | C-26 | retour à la stratégie sûre |
| Retry | politique | au plus 1 tentative, uniquement pour un rejet non lié à la correction |
| Sérialisation forcée | mode dégradé | résultat identique au jeu non optimisé |

## 8.4 Modèle mémoire et publication

```text
- toute donnée partagée entre le thread autoritatif et les workers est publiée
  par un store Release et lue par un load Acquire (ou via un canal qui l'assure)
- les snapshots sont immuables après publication : aucune synchronisation
  supplémentaire n'est nécessaire pour les lire
- les compteurs de télémétrie utilisent Relaxed, sauf ceux servant à une décision
  de sûreté (Acquire/Release)
- aucun accès non synchronisé à une structure mutable partagée n'est autorisé,
  même "probablement sûr"
```

## 8.5 Déterminisme et parallélisme

```text
Pour qu'un workload parallélisé produise un résultat déterministe :
  1. les entrées proviennent d'un snapshot immuable
  2. la décomposition en sous-tâches est fonction déterministe des entrées
     (jamais du nombre de workers disponibles au moment T)
  3. la réduction est associative ET effectuée dans un ordre fixe
     (arbre de réduction indexé, pas "premier arrivé")
  4. les sorties sont ordonnées par order_key stable
  5. aucun RNG partagé n'est consulté
```

- R-740 : le nombre de sous-tâches PEUT dépendre du matériel, mais le **résultat** NE DOIT PAS en dépendre. Test T-263 vérifie l'identité des résultats avec 1, 2, 4 et 16 workers.

## 8.6 Traitement du hasard

```text
Politique par défaut : tout accès à un RNG partagé rend le workload
NON_DETERMINISTIC et non offloadable.

Option EXPERIMENTAL (désactivée par défaut, interdite en multijoueur) :
  substitution par un flux déterministe indexé
     seed_i = splitmix64(world_seed ^ tick ^ work_id ^ index)
  autorisée uniquement si :
     - le RNG n'est pas observé ailleurs dans le même tick
     - la séquence globale du RNG partagé n'est pas consommée par ce workload
       (sinon le décalage de séquence change le comportement du reste du jeu)
```

## 8.7 Interblocage : prévention par conception

```text
1. aucun verrou acquis par un worker en dehors du runtime
2. hiérarchie de verrous interne stricte et documentée :
      config < store < scheduler < telemetry
   acquisition dans cet ordre uniquement, vérifiée en mode DEBUG
3. aucune attente circulaire possible : le thread autoritatif n'attend que le
   drain, avec deadline ; les workers n'attendent jamais le thread autoritatif
4. parallélisme imbriqué en "help-first" : un parent qui attend exécute du travail
```

## 8.8 Gestion des exceptions Java dans un chemin optimisé

```text
- si le chemin Java d'origine peut lever une exception observable, le chemin
  optimisé DOIT reproduire exactement la même exception, au même moment logique
- en pratique : les workloads pouvant lever une exception observable ne sont
  offloadés que si l'exception est modélisée dans le résultat de la tâche
  (TaskOutcome::Invalid + reconstruction côté Java du même type d'exception)
- si l'exception ne peut pas être reproduite fidèlement -> REJECT(UnknownSideEffect)
```

---

# PARTIE 9 : JVM, BYTECODE, LIMITES DURES

Cette partie énonce explicitement ce que la JVM autorise, ce qu'elle interdit, et comment RUSTFORGE-X se comporte dans chaque cas. Elle est normative : toute implémentation qui contredit cette partie est fausse.

## 9.1 JIT (C1/C2)

| Fait | Conséquence pour RF-X |
|---|---|
| le JIT compile et recompile selon des profils internes | toute mesure de coût est instable pendant la phase de chauffe : les décisions ne sont prises qu'après `warmup_ticks` (défaut 1200 ticks, soit 60 s) |
| l'inlining peut faire disparaître une méthode | une sonde d'entrée/sortie empêche parfois l'inlining : c'est un coût réel, mesuré (R-313) |
| la déoptimisation peut survenir à tout moment | une hausse soudaine de coût sans changement de charge n'est PAS une régression RF-X : le détecteur exige une persistance sur 300 ticks avant de conclure |
| `nanoTime` a un coût non nul (20 à 30 ns) | le niveau `TIMED` n'est posé que sur des méthodes suffisamment longues (R-311) |

- R-750 : aucune décision d'activation ne DOIT être prise avant la fin de la phase de chauffe, sauf restauration depuis le cache avec fingerprint identique.

## 9.2 Instrumentation de bytecode

| Mécanisme | Disponibilité | Usage RF-X |
|---|---|---|
| transformation au chargement (ModLauncher/Mixin) | oui | mécanisme principal, sondes et hooks |
| `java.lang.instrument` + agent | seulement si l'utilisateur ajoute `-javaagent` | optionnel, active la retransformation à chaud |
| `Instrumentation.retransformClasses` | via agent uniquement | ajustement de niveau de sonde hors tick |
| redéfinition changeant le schéma (champs, méthodes) | interdit par la JVM | jamais utilisé |
| `Unsafe.defineAnonymousClass` / hidden classes | possible mais fragile | non utilisé en V1.0 |

- R-751 : RF-X DOIT fonctionner **sans agent**. L'agent est un bonus, jamais un prérequis.
- R-752 : le transformateur RF-X DOIT s'exécuter en dernier dans la chaîne, pour que `bytecode_hash` corresponde au bytecode réellement exécuté (R-209).

## 9.3 Chargement de classes

```text
- les classes sont chargées paresseusement : l'analyse doit l'être aussi
- un mod peut charger des classes à tout moment, y compris en cours de partie
- les chargeurs sont multiples (module layer Forge) : l'attribution de propriétaire
  se fait par chargeur (C-41)
- une classe générée dynamiquement n'a pas de jar : owner_mod_hash indisponible
  -> owner = "unknown", offload interdit au-delà de TL2 (RISK-09)
```

## 9.4 Réflexion, MethodHandle, invokedynamic

| Construction | Analysabilité | Politique |
|---|---|---|
| `Method.invoke` avec cible constante | partielle | résolution tentée ; sinon `unresolved` |
| `Method.invoke` avec cible dynamique | non | `UNKNOWN` |
| `MethodHandle` lié statiquement (lambda) | partielle | résolution via le pool de constantes |
| `invokedynamic` de lambda | partielle | cible statique identifiable dans la plupart des cas |
| `invokedynamic` de concaténation de chaînes | oui | traité comme une allocation, sans effet de bord |
| proxy dynamique | non | `UNKNOWN` |

- R-753 : la présence de réflexion non résolue dans le corps d'un workload interdit TL3 et au-delà.

## 9.5 Synchronisation

```text
- synchronized (méthode ou bloc) -> uses_synchronized, main_thread_required
  reste UNKNOWN mais THREAD_SAFE ne peut pas être conclu
- java.util.concurrent locks détectés par appel -> uses_locks
- volatile : lecture/écriture notées comme accès mémoire ordonnés
- RF-X NE DOIT JAMAIS retirer ni contourner une synchronisation existante (§1.4)
```

## 9.6 Exceptions

```text
- une sonde DOIT utiliser try/finally pour ne pas perdre l'événement de sortie
- une exception traversant un workload est un effet observable (DM-07)
- RF-X NE DOIT JAMAIS avaler une exception du code observé
```

## 9.7 JNI et méthodes natives dans le code observé

```text
- un workload appelant une méthode native tierce est UNMODELED_NATIVE_CALL
- politique : REJECT au-delà de TL1, sauf si la méthode native est connue comme
  pure par déclaration explicite de l'utilisateur (option avancée, hors défaut)
```

## 9.8 Garbage collection

```text
- RF-X ne contrôle pas le GC et ne DOIT pas prétendre le faire
- objectif mesurable : réduire allocation_rate_bytes_per_tick sur les chemins
  optimisés, ce qui réduit la fréquence des collectes
- la mémoire native RF-X n'est PAS gérée par le GC : elle est bornée (R-533)
- les pauses GC DOIVENT être exclues des mesures de gain (détection via
  GarbageCollectorMXBean, ticks contaminés marqués et écartés des statistiques)
```

## 9.9 Ce qui doit rester Java (conservé du V0.3 §43, précisé)

Un workload DOIT rester `JAVA_ONLY` s'il dépend de :

```text
- réflexion ou introspection non résolues
- ordre exact d'exécution observable non modélisable
- API non thread-safe atteignable
- mutation directe de structures Minecraft non miroitées
- objets JVM complexes non représentables en données valeurs
- code natif externe
- effets de bord inconnus
- chargement dynamique de classes dans son chemin chaud
```

Règle : **optimiser ce qui peut être prouvé, ne pas casser ce qui ne peut pas l'être.**

---

# PARTIE 10 : PIPELINE JAVA VERS RUST, RF-IR, COMPILATION

## 10.1 Vue d'ensemble

```text
Java workload
   |
   v
[1] capture            (entrées, bytecode, contexte)
   |
   v
[2] éligibilité        (sous-ensemble supporté ?)
   |
   +--- non ---> stratégie <= TL4 (offload d'opération sans transformation)
   |
   v
[3] lifting -> RF-IR   (C-28)
   |
   v
[4] vérification IR    (typage, bornes, absence d'effets non modélisés)
   |
   v
[5] optimisation       (C-29)
   |
   v
[6] exécution
       |-- interpréteur IR         (par défaut, EXPERIMENTAL)
       +-- code natif compilé      (C-30, feature "jit", EXPERIMENTAL)
   |
   v
[7] validation shadow  (C-24, obligatoire)
   |
   v
[8] promotion / bannissement
```

## 10.2 Sous-ensemble supporté (table normative)

| Construction bytecode | Statut | Note |
|---|---|---|
| arithmétique entière et flottante (`iadd`, `dmul`, ...) | SUPPORTED | sémantique IEEE-754 préservée, pas de réassociation |
| comparaisons et branchements | SUPPORTED | |
| boucles à bornes calculables | SUPPORTED | |
| variables locales primitives | SUPPORTED | |
| tableaux de primitifs (`iaload`, `dastore`, ...) | SUPPORTED | avec vérification de bornes explicite |
| `getfield`/`putfield` sur objets copiés dans le snapshot | PARTIALLY_SUPPORTED | uniquement champs primitifs de types dont la disposition est déclarée |
| appels statiques vers méthodes elles-mêmes SUPPORTED | SUPPORTED | inlining requis |
| appels virtuels monomorphes vers méthodes SUPPORTED | PARTIALLY_SUPPORTED | garde de type obligatoire |
| `Math.*` (sin, cos, sqrt, floor, ...) | SUPPORTED | implémentations bit-exactes requises, sinon UNSUPPORTED |
| allocation d'objets | UNSUPPORTED | sauf tableaux locaux de primitifs |
| `String`, collections | UNSUPPORTED | |
| exceptions (`athrow`, blocs `try`) | PARTIALLY_SUPPORTED | seules les exceptions implicites modélisées (division par zéro, hors bornes) |
| `synchronized`, `monitorenter` | UNSUPPORTED | |
| appels virtuels polymorphes | UNSUPPORTED | |
| réflexion, `invokedynamic` non résolu | UNSUPPORTED | |
| appels natifs | UNSUPPORTED | |
| accès à l'état Minecraft non miroité | UNSUPPORTED | |

- R-760 : une seule construction `UNSUPPORTED` dans le corps rend le workload non éligible à l'IR (`IR_ELIGIBLE = false`). Aucune "approximation".

## 10.3 RF-IR : définition

```rust
pub enum IrType { I32, I64, F32, F64, Bool, Ptr(ArrayTy) }

pub enum IrInst {
    Const { dst: Val, ty: IrType, bits: u64 },
    Bin   { dst: Val, op: BinOp, a: Val, b: Val },
    Un    { dst: Val, op: UnOp, a: Val },
    Cmp   { dst: Val, op: CmpOp, a: Val, b: Val },
    Load  { dst: Val, arr: Val, idx: Val, ty: IrType },   // borne vérifiée
    Store { arr: Val, idx: Val, src: Val, ty: IrType },
    Br    { cond: Val, then_bb: Bb, else_bb: Bb },
    Jmp   { target: Bb },
    Phi   { dst: Val, incoming: Vec<(Bb, Val)> },
    Call  { dst: Option<Val>, func: IrFuncId, args: Vec<Val> },  // fonctions IR pures
    Ret   { val: Option<Val> },
    Trap  { kind: TrapKind },   // DivByZero, IndexOutOfBounds, ...
}

pub struct IrFunction {
    pub id: IrFuncId,
    pub params: Vec<IrType>,
    pub ret: Option<IrType>,
    pub blocks: Vec<BasicBlock>,   // forme SSA
    pub attrs: IrAttrs,            // pure, deterministic, no_trap, bounds_checked
}
```

Propriétés obligatoires :

- forme SSA avec blocs de base et `phi`
- typage statique total, aucun type dynamique
- aucune opération d'effet de bord hors `Store` sur tableaux appartenant à la tâche
- les pièges (`Trap`) sont explicites : toute opération pouvant échouer est modélisée

## 10.4 Vérificateur IR (obligatoire avant toute exécution)

```text
Le vérificateur REFUSE une fonction IR si :
   - un Val est utilisé avant définition (violation SSA)
   - les types ne concordent pas
   - un Load/Store n'est pas précédé d'une vérification de borne ou n'est pas
     marqué bounds_checked
   - le graphe de flot contient un bloc inaccessible non éliminé
   - une boucle n'a pas de condition de sortie prouvable ET pas de compteur
     d'itérations borné (protection contre les boucles infinies)
   - la fonction appelle une fonction non vérifiée
```

- R-761 : l'exécution IR DOIT être bornée par un compteur d'instructions (`ir.max_steps`, défaut 10^7). Dépassement => `Trap(StepLimit)` et démotion.

## 10.5 Passes d'optimisation (C-29)

| Passe | Effet | Condition de sûreté |
|---|---|---|
| constant folding | évalue les expressions constantes | sémantique IEEE respectée |
| dead code elimination | supprime le code sans effet | aucun `Store` ni `Trap` supprimé |
| common subexpression elimination | factorise | opérandes identiques et pures |
| copy propagation | simplifie | SSA |
| loop invariant code motion | sort les invariants | pas de `Trap` déplacé avant sa garde |
| unrolling borné | déroule les petites boucles | facteur <= 8, taille de code bornée |
| bounds check elimination | supprime les vérifications prouvées redondantes | preuve par intervalle obligatoire |
| vectorisation (SIMD) | traite k éléments à la fois | uniquement si l'ordre n'affecte pas le résultat ; jamais sur une réduction flottante non associative |
| layout SoA | réorganise les tableaux | uniquement sur des tampons possédés par la tâche |

- R-762 : **aucune passe ne DOIT changer le résultat observable**. Chaque passe possède un test de non-régression sémantique (T-51x) comparant l'IR avant/après sur des entrées aléatoires (fuzzing).

## 10.6 Exécution

```text
Interpréteur IR (par défaut si IR activé)
   - boucle de dispatch simple, sans allocation
   - coût typique 3 à 10x plus lent que du natif, mais permet la validation
   - suffit à valider la correction avant d'activer le JIT

Backend Cranelift (feature "jit", EXPERIMENTAL, désactivé par défaut)
   - traduction IR -> IR Cranelift -> code machine
   - compilation hors tick, en tâche BACKGROUND
   - cache disque (compiled_cache) clé = hash(IR) + ISA + version
   - une fonction compilée n'est utilisée qu'après validation shadow sur la
     version compilée elle-même (INV-04)
```

- R-763 : le JIT NE DOIT PAS être une dépendance de build par défaut : la feature Cargo est optionnelle et la CI construit les deux variantes.
- R-764 : une fonction compilée DOIT être exécutée avec les mêmes gardes que l'interpréteur (bornes, limites d'étapes via compteur de retour de boucle).

## 10.7 Fallback

```text
échec de lifting        -> pas d'IR, stratégie <= TL4
échec de vérification   -> IR rejeté, journalisé, workload marqué non éligible
trap à l'exécution      -> résultat invalide, retour Java pour cet appel,
                           incrément d'un compteur ; 3 traps -> démotion
mismatch de validation  -> bannissement de la stratégie IR pour ce fingerprint
```

## 10.8 Critères d'acceptation de la chaîne IR

```text
[ ] 100 % des fonctions IR passent le vérificateur avant exécution
[ ] fuzzing de 10^6 entrées : interpréteur IR == Java sur le corpus éligible
[ ] fuzzing de 10^6 entrées : code compilé == interpréteur IR
[ ] aucune passe d'optimisation ne modifie un résultat (T-51x)
[ ] le JIT désactivé, le système reste pleinement fonctionnel
```

---

# PARTIE 11 : VALIDATION, SHADOW EXECUTION, PROMOTION

Cette partie complète C-24, C-25, C-26 par le protocole complet.

## 11.1 Pipeline obligatoire

```text
Candidate
   -> Safety analysis        (C-14 : risque acceptable ?)
   -> Cost analysis          (C-15 : gain estimé > seuil ?)
   -> Shadow execution       (C-25 : 64 exécutions comparées)
   -> Result comparison      (C-24 : oracle strict)
   -> Benchmark              (mesure réelle sur canary)
   -> Promotion              (ACTIVE)
```

Aucune étape n'est facultative pour TL >= 3.

## 11.2 Oracle de correction : ce qui est comparé

| Aspect | Comparaison | Tolérance |
|---|---|---|
| valeur de retour | égalité binaire | aucune |
| flottants | égalité bit à bit | aucune par défaut ; une tolérance explicite est une option avancée qui interdit la promotion au-delà de TL4 |
| séquence de commandes | égalité après tri par `order_key` | aucune |
| régions écrites | hash des régions | aucune |
| exceptions | type et condition | aucune |
| événements émis | type, ordre, annulation | aucune |
| durée | non comparée pour la correction | mesurée séparément |

## 11.3 Vérification d'invariants à l'exécution

En mode `DEBUG` et pendant les phases `CANARY`, les invariants suivants sont vérifiés en continu :

```text
- somme des entités par chunk cohérente avant/après
- nombre de block entities inchangé sauf commande explicite
- aucun bloc modifié hors des régions déclarées dans le write set
- aucun paquet supplémentaire émis
- aucune allocation Java inattendue dans le chemin optimisé
```

Toute violation => rollback immédiat + dump.

## 11.4 Canary

```text
- fraction du trafic : canary.traffic_pct (défaut 10 %)
- durée : canary.ticks (défaut 300)
- sélection déterministe : hash(order_key) % 100 < traffic_pct
  (jamais aléatoire, pour être reproductible)
- pendant le canary, le chemin Java reste exécuté pour la fraction restante,
  ce qui fournit une comparaison de performance dans les mêmes conditions
```

## 11.5 Promotion et démotion

```text
Promotion  : mismatches == 0 ET timeouts == 0 ET gain_mesuré > 0 ET
             conflits < conflict_threshold (défaut 2 %)
Démotion   : un seul mismatch, OU 3 timeouts, OU gain < 0 sur 300 ticks,
             OU conflits > 5 %, OU incident système
Cooldown   : 30 minutes avant nouvelle tentative, doublé à chaque échec,
             plafonné à 4 heures
Bannissement définitif (pour le fingerprint) : 3 démotions pour correction
```

## 11.6 Journalisation obligatoire

Chaque transition de validation produit une entrée dans `optimization.log` :

```text
[2026-01-01T12:00:00Z] WORK 0x1827A44C  SHADOW  runs=64 mismatch=0 p50=12.1us
[2026-01-01T12:00:31Z] WORK 0x1827A44C  CANARY  traffic=10% ticks=300 gain=+1.42ms/tick
[2026-01-01T12:05:42Z] WORK 0x1827A44C  ACTIVE  strategy=RUST_PARALLEL(workers=6)
[2026-01-01T13:11:07Z] WORK 0x1827A44C  DEMOTE  reason=CONFLICT_RATE(6.1%) -> JAVA_BATCH
```

---

# PARTIE 12 : PROFILING ADAPTATIF ET BUDGETS

## 12.1 Niveaux de profondeur

| Chaleur | Instrumentation | Analyse | Coût cible |
|---|---|---|---|
| `COLD` | compteur ou rien, échantillonnage | aucune | ~0 |
| `WARM` | compteur + durée | statique légère (signature, appels directs) | < 0,3 % |
| `HOT` | durée + contexte + allocation | statique complète + dépendances | < 1 % |
| `CRITICAL` | tout + capture d'entrées pour shadow | complète + validation prioritaire | < 2 % |

## 12.2 Budgets

| Budget | Défaut | Comportement au dépassement |
|---|---|---|
| CPU profilage | 2 % d'un cœur | réduction de niveau, puis `THROTTLED`, puis `OFF` |
| CPU analyse | 5 % d'un cœur, thread dédié basse priorité | file d'analyse ralentie, jamais de blocage |
| Mémoire profilage | 64 Mio | éviction LRU des `COLD` |
| Mémoire native totale | 512 Mio | purge des caches, `DEGRADED` |
| Snapshot par tick | 4 Mio | refus, repli Java |
| Commit par tick | 2 ms | rejet des buffers restants |
| Traversées FFI par tick | 50 | agrégation forcée, alerte |
| Shadow CPU | 5 % d'un cœur | suspension de la shadow execution |

## 12.3 Priorité des budgets

```text
jeu > commit > profilage > analyse > validation > expérimentation
```

Un budget de rang inférieur est **toujours** sacrifié en premier.

## 12.4 Auto-mesure de l'overhead

```text
overhead_total = t_sondes + t_flush + t_echantillonnage + t_hooks
               + t_soumission + t_drain + t_commit + t_telemetrie

mesuré par :
   - compteurs dédiés autour de chaque zone
   - comparaison périodique "tick avec RF-X actif" vs "tick avec RF-X en pause"
     (une fois toutes les N=6000 ticks, RF-X se met en pause pendant 20 ticks
     pour mesurer une ligne de base ; cette pause est invisible pour le joueur)

si overhead_total > budget pendant 100 ticks consécutifs :
   réduire le niveau de profilage ;
   si toujours au-dessus : suspendre les nouvelles décisions ;
   si toujours au-dessus : DEGRADED
```

- R-770 : le runtime DOIT toujours être capable de répondre à la question "combien est-ce que je coûte ?" avec un chiffre mesuré, jamais estimé.

---

# PARTIE 13 : BOUCLE D'APPRENTISSAGE

## 13.1 Boucle canonique

```text
OBSERVE   -> collecte de mesures (C-05, C-34)
ANALYZE   -> faits statiques et dynamiques (C-07..C-13)
CLASSIFY  -> flags et chaleur (C-08)
DECIDE    -> stratégie et score (C-15)
EXTRACT   -> capture, snapshot, IR éventuel (C-19, C-28)
OPTIMIZE  -> passes, batching, parallélisation (C-29, C-17)
VALIDATE  -> shadow, canary (C-24, C-25)
EXECUTE   -> workers, commit (C-18, C-22)
MEASURE   -> gain réel, conflits, stabilité (C-34)
LEARN     -> mise à jour du modèle de coût et de fiabilité (C-44)
```

## 13.2 Ce qui est appris et ce qui ne l'est jamais

```text
APPRIS
  - modèles de coût (FFI, copie, soumission, barrière)
  - speedup attendu par profil de workload et stratégie
  - paramètres (taille de batch, nombre de workers, grain)
  - fiabilité empirique (probabilité de conflit ou de mismatch)

JAMAIS APPRIS
  - les règles de sûreté (elles sont fixes et prouvées)
  - le fait qu'une transformation est correcte (cela se valide, cela ne s'apprend pas)
  - une autorisation d'ignorer un invariant
```

- R-780 : aucune quantité apprise ne DOIT pouvoir relâcher une contrainte de sûreté. L'apprentissage n'agit que sur les estimations de gain et de coût.

## 13.3 Persistance et réversibilité

```text
modèle : cache/model.json (lisible, versionné, horodaté)
commandes : /rfx learn show | export | reset
réinitialisation automatique : dérive détectée (R-653), changement majeur de version
```

---

# PARTIE 14 : CACHES

Complète C-32.

## 14.1 Clés

```text
workload_cache        : WorkId + Fingerprint
optimization_cache    : WorkId + Fingerprint + mode
compiled_cache        : hash(IR) + ISA + rfx_version
compatibility_cache   : mod_set_hash + owner_mod_hash
benchmark_cache       : WorkId + Fingerprint + StrategyKind + params_hash
result_cache          : WorkId + hash(entrées normalisées)
```

## 14.2 Invalidation

```text
- par fingerprint (systématique, voir §4.5)
- par écriture : result_cache invalidé dès qu'une écriture touche un domaine lu
- par TTL en ticks : result_cache, défaut 200 ticks
- par version de schéma : purge complète
- par commande utilisateur : /rfx cache purge
```

## 14.3 Corruption

```text
- CRC32C par entrée, vérifié à la lecture
- en-tête de fichier : magic "RFXDB1\0" + schema_version + fingerprint global
- entrée corrompue -> purge de l'entrée + compteur
- fichier corrompu -> rotation en .bak + recréation
```

## 14.4 Éviction

```text
- LRU par cache, avec plancher de rétention pour les entrées HOT
- éviction déclenchée à 90 % de la taille maximale
- éviction par lot (batch) pour éviter le coût d'éviction unitaire
```

## 14.5 Tailles maximales

Voir tableau §5.29. Total par défaut : **256 Mio sur disque**, **128 Mio en mémoire**.

## 14.6 Versionnement de schéma

```text
schema_version est incrémentée à chaque changement de structure persistée.
La lecture d'une version inférieure est autorisée si une migration existe ;
sinon le fichier est mis de côté et recréé. Aucune lecture "au mieux".
```

---

# PARTIE 15 : ARCHITECTURE CLIENT ET SERVEUR

## 15.1 Serveur

```text
             SERVER
                |
          Server thread  (autoritatif)
                |
          RUSTFORGE-X hooks
                |
       +--------+--------+
       v        v        v
     Chunk     AI       IO
    workers   workers  workers        (pool unique, catégories logiques)
       |        |        |
       +--------+--------+
                v
          Ordered commit
```

Métriques serveur suivies : TPS, MSPT, budget de tick (50 ms), chemin critique, temps de simulation, entités, chunks, IA, pathfinding, réseau.

Décomposition du MSPT publiée par RF-X :

```text
MSPT = t_vanilla + t_mods + t_rfx_hooks + t_commit + t_gc_attribue
```

- R-790 : `t_rfx_hooks + t_commit` DOIT rester inférieur à 5 % du MSPT en régime établi.

## 15.2 Client

```text
             CLIENT
                |
       +--------+--------+
       |                 |
   Game logic        Render prep
       |                 |
       v                 v
  RF-X hooks         RF-X hooks
       |                 |
       +--------+--------+
                v
          Worker pool (frame-aware)
```

Métriques client : FPS, frametime, 1 % low, 0,1 % low, temps de game thread, préparation de rendu, préparation de chunks, CPU, mémoire.

**Scheduler frame-aware (R-791).**

```text
- le scheduler connaît le temps restant avant la fin du budget de frame
- une tâche dont le coût estimé dépasse le temps restant n'est pas lancée sur le
  chemin critique : elle est reportée ou découpée
- objectif explicite : ne jamais transformer un gain moyen en spike de frametime
- métrique de contrôle : le 0,1 % low ne doit jamais se dégrader, même si la
  moyenne s'améliore (critère de rejet d'une optimisation)
```

- R-792 : une optimisation qui améliore le FPS moyen mais dégrade le 1 % low de plus de 3 % DOIT être rejetée.

## 15.3 Différences de politique

| Aspect | Serveur | Client |
|---|---|---|
| Budget de tick | 50 ms | dépend du FPS cible, mesuré |
| Priorité | throughput et stabilité du tick | latence de frame et régularité |
| Parallélisme | agressif si sûr | conservateur pendant la frame, agressif entre les frames |
| Snapshots | par tick | par frame et par tick logique |
| Réseau | émission | réception |
| Déterminisme | critique (autorité) | important (prédiction) |

---

# PARTIE 16 : SOUS-SYSTÈMES DE DOMAINE GÉNÉRIQUES

Ces sous-systèmes ne sont **pas** des optimisations pour un mod donné. Ce sont des **pipelines génériques** que le moteur peut utiliser quand un workload correspond à leur forme.

## 16.1 C-49 : Entity Engine

**Purpose.** Traiter par lots des opérations d'entités indépendantes.

```text
Entities (sélection par C-15)
   -> Snapshot ENTITY_SET (SoA : id, pos, vel, aabb, flags, target)
   -> Partition spatiale (grille de chunks, taille de cellule configurable)
   -> Workers Rust (une cellule = une tâche, indépendance garantie par partition)
   -> Results (patches d'entités)
   -> Validation (epoch par entité)
   -> Commit ordonné par (chunk, entity_id)
```

Opérations génériques supportées : recherche de voisinage, filtrage par distance, sélection de cible, tests de visibilité géométriques, mise à jour de vitesse, détection de collisions candidates (broad phase).

- R-800 : le moteur NE DOIT PAS remplacer la logique d'IA d'un mod. Il ne peut accélérer que des **opérations pures identifiées** (voisinage, distances, filtres) dont le workload dépend.
- R-801 : toute entité modifiée hors du pipeline pendant le tick invalide le patch correspondant.

**Tests.** T-520 (voisinage identique à la référence sur 10^5 entités), T-521 (partition n'affecte pas le résultat), T-522 (invalidations correctes).

## 16.2 C-50 : Chunk Engine

**Purpose.** Représenter le pipeline de chunk en tâches ordonnées.

```text
Request -> Load -> Generate -> Structures -> Lighting -> Features -> Mesh -> Upload
```

- chaque étape est un `TaskNode` avec dépendances explicites
- le parallélisme entre chunks est autorisé si les régions ne se chevauchent pas (bordures incluses : une marge d'un chunk est intégrée aux régions)
- RF-X n'implémente PAS sa propre génération de terrain en V1.0 : il ordonnance et accélère les étapes **pures** qui lui sont confiées (calculs de bruit, remplissage de tableaux, compression de sections)

- R-810 : aucune étape ne DOIT être exécutée hors du thread autoritatif si elle appelle du code de mod non prouvé (générateurs de structures tiers, décorateurs).
- R-811 : le chargement de chunk NE DOIT JAMAIS être déclenché par RF-X.

**Tests.** T-530 (chunks identiques avec et sans RF-X, comparaison NBT), T-531 (aucune génération supplémentaire), T-532 (marge de bordure respectée).

## 16.3 C-51 : Pathfinding Engine

**Purpose.** Fournir une implémentation native de recherche de chemin sur graphe **quand et seulement quand** le workload observé correspond exactement au contrat.

```text
Contrat :
  entrée  : grille de coûts extraite du snapshot (BLOCK_REGION), départ, arrivée,
            fonction de coût déclarée
  sortie  : chemin (liste de positions) identique à celui du chemin de référence
  validation : comparaison stricte avec la sortie Java sur 64 exécutions

Si l'implémentation native ne reproduit pas exactement le chemin (y compris les
égalités de coût et l'ordre d'exploration), la stratégie est rejetée.
```

- R-820 : le pathfinding natif est `EXPERIMENTAL` et n'est activé que pour des workloads dont la fonction de coût est prouvée équivalente. Toute divergence d'un seul nœud est un mismatch.

**Tests.** T-540 (10^4 requêtes identiques à la référence), T-541 (départ inaccessible géré), T-542 (budget de nœuds respecté).

## 16.4 Spatial indexing

```text
Structure : grille uniforme par chunk + liste par cellule (SoA)
Construction : incrémentale, mise à jour par les commits d'entités
Requêtes supportées : k plus proches, rayon, boîte, raycast grossier
Complexité : O(1) amorti par insertion, O(k) par requête locale
Invalidation : par epoch de chunk
```

## 16.5 C-42 : Networking (rappel)

Voir §5.40. Pipeline autorisé :

```text
Paquet à émettre
   -> préparation/sérialisation (parallélisable si indépendante)
   -> compression conforme (parallélisable)
   -> file d'émission ORDONNÉE (ordre strictement préservé)
   -> envoi (thread réseau existant, inchangé)
```

## 16.6 C-52 : Lighting Engine

**Maturité : `FUTURE`.** Interfaces et tests de contrat présents, aucune implémentation. Motif : le calcul de lumière est fortement couplé à l'état du monde et déjà optimisé par plusieurs mods ; l'intégrer sans preuve d'équivalence serait une source majeure de divergence visuelle.

## 16.7 C-53 : Client Render Prep

**Maturité : `EXPERIMENTAL`.** Périmètre strictement limité à la **préparation de données** hors thread de rendu (culling géométrique, tri, construction de tampons), jamais aux appels graphiques.

- R-830 : aucun appel OpenGL n'est effectué hors du thread de rendu. Cette règle n'a aucune exception.

---

# PARTIE 17 : MOD DISCOVERY

Voir C-41 pour l'implémentation. Cette partie fixe les exigences fonctionnelles.

```text
Un mod DOIT pouvoir être analysé sans aucune intégration manuelle :
  1. détection du conteneur et de la version
  2. attribution de propriétaire aux classes chargées
  3. détection des handlers d'événements
  4. détection des threads créés
  5. détection des hotspots par profilage
  6. construction des workloads
Aucune étape ne requiert de connaissance préalable du mod.
```

Sortie type (vue mods, générée, jamais codée en dur) :

```text
MOD              CPU      HOT WORKLOADS   ANALYZED   RUST CANDIDATES   ACTIVE   UNSAFE   UNKNOWN
<modid A>       18.4 %          14           184            37            21        9        18
<modid B>        9.8 %           9            96            22            12        4        11
<modid C>        4.2 %           4            41             9             5        1         6
```

---

# PARTIE 18 : COMPATIBILITÉ

## 18.1 Forge

RF-X DOIT préserver : Forge Event Bus, Mod Event Bus, cycle de vie, registres, capabilities, networking, commandes, sauvegardes, dimensions, systèmes de mods.

- R-840 : RF-X NE DOIT PAS exiger la réécriture d'un mod.
- R-841 : RF-X NE DOIT PAS empêcher un autre mod de fonctionner ; en cas de conflit, RF-X se retire (quarantaine).

## 18.2 Sauvegardes

```text
- aucun changement du format de monde
- les données RF-X sont stockées HORS du monde, dans <gameDir>/rustforgex/
- si une donnée devait un jour être attachée au monde, elle le serait dans un
  namespace séparé, ignorable par un jeu sans RF-X (non nécessaire en V1.0)
- test obligatoire : monde créé avec RF-X, chargé sans RF-X, puis rechargé avec
  RF-X : aucune différence détectable (T-550)
```

## 18.3 Réseau

Les trois configurations DOIVENT fonctionner :

```text
client vanilla   <-> serveur RF-X
client RF-X      <-> serveur vanilla
client RF-X      <-> serveur RF-X
```

- R-842 : aucun protocole propriétaire n'est requis. Si un canal RF-X est ajouté un jour, son absence DOIT être sans effet.

## 18.4 Autres mods d'optimisation

```text
- détection de transformateurs concurrents sur les mêmes classes
- si un autre mod transforme la même méthode : FORCE_SERIAL par défaut,
  puis ALLOW si la stabilité est démontrée sur 30 minutes sans incident
- si un autre mod fournit déjà une exécution parallèle du même workload,
  RF-X ne double pas la parallélisation : il mesure et laisse faire
```

## 18.5 Verdicts (rappel)

```text
ALLOW | FORCE_JAVA | FORCE_SERIAL | FORCE_BATCH | FORCE_PARALLEL | DISABLE
```

La compatibilité s'exprime en **propriétés et fingerprints**, jamais en noms de mods côté moteur (INV-12).

---

# PARTIE 19 : SÉCURITÉ ET CONFINEMENT

## 19.1 Code `unsafe` en Rust

```text
- tout bloc unsafe DOIT être justifié par un commentaire SAFETY décrivant
  l'invariant qui le rend correct
- tout module contenant du unsafe DOIT être listé dans SECURITY.md
- le CI exécute cargo-geiger (comptage) et refuse une augmentation non justifiée
- Miri est exécuté sur les crates sans FFI (rfx-model, rfx-ir, rfx-taskgraph)
- les tests de concurrence utilisent loom sur les structures lock-free critiques
```

## 19.2 Données venant de Java

```text
Toute donnée reçue est :
  - bornée (longueur, taille, indices vérifiés)
  - validée (types, plages, cohérence)
  - versionnée (schéma)
Aucune confiance : un bug côté Java ne DOIT jamais provoquer une écriture
hors limites côté natif.
```

## 19.3 Confinement des pannes

```text
Niveau 1 : tâche          -> TaskOutcome d'échec, aucun effet
Niveau 2 : workload       -> démotion, JAVA_ONLY
Niveau 3 : sous-système   -> désactivation (ex : miroir, IR)
Niveau 4 : runtime        -> DEGRADED
Niveau 5 : arrêt          -> HALTED, jeu en Java pur
Jamais   : crash de la JVM
```

## 19.4 Watchdog

```text
- surveille : durée des tâches, durée des hooks, progression du drain,
  croissance mémoire, taux de panics, absence de progression du tick
- action graduée : alerte -> annulation -> démotion -> désactivation -> HALT
- le watchdog lui-même est simple, sans allocation, et ne peut pas bloquer
```

## 19.5 Surface d'attaque et vie privée

```text
- aucune connexion réseau initiée par RF-X (INV-15)
- aucune télémétrie externe
- les rapports exportables sont anonymisés (chemins, noms d'utilisateur)
- aucun chargement de code natif tiers en V1.0 (pas de plugins natifs externes)
- vérification SHA-256 obligatoire du binaire natif embarqué (R-300)
```

## 19.6 Reproduction d'incident

```text
Un dump d'incident contient :
  - descripteur du workload, décision, stratégie, fingerprint
  - entrées capturées (snapshot sérialisé, tronqué à une taille maximale)
  - séquences de commandes attendue et obtenue
  - versions (MC, Forge, RF-X, JVM, OS, CPU)
Il DOIT permettre de rejouer le cas hors ligne via rfx-cli.
```

---

# PARTIE 20 : STRATÉGIE DE TEST

## 20.1 Principes

```text
- tout comportement spécifié est testé
- tout invariant est testé
- tout mode de défaillance est testé par injection
- un test qui ne peut pas échouer n'est pas un test
- les tests de correction priment sur les tests de performance
```

## 20.2 Pyramide et volumétrie cible

| Niveau | Portée | Nombre cible | Durée | Exécution |
|---|---|---|---|---|
| Unitaires Rust | fonctions pures, structures | 400+ | < 60 s | à chaque commit |
| Unitaires Java | bridge, bootstrap, config | 120+ | < 60 s | à chaque commit |
| Propriété / fuzzing | IR, conflits, ordonnancement | 30 propriétés | < 5 min | à chaque commit |
| Concurrence (loom) | structures lock-free | 15 | < 10 min | nightly |
| Intégration sans jeu | pipeline complet sur données simulées | 80 | < 5 min | à chaque commit |
| Intégration Forge | serveur dédié headless scripté | 40 | < 20 min | à chaque PR |
| Gameplay | scénarios scriptés client + serveur | 25 | < 40 min | nightly |
| Stress / chaos | charge, injection de pannes | 15 | < 2 h | nightly |
| Longue durée | 8 h à 24 h | 4 | 24 h | hebdomadaire |
| Performance / non-régression | benchmarks | 20 | < 60 min | à chaque PR (sous-ensemble) + nightly (complet) |

## 20.3 Catalogue de tests (identifiants normatifs)

### 20.3.1 Fondations (T-0xx)

```text
T-001  le workspace Rust compile en debug et release
T-002  le projet Java compile et produit le JAR
T-003  aucun warning bloquant (clippy -D warnings, javac -Werror)
T-004  les modèles Rust et Java sont synchronisés (génération vérifiée)
T-005  aucune dépendance de couche inférieure vers couche supérieure (INV-13)
T-006  aucun TODO/FIXME/unimplemented dans un module marqué STABLE
T-007  toutes les options de configuration ont un défaut, une plage, une doc
T-008  tous les codes d'erreur sont documentés et uniques
T-009  la licence et les NOTICE sont présents et cohérents
T-010  le JAR ne contient aucun asset tiers
T-011  cohérence des ClassificationFlags (table de vérité)
```

### 20.3.2 Composants (T-1xx à T-5xx)

Les tests par composant sont listés dans chaque fiche en PARTIE 5. Récapitulatif des plages :

```text
T-100..T-103  C-01 Forge Integration
T-110..T-114  C-02 Bootstrap
T-120..T-123  C-03 Native Loader
T-130..T-134  C-04 Instrumentation
T-140..T-144  C-05 Profiler
T-150..T-154  C-06 Event Observer
T-160..T-164  C-07 Call Graph
T-170..T-172  C-08 Workload Profiler
T-180..T-184  C-09 Workload DB
T-190..T-194  C-10 Dependency Analyzer
T-200..T-205  C-11 Data Flow Analyzer
T-210..T-212  C-12 Thread Analyzer
T-220..T-223  C-13 Determinism Analyzer
T-230..T-232  C-14 Risk Engine
T-240..T-245  C-15 Decision Engine
T-250..T-253  C-16 Task Graph
T-260..T-267  C-17 Scheduler
T-270..T-273  C-18 Worker Runtime
T-280..T-284  C-19 Snapshot
T-290..T-294  C-20 World Mirror
T-300..T-302  C-21 Command Buffer
T-310..T-316  C-22 Commit Engine
T-320..T-323  C-23 Conflict Detection
T-330..T-334  C-24 Validation
T-340..T-342  C-25 Shadow Execution
T-350..T-353  C-26 Rollback
T-360..T-364  C-27 Runtime Core
T-370..T-373  C-31 Memory
T-380..T-384  C-32 Cache
T-390..T-392  C-33 Compatibility
T-400..T-402  C-34 Telemetry
T-410..T-412  C-35 Diagnostics
T-420..T-422  C-38 CLI
T-430..T-432  C-39 SDK
T-440..T-442  C-41 Mod Discovery
T-450..T-452  C-42 Network
T-460..T-461  C-43 Explainability
T-470..T-473  C-44 Learning
T-480..T-482  C-45 Hardware Probe
T-490..T-491  C-46 Dashboard
T-500..T-502  C-48 Persistence
T-510..T-519  C-28/29/30 IR, passes, backend
T-520..T-522  C-49 Entity Engine
T-530..T-532  C-50 Chunk Engine
T-540..T-542  C-51 Pathfinding
T-550         compatibilité de sauvegarde
```

### 20.3.3 Tests de correction transversaux (T-6xx)

```text
T-600  équivalence séquentiel/parallèle sur 1000 scénarios déterministes
T-601  identité des résultats avec 1, 2, 4, 8, 16 workers
T-602  identité des séquences de commandes sur 1000 exécutions
T-603  aucune divergence d'événement (trace comparée)
T-604  aucune modification hors write set déclaré (vérification par diff de monde)
T-605  round-trip de sauvegarde identique
T-606  aucune fuite mémoire native sur 10^6 tâches
T-607  aucun deadlock sous parallélisme imbriqué profond (10^5 itérations)
T-608  résistance à l'injection de panic à chaque point d'entrée FFI
T-609  résistance à l'injection d'exception dans chaque hook Java
T-610  résistance au retrait brutal du natif (simulation d'échec de chargement)
T-611  chaos : annulation aléatoire de 10 % des tâches, jeu toujours correct
T-612  chaos : rejet aléatoire de 20 % des command buffers, jeu toujours correct
T-613  horloge non monotone simulée
T-614  disque plein pendant la persistance
T-615  changement de fingerprint en cours de partie (rechargement de mod simulé)
```

### 20.3.4 Tests de gameplay (T-4xx gameplay, scriptés)

```text
G-01  démarrage serveur dédié, chargement du monde, 5 minutes de tick à vide
G-02  connexion, déconnexion, reconnexion d'un client
G-03  génération de 2000 chunks, comparaison NBT avec référence
G-04  téléportation entre dimensions
G-05  combat contre 200 entités hostiles
G-06  circuit de redstone complexe (horloge, comparateurs, pistons) sur 10 000 ticks
G-07  automatisation lourde (machines de mods) sur 30 minutes
G-08  sauvegarde, arrêt, rechargement, comparaison d'état
G-09  chargement/déchargement massif de chunks (joueur en mouvement rapide)
G-10  20 joueurs simulés en mouvement
G-11  commandes vanilla et de mods
G-12  crafting, inventaires, conteneurs
G-13  pathfinding de 500 entités
G-14  météo, cycle jour/nuit, événements planifiés
G-15  client : parcours scripté avec mesure de frametime
```

Chaque scénario est exécuté **deux fois** : avec RF-X désactivé (référence) et avec RF-X actif. Le critère est l'**égalité d'état** et l'absence d'erreur, pas la performance.

### 20.3.5 Tests de stress (T-7xx)

```text
T-700  8 heures continues, profil heavy, surveillance mémoire et dérive
T-701  1 mod, 10, 50, 100, 250, 500 mods synthétiques
T-702  10^7 tâches soumises
T-703  100 000 entités
T-704  saturation CPU externe (charge concurrente 100 %)
T-705  mémoire limitée (-Xmx4G sur profil heavy)
T-706  disque lent (IO throttling)
T-707  sur-souscription : 4 pools de threads tiers actifs
```

### 20.3.6 Critères de réussite génériques

| Type de test | Critère |
|---|---|
| correction | égalité stricte avec la référence, zéro exception non attendue |
| concurrence | zéro deadlock, zéro course détectée, résultats identiques |
| stress | aucune dégradation cumulative, mémoire stable, aucun crash |
| performance | pas de régression au-delà du seuil, gains reproductibles |
| chaos | le jeu reste jouable et correct, dégradation propre |

## 20.4 Infrastructure de test

```text
- serveur dédié headless piloté par script (stdin/commandes) pour l'intégration
- client automatisé : mode "replay" pilotant les entrées, capture de frametime
- mods synthétiques générés (tools/modgen) : ils produisent des workloads
  paramétrables (CPU, allocations, écritures, threads) SANS dépendre de mods
  tiers, ce qui rend la CI reproductible et légalement propre
- comparateur d'état de monde : diff NBT structuré, ignorant les champs
  volontairement non déterministes de Minecraft (identifiés et documentés)
```

- R-850 : la suite de tests NE DOIT PAS dépendre du téléchargement de mods tiers. Les profils réalistes sont approchés par des mods synthétiques et, pour les tests manuels, par des modpacks installés par l'utilisateur.

---

# PARTIE 21 : BENCHMARKS

## 21.1 Ce qui est mesuré

```text
Serveur : TPS, MSPT (p50, p95, p99, max), temps par phase, temps de commit,
          allocations/tick, GC (comptes et pauses), latence de chunk,
          overhead RF-X, nombre de traversées FFI
Client  : FPS moyen, frametime p50/p95/p99, 1 % low, 0,1 % low, temps de
          préparation de rendu, allocations, overhead RF-X
Micro   : coût de soumission, coût de vol, coût de snapshot par octet,
          coût FFI, coût de commit par commande, coût des passes IR
```

## 21.2 Configurations comparées (obligatoires)

```text
A. Vanilla + Forge
B. Vanilla + Forge + mods (profil donné)
C. B + RUSTFORGE-X en mode OBSERVE_ONLY      (coût pur du profilage)
D. B + RUSTFORGE-X en mode SAFE
E. B + RUSTFORGE-X en mode BALANCED
F. B + RUSTFORGE-X en mode PERFORMANCE
G. B + RUSTFORGE-X + systèmes SDK natifs (si applicable)
```

Le passage de B à C mesure le **coût** du système. Le passage de C à E mesure le **bénéfice net**.

## 21.3 Méthodologie (normative)

```text
1. matériel et JVM figés, documentés dans le résultat
2. graine de monde figée, monde pré-généré fourni par le harnais
3. échauffement : 3 minutes ignorées (JIT + chargement)
4. mesure : 10 minutes minimum, ou 12 000 ticks
5. répétitions : 5 exécutions indépendantes (processus relancé)
6. statistiques : médiane et intervalle interquartile ; jamais la moyenne seule
7. rejet d'exécution : si l'écart-type inter-répétitions > 10 %, l'exécution est
   invalidée et relancée (avec journal de la cause : charge externe probable)
8. les ticks contaminés par une pause GC majeure sont marqués (pas supprimés)
   et rapportés séparément
9. tout résultat publié cite : version RF-X, commit, profil, matériel, JVM, date
```

- R-860 : **aucune performance ne doit être inventée.** Un chiffre non produit par le harnais ne DOIT pas apparaître dans la documentation, le README, ou l'interface.
- R-861 : le format de résultat est JSON, versionné, stocké dans `benchmarks/results/`.

Format de résultat :

```json
{
  "schema": 1,
  "rfx_version": "1.0.0",
  "commit": "<sha>",
  "profile": "heavy",
  "config": "E",
  "hardware": { "cpu": "...", "cores": 16, "ram_gb": 32 },
  "jvm": { "vendor": "...", "version": "17.0.x", "flags": ["-Xmx8G"] },
  "runs": 5,
  "metrics": {
    "mspt_p50": { "median": 0.0, "iqr": 0.0, "unit": "ms" },
    "mspt_p95": { "median": 0.0, "iqr": 0.0, "unit": "ms" },
    "tps":      { "median": 0.0, "iqr": 0.0, "unit": "tps" }
  },
  "notes": []
}
```

## 21.4 Non-régression

```text
- chaque PR exécute un sous-ensemble rapide (profil light + medium, 3 minutes)
- seuil d'échec : régression > 3 % sur MSPT p95 ou > 5 % sur frametime p99
- le nightly exécute la suite complète et met à jour la courbe historique
- une amélioration de la moyenne accompagnée d'une dégradation du 1 % low
  est traitée comme une régression (R-792)
```

## 21.5 Benchmarks micro obligatoires

```text
B-01  soumission de tâche (ns)
B-02  vol de tâche (ns)
B-03  aller-retour FFI minimal (ns)
B-04  transfert de 1 Kio / 64 Kio / 1 Mio (ns)
B-05  snapshot ENTITY_SET par entité (ns)
B-06  détection de conflit par buffer (ns)
B-07  application d'une commande (ns)
B-08  interprétation IR par instruction (ns)
B-09  compilation IR d'une fonction moyenne (µs)
B-10  hash de fingerprint (ns)
```

---

# PARTIE 22 : PROFILS DE MODPACK DE TEST

Les profils décrivent des **charges**, pas des listes de mods tiers. Chaque profil est réalisable :

1. avec des mods synthétiques générés (CI, reproductible, sans dépendance légale) ;
2. avec un modpack réel installé par l'utilisateur (tests manuels, documentés dans `BENCHMARKS.md`).

| Profil | Charge visée | Mods synthétiques | Entités | Chunks chargés | Objectif |
|---|---|---|---|---|---|
| `vanilla` | référence | 0 | 200 | 400 | mesurer le coût pur de RF-X |
| `light` | faible | 10 | 500 | 600 | validation rapide en CI |
| `medium` | moyenne | 60 | 2 000 | 1 200 | profil de PR |
| `heavy` | forte | 150 | 8 000 | 2 500 | profil principal de benchmark |
| `very_heavy` | très forte | 300 | 20 000 | 4 000 | scalabilité |
| `stress` | extrême | 500 | 100 000 | 6 000 | limites et dégradation |

Chaque mod synthétique peut être paramétré :

```text
- coût CPU par tick (boucle de calcul pur)
- taux d'allocation
- écritures dans le monde (blocs, entités)
- utilisation de threads propres
- handlers d'événements (nombre, priorité, annulation)
- usage de réflexion
- non-déterminisme (RNG, temps)
```

- R-870 : les profils DOIVENT inclure au moins un mod synthétique fortement multithreadé et un mod synthétique non déterministe, afin de vérifier que RF-X les traite correctement (refus ou sérialisation) plutôt que de les casser.
- R-871 : aucun profil ne DOIT être conçu pour optimiser un mod nommé.

---

# PARTIE 23 : BUILD

## 23.1 Prérequis

```text
JDK 17 (toolchain Gradle, téléchargée automatiquement si absente)
Gradle 8.x (wrapper fourni)
ForgeGradle 6.x
Rust stable (version épinglée par rust-toolchain.toml, ex : 1.7x)
cargo, rustfmt, clippy
cbindgen (optionnel, génération d'en-têtes de contrôle)
cibles : x86_64-pc-windows-msvc | x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu
```

## 23.2 Organisation du build

```text
Gradle est le point d'entrée unique.
La tâche :buildNative appelle cargo pour chaque cible activée.
Les artefacts natifs sont copiés dans src/main/resources/natives/<os>-<arch>/
avec leur fichier .sha256, puis empaquetés dans le JAR.
```

`build.gradle` (extrait normatif) :

```groovy
tasks.register('buildNative', Exec) {
    workingDir rootDir
    commandLine 'cargo', 'build', '--release', '--target', project.nativeTarget
}
tasks.register('copyNative', Copy) {
    dependsOn 'buildNative'
    from "target/${project.nativeTarget}/release/${project.nativeLibName}"
    into "src/main/resources/natives/${project.nativeDir}"
}
tasks.register('hashNative') { dependsOn 'copyNative' /* écrit le .sha256 */ }
processResources.dependsOn 'hashNative'
```

## 23.3 Commandes exactes

```text
# nettoyage complet
./gradlew clean && cargo clean

# compilation
./gradlew compileJava
cargo build --workspace

# compilation avec JIT expérimental
cargo build --workspace --features jit

# tests
cargo test --workspace
./gradlew test

# tests étendus (nightly)
cargo test --workspace -- --ignored
cargo miri test -p rfx-model -p rfx-ir -p rfx-taskgraph
cargo test -p rfx-scheduler --features loom

# lint
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
./gradlew spotlessCheck

# build complet du JAR (natif inclus pour la plateforme hôte)
./gradlew build

# build multiplateforme (nécessite les toolchains croisées ou la CI)
./gradlew buildAll

# package final signé et hashé
./gradlew package

# benchmarks
cargo bench -p rfx-bench
./gradlew benchmark -Pprofile=heavy -Pconfig=E

# release (depuis un dépôt propre, sur un tag)
./gradlew release -Pversion=1.0.0
```

## 23.4 Reproductibilité

```text
- versions épinglées : Gradle wrapper, ForgeGradle, rust-toolchain.toml, Cargo.lock
- SOURCE_DATE_EPOCH utilisé, entrées de JAR triées, timestamps normalisés
- objectif : deux builds du même commit produisent des JAR identiques bit à bit
  (hors signature). Test T-900 en CI compare deux builds successifs.
```

## 23.5 Gestion des cibles manquantes

```text
Si une cible native n'est pas disponible au build :
   - le JAR est produit sans ce natif
   - les métadonnées listent les plateformes incluses
   - au runtime, une plateforme absente conduit à DEGRADED avec message explicite
Un build de release EXIGE au minimum windows-x86_64 et linux-x86_64 (R-880).
```

---

# PARTIE 24 : CI/CD

## 24.1 Pipelines

| Pipeline | Déclencheur | Contenu | Durée cible |
|---|---|---|---|
| `pr` | pull request | fmt, clippy, build, tests unitaires + intégration sans jeu, tests Forge courts, bench rapide | < 25 min |
| `commit` | push sur la branche principale | pipeline `pr` + build multiplateforme + artefact | < 40 min |
| `nightly` | planifié | suite complète : gameplay, stress, loom, miri, benchmarks complets, longue durée courte (2 h) | < 6 h |
| `rc` | tag `vX.Y.Z-rc.N` | nightly + vérification d'artefact + test d'installation propre | < 8 h |
| `stable` | tag `vX.Y.Z` | pipeline `rc` + publication des artefacts et des sommes | < 8 h |

## 24.2 Jobs obligatoires

```text
fmt              cargo fmt --check, spotlessCheck
lint             clippy -D warnings, javac -Werror
lint-no-fiction  refuse TODO/FIXME/unimplemented!/todo!/"placeholder"
                 dans tout fichier appartenant à un module marqué STABLE
lint-no-modnames refuse tout littéral de nom de mod dans crates/rfx-decision,
                 rfx-analyzer, rfx-exec (INV-12)
build            build Rust + Java, toutes cibles
test-unit        cargo test, gradle test
test-integration pipeline complet sur données simulées
test-forge       serveur dédié headless, scénarios G-01..G-08
bench-quick      profils light et medium, comparaison au historique
artifact-verify  vérifie le JAR : présence des natifs, hashes, mods.toml,
                 absence d'assets tiers, taille attendue
security         cargo-audit, cargo-deny (licences), cargo-geiger (unsafe)
docs             vérifie que toutes les options de config et tous les codes
                 d'erreur sont documentés
```

## 24.3 Règles de blocage

```text
Un merge est BLOQUÉ si :
  - un test échoue
  - un lint échoue
  - une régression de performance dépasse le seuil (§21.4)
  - la couverture de test des crates critiques descend sous 70 %
  - un module STABLE contient une fiction (TODO, placeholder)
  - une nouvelle dépendance a une licence non approuvée
```

## 24.4 Artefacts de CI

```text
- JAR par plateforme et JAR universel
- rapports de test (JUnit XML, cargo-nextest JSON)
- résultats de benchmark (JSON) + graphique historique
- rapport de couverture
- SBOM (CycloneDX) des dépendances Rust et Java
```

---

# PARTIE 25 : RELEASE ET DISTRIBUTION

## 25.1 Versionnement

```text
SemVer : MAJOR.MINOR.PATCH
MAJOR : changement d'ABI, de schéma de cache non migrable, ou de comportement
MINOR : nouvelles fonctionnalités compatibles
PATCH : corrections
Suffixes : -rc.N pour les candidates
La version Minecraft/Forge cible est indiquée dans le nom du fichier :
   rustforgex-1.0.0-mc1.20.1-forge47.jar
```

## 25.2 Contenu d'une release

```text
rustforgex-<version>-mc1.20.1-forge47.jar
rustforgex-<version>-sources.jar            (optionnel)
SHA256SUMS
SHA256SUMS.asc                              (si signature GPG configurée)
CHANGELOG.md (extrait de la version)
LICENSE
NOTICE
README.md
```

## 25.3 Processus complet (depuis un dépôt propre)

```text
 1. git clone && git checkout <tag>
 2. vérifier que l'arbre est propre (aucune modification locale)
 3. ./gradlew clean
 4. mettre à jour CHANGELOG.md (section de version, date, contenu)
 5. ./gradlew buildAll            (natifs windows-x64, linux-x64, linux-arm64)
 6. ./gradlew test                (suite complète)
 7. ./gradlew benchmark -Pprofile=heavy   (résultats joints à la release)
 8. ./gradlew package             (JAR final, hashes)
 9. ./gradlew verifyArtifact      (contrôles automatiques, voir 25.4)
10. test d'installation propre (PARTIE 26) sur Windows et Linux
11. signature (facultative mais recommandée)
12. publication
```

- R-890 : une release NE DOIT PAS être publiée si un seul test échoue ou si `verifyArtifact` signale une anomalie.

## 25.4 Vérification d'artefact

```text
[ ] le JAR se charge sur Forge 47.x client et serveur
[ ] mods.toml présent, modid, version, dépendances correctes
[ ] natifs présents pour les plateformes annoncées, hashes valides
[ ] aucun fichier tiers (assets, mods, bibliothèques non déclarées)
[ ] licences des dépendances embarquées présentes dans NOTICE
[ ] taille du JAR dans la plage attendue (alerte si écart > 20 %)
[ ] version dans le manifeste == version du tag
[ ] aucune classe de test embarquée
```

## 25.5 Distribution

Ce que le projet distribue :

```text
- son propre code source
- ses propres binaires natifs
- sa documentation
```

Ce que le projet NE distribue PAS :

```text
- Minecraft, ses assets, ses bibliothèques
- Forge (dépendance, non redistribuée)
- des mods tiers, des modpacks, des mondes contenant du contenu tiers
```

Plateformes envisagées :

| Plateforme | Exigences de packaging |
|---|---|
| GitHub Releases | artefacts + SHA256SUMS + notes de version |
| Modrinth | métadonnées de projet, versions de jeu et de loader, changelog, licence déclarée |
| CurseForge | métadonnées équivalentes, fichier principal, changelog |

- R-891 : les exigences exactes de chaque plateforme DOIVENT être vérifiées sur leur documentation officielle au moment de la publication. Ce document ne les invente pas et ne les fige pas.
- R-892 : la licence du projet DOIT être choisie et déclarée avant la première publication publique (ADR-010) ; le fichier `LICENSE` doit être cohérent avec les licences des dépendances (vérifié par `cargo-deny`).

---

# PARTIE 26 : INSTALLATION

## 26.1 Procédure utilisateur

```text
1. installer Minecraft Java Edition 1.20.1
2. installer Forge 47.x
3. déposer rustforgex-<version>-mc1.20.1-forge47.jar dans mods/
4. lancer le jeu
5. au premier lancement, RF-X extrait son binaire natif et sonde le matériel
6. le fichier de configuration est créé dans <gameDir>/rustforgex/config/
```

## 26.2 Test d'installation propre (obligatoire avant release)

```text
[ ] machine ou conteneur vierge
[ ] Minecraft 1.20.1 + Forge 47.x installés
[ ] ajout du JAR RF-X seul (aucun autre mod)
[ ] lancement du client : aucune erreur, aucun avertissement rouge
[ ] création d'un monde
[ ] 10 minutes de jeu : marche, casse, pose, combat
[ ] sauvegarde et quitte
[ ] rechargement du monde : état correct
[ ] /rfx status répond
[ ] arrêt propre : aucun thread résiduel, aucun fichier verrouillé
[ ] même procédure sur serveur dédié + client vanilla connecté
[ ] désinstallation : retrait du JAR, le monde se recharge sans RF-X
```

- R-900 : la désinstallation DOIT être sans conséquence. Un monde joué avec RF-X DOIT rester parfaitement jouable sans RF-X.

## 26.3 Diagnostic d'installation

En cas de problème, RF-X écrit un fichier `rustforgex/logs/boot.log` contenant :

```text
plateforme, arch, JVM, version Forge, chemin d'extraction, résultat de chargement,
hash du natif, résultat du handshake ABI, état final, cause de dégradation
```

---

# PARTIE 27 : DOCUMENTATION DU DÉPÔT

## 27.1 Fichiers obligatoires

| Fichier | Contenu minimal |
|---|---|
| `README.md` | ce qu'est le projet, ce qu'il n'est pas, état de maturité, installation, liens |
| `ARCHITECTURE.md` | couches, composants, flux, diagrammes, décisions clés |
| `BUILDING.md` | prérequis, commandes exactes, cibles, dépannage de build |
| `INSTALLATION.md` | procédure utilisateur, prérequis, désinstallation |
| `CONFIGURATION.md` | toutes les options, types, plages, défauts, effets |
| `COMPATIBILITY.md` | comportement face aux autres mods, verdicts, quarantaine, limites |
| `TROUBLESHOOTING.md` | symptômes, causes, actions, comment produire un rapport |
| `BENCHMARKS.md` | méthodologie, profils, résultats mesurés, comment reproduire |
| `RELEASING.md` | processus de release pas à pas, checklist |
| `SECURITY.md` | modèle de menace, unsafe, absence de réseau, signalement |
| `RUST_MOD_SDK.md` | guide du SDK, contrats, exemple complet |
| `docs/AGENT.md` | voir 27.2 |
| `docs/decisions/` | un ADR par décision structurante |
| `docs/spec/` | copie versionnée de ce cahier des charges |

- R-910 : toute affirmation chiffrée dans la documentation DOIT provenir d'un résultat de benchmark versionné (R-860).

## 27.2 docs/AGENT.md

Ce fichier DOIT permettre à un agent de reprendre le projet sans aucune conversation antérieure. Contenu obligatoire :

```text
1. Carte du dépôt : quel code est où, quel crate implémente quel composant
2. Comment construire, tester, lancer, benchmarker (commandes exactes)
3. Comment lancer un serveur de test et un client de test
4. Où sont les identifiants normatifs (R-xxx, C-xx, T-xxx) et comment les citer
5. Ordre d'implémentation recommandé (jalons M0..M10)
6. Invariants à ne jamais violer (liste INV-xx recopiée)
7. Procédure de debug : logs, dumps, /rfx why, rfx-cli
8. Procédure quand un test échoue : comment reproduire, comment isoler
9. Règles de commit et de PR (format, identifiants, tests requis)
10. Ce qu'il ne faut jamais faire (liste explicite, voir FINAL AGENT EXECUTION CONTRACT)
11. Comment ajouter un composant, une métrique, une option, un test
12. État courant : ce qui est STABLE, EXPERIMENTAL, DISABLED, FUTURE
```

## 27.3 ADR

Format :

```text
# ADR-XXX : <titre>
Statut : proposé | accepté | remplacé par ADR-YYY
Contexte : ...
Options envisagées : ... (avec avantages et inconvénients)
Décision : ...
Conséquences : ... (positives et négatives)
Date, auteur
```

ADR initiaux obligatoires :

```text
ADR-001  Forge comme plateforme initiale, abstraction du loader
ADR-002  Java reste l'état autoritatif
ADR-003  Modèle command buffer + commit ordonné
ADR-004  JNI + DirectByteBuffer plutôt que Panama sur Java 17
ADR-005  Work stealing Chase-Lev, pool unique
ADR-006  Domaines abstraits pour les read/write sets
ADR-007  Fingerprint complet comme clé de tout cache
ADR-008  RF-IR restreint plutôt que traduction générale
ADR-009  Cranelift comme backend optionnel
ADR-010  Choix de licence
ADR-011  Modèle d'apprentissage inspectable uniquement
ADR-012  Aucune télémétrie réseau
```

---

# PARTIE 28 : CONFIGURATION

## 28.1 Fichier

`<gameDir>/rustforgex/config/rustforgex.toml`, format TOML, commenté, créé avec les valeurs par défaut au premier lancement.

## 28.2 Contenu complet (référence normative)

```toml
schema = 1

[general]
enabled            = true          # false = RF-X ne fait rien du tout
mode               = "balanced"    # safe | balanced | performance | experimental | debug
side_client        = true
side_server        = true
warmup_ticks       = 1200          # avant toute décision d'activation

[profiler]
enabled            = true
cpu_budget_pct     = 2.0           # % d'un cœur
memory_budget_mb   = 64
sample_hz          = 100
context_depth      = 3             # 1..8
max_workloads      = 20000
probe_regression_threshold_pct = 15.0

[analysis]
enabled            = true
cpu_budget_pct     = 5.0
max_call_depth     = 12
max_nodes_per_workload = 2000
max_pairs_per_tick = 20000

[decision]
min_hold_ticks     = 600
revisit_ticks      = 1200
seuil_activation_ns = 50000        # surchargé par le mode
max_correctness_risk = 0.15        # surchargé par le mode
mainthread_weight  = 3.0

[scheduler]
workers            = 0             # 0 = automatique
max_workers        = 32
oversubscription_threshold = 1.5
min_task_ns        = 20000
batch_max          = 1024
queue_high_watermark = 4096
queue_hard_limit   = 8192
nested_depth_max   = 4

[snapshot]
budget_bytes_per_tick = 4194304
max_block_region   = [32, 32, 32]

[commit]
max_ns_per_tick    = 2000000
max_commands_per_buffer = 4096
max_conflict_checks = 100000

[validation]
shadow_runs        = 64
canary_ticks       = 300
canary_traffic_pct = 10
conflict_threshold_pct = 2.0
demote_conflict_pct = 5.0
cooldown_minutes   = 30

[shadow]
enabled            = true
max_cpu_pct        = 5.0

[memory]
max_native_mb      = 512
arena_initial_mb   = 1
arena_max_mb       = 16

[cache]
enabled            = true
max_disk_mb        = 256
max_memory_mb      = 128
result_cache_ttl_ticks = 200
purge_on_version_change = true

[mirror]
enabled            = false         # EXPERIMENTAL
max_chunks         = 256
max_memory_mb      = 128
audit_rate_hz      = 1
max_invalidation_rate_pct = 30

[ir]
enabled            = false         # EXPERIMENTAL
jit                = false         # EXPERIMENTAL, nécessite la feature de build
max_steps          = 10000000
max_function_size  = 4096

[network]
enabled            = false         # EXPERIMENTAL

[learning]
enabled            = true
exploration_pct    = 2.0           # 0 en mode safe
reset_on_drift     = true

[telemetry]
enabled            = true
tick_log           = false         # mode DEBUG uniquement

[diagnostics]
report_on_incident = true
keep_days          = 7
max_disk_mb        = 256

[ui]
dashboard_key      = "F6"
enabled            = true

[overrides]
# Contrôle utilisateur explicite. C'est le SEUL endroit où un nom de mod
# peut apparaître. Le moteur, lui, ne raisonne jamais sur les noms.
# force_java   = ["some_modid"]
# disable      = ["some_modid:some.Class#method"]
# force_serial = []
```

## 28.3 Modes

| Mode | `max_correctness_risk` | TL max | Exploration | Shadow | IR |
|---|---|---|---|---|---|
| `safe` | 0.05 | TL3 | 0 % | oui | non |
| `balanced` | 0.15 | TL4 | 2 % | oui | non |
| `performance` | 0.30 | TL4 | 5 % | oui | non |
| `experimental` | 0.30 | TL6 | 5 % | oui | oui |
| `debug` | 0.15 | TL4 | 0 % | oui | selon config |

- R-920 : `debug` active les assertions d'invariants, la journalisation par tick et l'instrumentation maximale ; il n'est pas destiné au jeu normal et l'indique clairement.

## 28.4 Surcharges

```text
Priorité (du plus fort au plus faible) :
  1. propriétés système -Drustforgex.<section>.<clé>=<valeur>
  2. commandes /rfx set (persistées si demandé)
  3. fichier rustforgex.toml
  4. valeurs par défaut
```

## 28.5 Validation

```text
- toute valeur hors plage est rejetée avec un message précis et remplacée par
  le défaut (jamais un comportement indéfini)
- une clé inconnue est conservée dans le fichier et signalée dans les logs
- le rechargement à chaud (/rfx reload) s'applique aux seuils et budgets ;
  les clés structurelles sont marquées "redémarrage requis"
```

---

# PARTIE 29 : DEFINITION OF DONE

## 29.1 Component done

Un composant `C-xx` est terminé si et seulement si :

```text
[ ] spécifié dans ce document et implémenté conformément
[ ] toutes ses interfaces publiques sont documentées (rustdoc / javadoc)
[ ] ses modes de défaillance sont implémentés ET testés par injection
[ ] son fallback est implémenté ET testé
[ ] ses métriques sont émises et visibles via /rfx status ou le dashboard
[ ] ses tests listés en PARTIE 5 passent
[ ] couverture de test >= 70 % des lignes du module (80 % pour L2 et L4)
[ ] aucun TODO/FIXME/unimplemented dans le module s'il est marqué STABLE
[ ] son niveau de maturité est déclaré dans le code et dans la config
[ ] ses options de configuration sont documentées dans CONFIGURATION.md
[ ] ses codes d'erreur sont enregistrés dans l'annexe A.2
```

## 29.2 Feature done

Une fonctionnalité (ex : "offload parallèle des workloads purs") est terminée si :

```text
[ ] tous les composants qu'elle mobilise sont "component done"
[ ] elle traverse le pipeline complet OBSERVE..LEARN sans intervention manuelle
[ ] elle possède au moins un test de bout en bout sur serveur dédié
[ ] elle possède au moins un test de correction comparant à la référence
[ ] elle possède un benchmark mesurant son gain, résultat versionné
[ ] elle est explicable via /rfx why
[ ] elle est démotable et son rollback est testé
[ ] elle est désactivable par configuration
[ ] elle n'introduit aucune régression au-delà des seuils de la PARTIE 21
```

## 29.3 Milestone done

```text
[ ] toutes les fonctionnalités du jalon sont "feature done"
[ ] le JAR produit se charge, se joue et se désinstalle proprement
[ ] la suite de tests du jalon passe intégralement en CI
[ ] les benchmarks du jalon sont exécutés et archivés
[ ] la documentation est à jour (README, ARCHITECTURE, CONFIGURATION, AGENT)
[ ] le CHANGELOG contient les entrées du jalon
[ ] aucune fiction dans les modules STABLE
[ ] les ADR des décisions prises pendant le jalon sont écrits
```

## 29.4 Release candidate

```text
[ ] jalon courant "milestone done"
[ ] pipeline rc vert (nightly complet + vérification d'artefact)
[ ] test d'installation propre réussi sur Windows x64 et Linux x64
[ ] test long 8 heures sans dérive mémoire ni incident
[ ] tests de gameplay G-01..G-15 passés avec égalité d'état
[ ] benchmarks publiés avec méthodologie et matériel documentés
[ ] liste des limitations connues rédigée
[ ] tous les EXPERIMENTAL désactivés par défaut et signalés
```

## 29.5 Stable release

```text
[ ] tous les critères de release candidate
[ ] au moins une RC ayant vécu 7 jours sans défaut bloquant signalé
[ ] zéro défaut ouvert de sévérité critique ou majeure
[ ] critères d'acceptation de la PARTIE 31 tous cochés
[ ] LICENSE, NOTICE, SECURITY.md complets et exacts
[ ] SHA256SUMS publiés
[ ] procédure de rollback utilisateur documentée (retirer le JAR)
```

## 29.6 Définitions de sévérité

| Sévérité | Définition | Action |
|---|---|---|
| critique | corruption de monde ou de sauvegarde, crash JVM, perte de données | bloque toute release, correction immédiate |
| majeure | divergence de comportement, deadlock, régression > 10 % | bloque la release stable |
| mineure | régression < 10 %, message incorrect, métrique fausse | corrigée dans la version suivante |
| cosmétique | formulation, mise en page du dashboard | backlog |

---

# PARTIE 30 : ROADMAP ET JALONS

Chaque jalon produit un **JAR installable et jouable**. Aucun jalon ne laisse le projet dans un état non fonctionnel.

## M0 : Bootstrap (fondations)

```text
Contenu   : C-01, C-02, C-03, C-27 (squelette), C-37, C-45, C-40 (build+CI)
Livrable  : JAR Forge qui se charge, charge le natif, sonde le matériel,
            expose /rfx status, ne fait rien d'autre
DoD       : T-001..T-010, T-100..T-103, T-110..T-114, T-120..T-123, T-480..T-482
Sortie    : le jeu tourne exactement comme sans le mod, overhead mesuré ~0
```

## M1 : Observation

```text
Contenu   : C-04, C-05, C-06, C-31, C-34, C-35, C-38, C-41, C-36 (harnais)
Livrable  : profilage adaptatif complet, dashboard textuel, rapports
DoD       : T-130..T-134, T-140..T-144, T-150..T-154, T-370..T-373,
            T-400..T-402, T-410..T-412, T-420..T-422, T-440..T-442
Sortie    : "je sais ce que fait ce modpack", overhead < 2 % mesuré et affiché
```

## M2 : Analyse et ordonnancement

```text
Contenu   : C-07, C-08, C-09, C-10, C-12, C-16, C-17, C-18, C-48
Livrable  : workloads modélisés, task graph, scheduler opérationnel utilisé
            uniquement pour des tâches internes (analyse, persistance)
DoD       : T-160..T-164, T-170..T-172, T-180..T-184, T-190..T-194,
            T-210..T-212, T-250..T-253, T-260..T-267, T-270..T-273
Sortie    : ordonnancement prouvé correct, aucune transformation du jeu encore
```

## M3 : Décision

```text
Contenu   : C-11, C-13, C-14, C-15, C-32, C-33, C-43
Livrable  : décisions calculées et expliquées, mais appliquées uniquement
            pour TL0..TL2 (JAVA_ONLY, JAVA_BATCH)
DoD       : T-200..T-205, T-220..T-223, T-230..T-232, T-240..T-245,
            T-380..T-384, T-390..T-392, T-460..T-461
Sortie    : premiers gains réels par batching, /rfx why complet
```

## M4 : Exécution sûre

```text
Contenu   : C-19, C-21, C-22, C-23, C-24, C-25, C-26
Livrable  : offload réel des workloads purs vers les workers Rust,
            snapshot, command buffer, commit ordonné, validation, rollback
DoD       : T-280..T-284, T-300..T-302, T-310..T-316, T-320..T-323,
            T-330..T-334, T-340..T-342, T-350..T-353, T-600..T-615
Sortie    : premier RUST_OFFLOAD validé en production, gain mesuré
```

## M5 : Adaptation et domaine entités

```text
Contenu   : C-44, C-46, C-49, C-20 (EXPERIMENTAL)
Livrable  : apprentissage local, dashboard graphique, pipeline entités,
            world mirror désactivé par défaut
DoD       : T-470..T-473, T-490..T-491, T-520..T-522, T-290..T-294
Sortie    : le système s'améliore seul, gains mesurables sur profil heavy
```

## M6 : IR et chunks

```text
Contenu   : C-28, C-29, C-50, C-51
Livrable  : lifting bytecode vers RF-IR, interpréteur IR, vérificateur,
            pipeline chunk et pathfinding (EXPERIMENTAL)
DoD       : T-510..T-519, T-530..T-532, T-540..T-542
Sortie    : premiers workloads exécutés hors JVM avec égalité prouvée
```

## M7 : Compilation et client

```text
Contenu   : C-30 (feature jit), C-42, C-53
Livrable  : backend Cranelift optionnel, préparation de rendu, réseau
DoD       : fuzzing IR/compilé, T-450..T-452, benchmarks client
Sortie    : chemin natif complet, désactivé par défaut
```

## M8 : SDK

```text
Contenu   : C-39
Livrable  : SDK Rust documenté, système d'exemple, vérification des
            déclarations à l'exécution
DoD       : T-430..T-432, RUST_MOD_SDK.md complet
Sortie    : écriture de systèmes natifs possible
```

## M9 : Durcissement

```text
Contenu   : tests longs, chaos, compatibilité, documentation finale
Livrable  : release candidate
DoD       : PARTIE 29.4 intégralement
Sortie    : rustforgex-1.0.0-rc.1
```

## M10 : Stabilisation

```text
Contenu   : corrections, benchmarks publiés, publication
Livrable  : release stable
DoD       : PARTIE 29.5 et PARTIE 31 intégralement
Sortie    : rustforgex-1.0.0
```

## 30.1 Correspondance avec les phases du V0.3

| Phase V0.3 | Jalon V1.0 | Écart et justification |
|---|---|---|
| Phase 1 Bootstrap | M0 | identique, enrichi (ABI, hashes, probe) |
| Phase 2 Profiler | M1 | identique, budgété |
| Phase 3 Analyzer | M2 + M3 | scindé : analyse et décision sont deux jalons distincts |
| Phase 4 Scheduler | M2 | avancé : le scheduler sert d'abord aux tâches internes |
| Phase 5 World/Entity Mirror | M4 (snapshot) + M5 (mirror) | le snapshot suffit ; le mirror devient optionnel (C-02 de l'audit) |
| Phase 6 Automatic offload | M4 | identique, avec validation obligatoire |
| Phase 7 IR | M6 | identique, périmètre restreint (I-01) |
| Phase 8 Native execution | M7 | identique, optionnel |
| Phase 9 Rust-native subsystems | M8 | requalifié : via SDK, opt-in (I-06) |
| Phase 10 Rust-dominant runtime | hors V1.0 | direction conservée, non promise |

---

# PARTIE 31 : CRITÈRES D'ACCEPTATION

## 31.1 Checklist principale

```text
[ ] clean build             : ./gradlew clean build réussit sur un dépôt neuf
[ ] Forge startup           : le JAR se charge client et serveur, Forge 47.x
[ ] native runtime loading  : natif extrait, hash vérifié, ABI validée
[ ] profiler                : overhead mesuré < 2 %, adaptation de niveau vérifiée
[ ] workload discovery      : workloads détectés sans intégration manuelle
[ ] analyzer                : read/write sets, threads, déterminisme produits
[ ] decision engine         : décisions expliquées, aucune sans motif
[ ] scheduler               : 10^7 tâches sans perte ni deadlock
[ ] safe parallelism        : résultats identiques avec 1, 2, 4, 8, 16 workers
[ ] snapshot                : cohérent, borné, sans référence Java
[ ] command buffer          : ordre déterministe, append-only
[ ] validation              : aucune promotion sans 64 shadow runs conformes
[ ] rollback                : effectif en 1 tick, testé sous charge
[ ] fallback                : chaque composant dégrade vers Java sans crash
[ ] cache                   : invalidation par fingerprint, second démarrage rapide
[ ] diagnostics             : /rfx why répond pour toute décision active
[ ] benchmarks              : résultats reproductibles, méthodologie documentée
[ ] regression tests        : aucune régression au-delà des seuils
[ ] compatibility tests     : trois configurations réseau OK, mods concurrents OK
[ ] release build           : artefact vérifié, hashes publiés
[ ] clean installation      : procédure PARTIE 26 réussie sur Windows et Linux
[ ] JAR usable              : jeu jouable, désinstallable sans conséquence
```

## 31.2 Critères de correction (bloquants)

```text
[ ] zéro divergence d'état sur les scénarios G-01..G-15
[ ] zéro corruption de sauvegarde
[ ] zéro crash JVM imputable à RF-X
[ ] zéro deadlock
[ ] zéro fuite mémoire native sur 8 heures
[ ] zéro effet non validé appliqué à l'état autoritatif
[ ] tous les invariants INV-01..INV-15 vérifiés par test
```

## 31.3 Critères de performance

```text
[ ] overhead en mode OBSERVE_ONLY  < 2 % du MSPT (profil heavy)
[ ] overhead des hooks + commit    < 5 % du MSPT en régime établi
[ ] gain net mesuré > 0 en mode BALANCED sur profil heavy
[ ] aucune dégradation du 1 % low et du 0,1 % low côté client
[ ] moins de 50 traversées FFI par tick en régime établi
[ ] temps avant première optimisation active < 3 minutes (démarrage à froid)
[ ] démarrage à chaud (cache valide) : décisions restaurées en < 200 ms
```

## 31.4 Critères d'automatisation

```text
[ ] découverte des hotspots sans configuration
[ ] analyse des méthodes sans intégration par mod
[ ] construction des workloads automatique
[ ] identification des candidats automatique
[ ] test des stratégies automatique
[ ] activation des stratégies validées automatique
[ ] rollback des stratégies problématiques automatique
[ ] aucune action utilisateur requise pour obtenir un gain
```

## 31.5 Critères d'évolutivité

```text
[ ] aucune branche conditionnelle sur un nom de mod dans le moteur (INV-12)
[ ] complexité au plus O(n log n) en nombre de workloads suivis
[ ] fonctionnement vérifié à 10, 50, 100, 250 et 500 mods synthétiques
[ ] ajout d'un nouveau type de commande sans modifier le commit engine
[ ] ajout d'un nouveau sous-système de domaine sans modifier le scheduler
[ ] portage vers un autre loader possible en réimplémentant C-01 seul
```

## 31.6 Critères documentaires

```text
[ ] tous les fichiers de la PARTIE 27 présents et à jour
[ ] docs/AGENT.md permet à un agent neuf de builder, tester et livrer
[ ] chaque chiffre publié est traçable jusqu'à un fichier de résultats
[ ] chaque option de configuration est documentée
[ ] chaque code d'erreur est documenté
[ ] les limitations connues sont listées explicitement
```

---

# PARTIE 32 : ANNEXES

## A.1 Glossaire

Voir §0.6 pour les termes fondamentaux. Compléments :

| Terme | Définition |
|---|---|
| TL0..TL6 | niveau de transformation d'une stratégie (voir §4.13) |
| Blast radius | portée maximale des dégâts si une transformation est incorrecte |
| Order key | clé stable déterminant l'ordre de commit indépendamment de l'ordre d'achèvement |
| Canary | phase d'activation partielle et surveillée |
| Drain | phase du tick où les tâches sont récupérées et commitées |
| Heat | classe de coût d'un workload (COLD/WARM/HOT/CRITICAL) |
| Précision | qualité d'un ensemble d'accès (EXACT / OVER_APPROX / UNKNOWN) |
| Mod synthétique | mod de test généré, paramétrable, sans dépendance tierce |

## A.2 Codes d'erreur

| Code | Signification | Sévérité | Réaction |
|---|---|---|---|
| E-1001 | version de Forge hors plage supportée | majeure | OBSERVE_ONLY |
| E-1002 | version d'ABI incompatible | critique | DISABLED |
| E-1003 | hash du binaire natif invalide | critique | DISABLED |
| E-1004 | double initialisation du runtime | majeure | refus |
| E-1005 | binaire natif absent pour la plateforme | majeure | DEGRADED |
| E-1006 | échec de chargement de la bibliothèque | majeure | DEGRADED |
| E-1201 | overhead du profiler au-dessus du budget | mineure | réduction de niveau |
| E-1301 | buffer de sondes saturé | mineure | échantillons perdus |
| E-1501 | budget de snapshot dépassé | mineure | repli Java |
| E-2001 | descripteur incohérent | majeure | workload invalidé |
| E-2003 | rejet sans motif (erreur interne) | majeure | assertion, incident |
| E-2101 | collision de WorkId | majeure | workloads exclus |
| E-2201 | oscillation de décision détectée | mineure | verrouillage de stratégie |
| E-2301 | cycle détecté dans le task graph | majeure | arête refusée |
| E-2401 | backpressure : soumission refusée | mineure | repli Java |
| E-2501 | divergence d'audit du world mirror | majeure | mirror désactivé |
| E-2601 | échec d'application non compensable | critique | JAVA_ONLY permanent + dump |
| E-2701 | violation de déclaration d'un système SDK | majeure | système désactivé |
| E-2801 | soumission à un device non disponible | mineure | refus |
| E-2901 | opération hors de la phase de tick autorisée | majeure | assertion, incident |
| E-3001 | panic Rust capturée à la frontière FFI | majeure | sous-système désactivé |
| E-3002 | dépassement du budget mémoire natif | majeure | purge puis DEGRADED |
| E-3003 | corruption détectée dans une structure persistée | majeure | rotation et recréation |
| E-3004 | invariant violé | critique | HALT + dump |

## A.3 Index des invariants, exigences et composants

```text
Invariants     : INV-01..INV-15 (PARTIE 8)
Composants     : C-01..C-53 (PARTIE 3 et 5)
Interfaces     : IF-01..IF-06 (ABI), IF-10..IF-14 (Java/traits), IF-20..IF-29 (logiques)
Modèles        : DM-01..DM-17 (PARTIE 4)
Machines       : SM-01..SM-07 (PARTIE 7)
Défaillances   : FM-01..FM-31 (PARTIE 5)
Risques        : RISK-01..RISK-15 (PARTIE 1)
Exigences      : R-100..R-920 (tout le document)
Tests          : T-001..T-900, G-01..G-15, B-01..B-10
Décisions      : ADR-001..ADR-012
Jalons         : M0..M10
```

## A.4 Traçabilité V0.3 vers V1.0

| Section V0.3 | Statut | Section V1.0 |
|---|---|---|
| 0. Résumé exécutif | DEEPENED | 2.1 |
| 1.1 Ce que ce n'est pas | CORRECTED (C-01 audit) | 2.2 |
| 1.2 Ce que ça doit devenir | KEPT | 2.2 |
| 2. Vision ultime | DEEPENED | 2.3 |
| 3.1 Objectifs client | KEPT | 2.4, 15.2 |
| 3.2 Objectifs serveur | KEPT | 2.5, 15.1 |
| 3.3 Objectif transversal | KEPT | 2.6 |
| 4. Profiler pour comprendre | DEEPENED | 5.5, 12 |
| 5. Capture du workload | DEEPENED | 4.1..4.4, 5.5 |
| 6.1 Analyse statique | DEEPENED | 5.11, 9.2 |
| 6.2 Analyse dynamique | DEEPENED | 5.5 |
| 6.3 Combinaison | DEEPENED | 5.8 |
| 6.4 Décision automatique | DEEPENED | 5.15, 7.2 |
| 6.5 Capture sûre | DEEPENED | 4.3, 5.19 |
| 6.6 États de confiance | CORRECTED (C-04 audit) | 7.1 (SM-01) |
| 6.7 Capture refusée | DEEPENED | 4.15, 5.15 |
| 6.8 Validation avant activation | DEEPENED | 11 |
| 6.9 Zéro mod spécifique | KEPT | 2.7 (P-03), INV-12 |
| 7. Classification automatique | DEEPENED | 4.11 |
| 8. Détection d'opportunités | DEEPENED | 5.15, 16 |
| 9. Moteur de décision | DEEPENED | 5.15 |
| 10. Runtime Rust | DEEPENED | 3.6, 5.27 |
| 11. Scheduler adaptatif | DEEPENED | 5.17 |
| 12. Task Graph | DEEPENED | 5.16 |
| 13. Event Runtime | DEEPENED | 5.6 |
| 14. Virtualisation des handlers | CORRECTED (C-08 audit, speculative supprimé) | 5.6, 1.4 |
| 15. Offload générique | DEEPENED | 10 |
| 16. IR | REQUALIFIED (EXPERIMENTAL) | 10.3 |
| 17. Optimisations compilateur | CORRECTED (suppression de sync retirée) | 10.5, 1.4 |
| 18. World Mirror | CORRECTED (C-02 audit) | 5.20 |
| 19. Command Buffer | DEEPENED | 5.21, 4.16 |
| 20. Entity Engine | DEEPENED | 16.1 |
| 21. Chunk Engine | DEEPENED | 16.2 |
| 22. Networking | CORRECTED (C-05 audit) | 5.40, 16.5 |
| 23. Mémoire | DEEPENED | 5.28 |
| 24. Gestion de la JVM | DEEPENED | 6.1, 9 |
| 25. Architecture client | DEEPENED | 15.2 |
| 26. Architecture serveur | DEEPENED | 15.1 |
| 27. N mods | REQUALIFIED (I-03) | 31.5, 22 |
| 28. Modèle de compatibilité | DEEPENED | 5.31, 18 |
| 29. Système de confiance | CORRECTED (C-04 audit) | 4.13 (TL), 7.1 |
| 30. Shadow execution | CORRECTED (I-05) | 5.25, 11 |
| 31. Rollback automatique | DEEPENED | 5.26 |
| 32. Auto-tuning | DEEPENED | 5.42, 13 |
| 33. Hot Path Database | DEEPENED | 5.9 |
| 34. Budget d'analyse | DEEPENED | 12.2, 12.3 |
| 35. Profiler intégré | DEEPENED | 5.44 |
| 36. Diagnostic automatique | DEEPENED | 5.33 |
| 37. Sécurité et stabilité | DEEPENED | 19 |
| 38. Déterminisme | DEEPENED | 5.13, 8.5, 8.6 |
| 39. Modes | DEEPENED | 28.3 |
| 40. API Rust pour mods | REQUALIFIED (EXPERIMENTAL) | 5.37 |
| 41. Architecture hybride | KEPT | 2.3, 3.1 |
| 42. Migration progressive | DEEPENED | 30 |
| 43. Ce qui reste Java | DEEPENED | 9.9 |
| 44. Forge et compatibilité | DEEPENED | 18.1 |
| 45. Compatibilité sauvegardes | DEEPENED | 18.2 |
| 46. Compatibilité réseau | DEEPENED | 18.3, 5.40 |
| 47. Tests | DEEPENED | 20 |
| 48. Stress tests | DEEPENED | 20.3.5, 22 |
| 49. Benchmarks obligatoires | DEEPENED | 21 |
| 50. Non-régression | DEEPENED | 21.4 |
| 51. Architecture du projet | DEEPENED | 3.6 |
| 52. Technologies | DEEPENED | 6.1, 23.1 |
| 53. Plateformes | KEPT | 23.1, 23.5 |
| 54. Observabilité | DEEPENED | 5.33 |
| 55. Explainability | DEEPENED | 5.33, 5.41 |
| 56. Auto-optimisation sans intervention | KEPT | 31.4 |
| 57. Cache d'optimisation | DEEPENED | 4.5, 5.29, 14 |
| 58. Adaptation au matériel | DEEPENED | 4.14, 5.43 |
| 59. Optimisation du thread principal | DEEPENED | 2.7 (P-08), 5.15 |
| 60. Réduction de la contention | CORRECTED (limité au code RF-X) | 5.28, 1.4 |
| 61. Data-oriented execution | DEEPENED | 5.19, 5.28 |
| 62. SIMD | DEEPENED | 10.5 |
| 63. GPU | REQUALIFIED (FUTURE) | 5.45 |
| 64. Gestion des erreurs | DEEPENED | A.2, 6.10 |
| 65. Crash containment | DEEPENED | 19.3, 19.4 |
| 66. Diagnostics persistants | DEEPENED | 4.17, 5.46 |
| 67. Interface utilisateur | DEEPENED | 5.44 |
| 68. Vue Mods | CORRECTED (généré, pas nommé) | 5.44, 17 |
| 69. Vue Workload | DEEPENED | 5.33 |
| 70. Philosophie de performance | KEPT | 2.7 (P-07), 21 |
| 71. Objectif de transparence | KEPT | 2.2, 18 |
| 72. Limite fondamentale | KEPT | 2.8, 10.2 |
| 73. Roadmap | DEEPENED | 30 |
| 74. Critères de réussite | DEEPENED | 31 |
| 75. Vision finale | KEPT | 2.3 |
| 76. Formulation officielle | KEPT | 2.2 |
| 77. Règle d'or | KEPT | 2.9 |

## A.5 Limitations connues de la V1.0

```text
- la transformation automatique de bytecode arbitraire vers natif n'existe pas
  et n'existera pas : seul un sous-ensemble déclaré est supporté (10.2)
- le world mirror, l'IR, le JIT, le réseau, le pathfinding natif, le SDK et la
  préparation de rendu sont EXPERIMENTAL et désactivés par défaut
- le lighting et le GPU sont FUTURE : interfaces seulement
- le gain dépend entièrement du modpack : un modpack dont le coût est dominé par
  des workloads écrivant l'état autoritatif ne bénéficiera que du batching
- les mesures de gain sont locales à la machine ; aucun chiffre n'est garanti
- le support Fabric/NeoForge n'est pas fourni, mais l'abstraction C-01 le permet
```

---

# FINAL AGENT EXECUTION CONTRACT

Ces règles sont **impératives** pendant toute l'implémentation de RUSTFORGE-X. Elles priment sur toute préférence de style, toute optimisation locale et toute envie d'aller vite.

## 1. Source de vérité

```text
1.1  Ce document est la source de vérité principale. En cas de doute, il tranche.
1.2  Si le document est ambigu, l'agent choisit l'option la plus conservatrice
     pour la correction, l'implémente, et écrit un ADR expliquant son choix.
1.3  L'agent NE DOIT PAS demander "que dois-je coder maintenant ?" si la réponse
     se déduit du jalon courant (PARTIE 30) et de la Definition of Done (PARTIE 29).
1.4  Toute divergence volontaire par rapport à ce document exige un ADR daté.
```

## 2. Boucle de travail obligatoire

```text
READ SPEC -> INSPECT REPO -> PLAN -> IMPLEMENT -> BUILD -> TEST -> FIX
          -> BENCHMARK -> DOCUMENT -> PACKAGE -> RELEASE
```

```text
2.1  Avant d'écrire du code : lire la fiche du composant concerné en PARTIE 5.
2.2  Après chaque implémentation : builder ET exécuter les tests, immédiatement.
2.3  Un travail n'est pas terminé tant que sa Definition of Done n'est pas cochée.
2.4  Ne jamais empiler plusieurs composants non testés.
```

## 3. Interdictions absolues

```text
3.1  NE JAMAIS écrire TODO, FIXME, unimplemented!(), todo!(), "placeholder" ou
     une implémentation factice dans un module déclaré STABLE.
3.2  NE JAMAIS inventer un chiffre de performance. Tout chiffre vient d'un
     fichier de résultats généré par le harnais.
3.3  NE JAMAIS écrire une branche conditionnelle sur un nom de mod dans le moteur.
3.4  NE JAMAIS muter l'état Minecraft hors du thread autoritatif.
3.5  NE JAMAIS activer une transformation sans validation quand la spec l'exige.
3.6  NE JAMAIS supprimer, contourner ou affaiblir un invariant INV-xx pour faire
     passer un test. Si un invariant gêne, c'est le code qui est faux.
3.7  NE JAMAIS assouplir le principe UNKNOWN = CONSERVATIVE.
3.8  NE JAMAIS laisser une panic Rust traverser la frontière FFI.
3.9  NE JAMAIS bloquer le thread autoritatif sans deadline.
3.10 NE JAMAIS redistribuer Minecraft, Forge, des mods ou des assets tiers.
3.11 NE JAMAIS ouvrir de connexion réseau depuis RUSTFORGE-X.
3.12 NE JAMAIS désactiver un test pour le faire passer ; le corriger ou le
     documenter comme défaut ouvert.
3.13 NE JAMAIS marquer une fonctionnalité STABLE sans ses tests, ses métriques,
     son fallback et sa documentation.
```

## 4. Obligations

```text
4.1  Tout composant implémenté DOIT exposer ses métriques et son état de maturité.
4.2  Tout chemin optimisé DOIT avoir un fallback Java testé.
4.3  Toute décision DOIT être explicable via /rfx why.
4.4  Tout accès non prouvé DOIT être traité comme le pire cas.
4.5  Tout commit de code DOIT citer les identifiants concernés :
        feat(C-17): work stealing deque [R-420, T-260]
4.6  Toute nouvelle option DOIT avoir défaut, plage, validation et documentation.
4.7  Tout nouveau code d'erreur DOIT être ajouté à l'annexe A.2.
4.8  Toute structure persistée DOIT porter magic, version de schéma et CRC.
4.9  Toute allocation native DOIT être comptée dans un budget.
4.10 Tout jalon DOIT se terminer par un JAR installable et jouable.
```

## 5. Autonomie de décision

L'agent PEUT décider seul, sans consultation :

```text
- le choix des structures de données internes non spécifiées
- le découpage en fonctions, modules et fichiers
- les noms internes, le style, l'organisation des tests
- les micro-optimisations n'affectant ni la sémantique ni les contrats
- l'ordre d'implémentation à l'intérieur d'un jalon
- les bibliothèques Rust courantes et maintenues, sous réserve de licence
  compatible et d'ajout au SBOM
```

L'agent NE DOIT PAS décider seul :

```text
- changer un contrat d'interface publié (IF-xx)
- changer le modèle de données (DM-xx) sans migration
- changer un invariant
- ajouter une dépendance lourde ou une bibliothèque native tierce
- modifier la licence
- publier une release
```

## 6. Gestion de l'incertitude

```text
6.1  Si une mesure manque, ne pas deviner : implémenter la mesure d'abord.
6.2  Si une preuve manque, ne pas activer : rester en JAVA_ONLY.
6.3  Si un test est instable (flaky), le traiter comme un défaut réel, jamais
     comme du bruit à ignorer.
6.4  Si un gain n'est pas reproductible, il n'existe pas.
6.5  Si une hypothèse (H-xx) est invalidée, appliquer le repli prévu et
     documenter l'invalidation.
```

## 7. Critère final

```text
Le travail est terminé lorsque, sur une machine vierge :

   git clone <repo>
   ./gradlew clean build test package

   produit rustforgex-<version>-mc1.20.1-forge47.jar ;
   ce JAR se charge sur Minecraft 1.20.1 + Forge 47.x, client et serveur ;
   le jeu se joue, se sauvegarde et se recharge sans différence de comportement ;
   /rfx status affiche un overhead mesuré et des optimisations validées actives ;
   les benchmarks archivés montrent un gain reproductible ;
   la checklist de la PARTIE 31 est intégralement cochée ;
   et le retrait du JAR laisse le monde parfaitement jouable.

Tant qu'un seul de ces points est faux, le projet n'est pas terminé.
```

---

**FIN DU CAHIER DES CHARGES RUSTFORGE-X V1.0 ULTIMATE**
