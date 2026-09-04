# Directives projet — RUSTFORGE-X

Guide de référence pour tout assistant travaillant sur ce dépôt. Il complète le
`CLAUDE.md` global de l'utilisateur et ne le remplace pas.

**RUSTFORGE-X n'est pas un mod de contenu.** C'est un runtime adaptatif Java/Rust :
il observe un modpack Forge, analyse ce qu'il fait, et ne déporte du travail vers du
code natif que lorsque la correction de cette transformation est prouvée. Les règles
ci-dessous en découlent.

---

## 0. Hiérarchie des sources

1. **`docs/spec/RUSTFORGE-X_Cahier_des_Charges_v1.0.md`** — source de vérité du projet.
   En cas de doute, il tranche. Son *FINAL AGENT EXECUTION CONTRACT* est impératif.
2. Ce fichier — conventions du dépôt.
3. `docs/AGENT.md` — carte du dépôt, état courant, procédures.
4. `tasks/lessons.md` — leçons apprises, à relire à chaque démarrage de session.

Si le cahier des charges est **ambigu**, choisir l'option la plus conservatrice pour la
correction, l'implémenter, et écrire un ADR. Ne jamais trancher silencieusement.
Toute divergence volontaire exige un ADR daté (contrat 1.4).

---

## 1. Comportement de l'assistant

- **Réponses concises.** Droit au but, pas d'introduction superflue.
- **Langue.** Les discussions, explications, Javadoc, commentaires et documentation
  sont en **français**. Le code — classes, méthodes, variables, modules Rust, noms de
  crates — est en **anglais**, clair et auto-descriptif.
- **Code modulaire.** Pas de classe monolithique. Une responsabilité par type.
- **Édition.** Ne renvoyer que les blocs modifiés avec assez de contexte, sauf demande
  explicite du fichier entier.
- **Jamais de chiffre inventé.** Aucune valeur de performance n'est écrite sans venir
  d'une mesure réelle. Tant que le harnais de benchmark (C-36) n'existe pas, aucun
  gain n'est annoncé nulle part — ni dans le code, ni dans la doc, ni en réponse.
- **Fin de réponse.** Terminer chaque réponse, une seule fois et à la toute fin, en
  rappelant le prénom de l'utilisateur (**Killian**). C'est un indicateur de vigilance :
  si le prénom disparaît ou change, l'assistant décroche.

---

## 2. Identifiants normatifs et commits

Le cahier des charges définit des identifiants stables, que le code et les commits
citent obligatoirement :

```text
R-xxx exigence       C-xx composant      IF-xx interface     DM-xx modèle de données
SM-xx machine états  INV-xx invariant    FM-xx défaillance   T-xxx test
B-xx benchmark       ADR-xxx décision    M-x jalon           E-xxxx code d'erreur
```

- Tout module porte, dans son commentaire de tête, le composant qu'il implémente et
  son **niveau de maturité** (`STABLE`, `EXPERIMENTAL`, `DISABLED`, `FUTURE`).
- Tout commit de code cite ses identifiants :
  `feat(C-17): work stealing deque [R-420, T-260]`
- Types de commit : `feat`, `fix`, `test`, `docs`, `refactor`, `bench`, `chore`, `build`.

`ADR-001` à `ADR-012` sont réservés au cahier des charges ; les ADR de l'agent
commencent à `ADR-016` (les 013 à 015 sont pris).

---

## 3. Les interdits (contrat agent, section 3)

Non négociables. Un test qui échoue à cause d'eux signale un défaut du code, jamais de
la règle.

| # | Interdit |
|---|---|
| 1 | `TODO`, `FIXME`, `todo!()`, `unimplemented!()`, placeholder ou implémentation factice dans un module `STABLE` |
| 2 | Inventer un chiffre de performance |
| 3 | Une condition sur un **nom de mod** dans le moteur (INV-12) |
| 4 | Muter l'état Minecraft hors du thread autoritatif (INV-02) |
| 5 | Affaiblir un invariant `INV-xx` pour faire passer un test |
| 6 | Assouplir le principe **UNKNOWN = CONSERVATIVE** |
| 7 | Laisser une panic Rust traverser la frontière FFI |
| 8 | Désactiver un test pour le faire passer |
| 9 | Ouvrir une connexion réseau (INV-15) |
| 10 | Redistribuer Minecraft, Forge, un mod ou un asset tiers |

**Hors du périmètre de décision de l'assistant** : changer un contrat `IF-xx` publié,
changer un `DM-xx` sans migration, changer un invariant, ajouter une dépendance lourde
ou une bibliothèque native tierce, modifier la licence, publier une release.

---

## 4. Architecture

### 4.1. Frontière Java ↔ Rust (critique)

- **JNI + `DirectByteBuffer`** (ADR-004). Pas de Panama sur Java 17, pas de JNA/JNR.
- **Par lot, jamais par élément** (R-700) : mille appels JNI sont remplacés par un
  appel portant un tampon de mille éléments. Cible : moins de 50 traversées par tick.
- Toute fonction exportée renvoie un `i32` : `0` = succès, **négatif** = code de
  l'annexe A.2. Une valeur de retour porte **un seul sens** — si un succès doit
  rapporter une donnée dont le domaine recouvre celui des codes d'erreur, contraindre
  le domaine ou passer par un paramètre de sortie.
- Aucune structure `repr(Rust)` ne traverse : entiers et tampons CBOR versionnés.
- **Chaque** point d'entrée est enveloppé dans `catch_unwind`.
- Sens des dépendances strict : `rfx-ffi` → `rfx-core` → `rfx-model`, jamais l'inverse
  (INV-13).

### 4.2. Code `unsafe` (Rust)

- Interdit par défaut : les crates déclarent `unsafe_code = "deny"` ou `"forbid"`.
- Autorisé uniquement dans `rfx-ffi` (c'est la frontière) et dans la sonde système.
- **Chaque bloc porte un commentaire `// SAFETY :`** énonçant la précondition qui le
  rend correct. Un bloc sans justification est un défaut.

### 4.3. Sécurité client / serveur (side safety)

- Le code serveur ne doit **jamais** connaître les classes client.
- Jamais de `Minecraft.getInstance()`, `Screen` ni `net.minecraft.client.*` dans du
  code commun sans `DistExecutor`. Un `NoClassDefFoundError` sur serveur dédié est
  inacceptable.
- Toute décision dépendant du côté passe par la configuration
  (`general.side_client` / `general.side_server`).

### 4.4. Événements et Mixins

- **Priorité à l'API** : bus d'événements Forge avant toute autre approche.
- Mixin en dernier recours, et alors : `@Inject`, `@ModifyVariable` ou
  `@ModifyConstant`. **Jamais `@Overwrite`** — la compatibilité avec les autres mods du
  modpack prime.
- Chaque mixin documente la raison exacte de sa présence.
- `compatibilityLevel` du fichier mixins se lit dans la version de mixin embarquée par
  Forge, jamais par analogie avec la version du langage.

### 4.5. Performance des accroches

- `ServerTickEvent` et consorts tournent 20 fois par seconde : aucune recherche lourde,
  aucun parsing, aucune allocation dans le chemin de sonde (INV-14).
- **N'installer une accroche que si elle fait quelque chose.** S'accrocher au tick pour
  n'y rien faire coûte du temps à chaque tick et ne rapporte rien.
- Chaque accroche passe par une garde qui capture, compte, et désactive après cinq
  échecs. Un défaut de RUSTFORGE-X ne doit jamais empêcher de jouer.

---

## 5. Textes, journalisation et internationalisation

- **Interdiction de coder en dur un texte affiché au joueur.**
- Utiliser `Component.translatable("categorie.rustforgex.cle")`.
  `Component.literal()` est réservé au débogage et aux valeurs techniques brutes
  (chemins, nombres, identifiants).
- **`en_us.json` et `fr_fr.json` sont obligatoires** et tenus à jour ensemble. Une clé
  ajoutée dans l'un et pas dans l'autre est un défaut.
- Bannir `System.out.println()`. Utiliser le logger de la classe :
  `LOGGER.info`, `LOGGER.debug`, `LOGGER.warn`, `LOGGER.error`.
- Les messages de journal destinés à l'utilisateur disent explicitement ce qui se passe
  **et si le jeu est affecté** : « inactif, le jeu tourne normalement » vaut mieux
  qu'une trace brute.

---

## 6. Configuration

- Fichier : `<gameDir>/rustforgex/config/rustforgex.toml`.
- Toute option a un **défaut sûr, une plage validée et une description** (R-590), et
  est documentée dans `CONFIGURATION.md`.
- Une valeur hors plage est rejetée avec un message précis et remplacée par le défaut.
  Jamais de comportement indéfini.
- Une clé inconnue est **conservée** et signalée, jamais supprimée (R-591).
- **Ne déclarer que les options réellement lues par du code implémenté.** Une option
  qui ne pilote rien trompe l'utilisateur.

---

## 7. Tests et vérification

Le standard de vérification prime sur la vitesse d'avancement.

- **Ne jamais désactiver un test** pour le faire passer. Un test instable est un défaut
  réel, jamais du bruit.
- Pour chaque champ qu'un composant est censé remplir, tester qu'il est
  **effectivement rempli**, pas seulement que l'appel a réussi.
- Un gain non reproductible n'existe pas.
- **Un jalon n'est pas terminé tant que le mod n'a pas été lancé dans Minecraft et que
  ses journaux n'ont pas été relus.** Les logs disent des choses que les assertions ne
  pensent pas à demander — deux défauts réels sont déjà passés sous 102 tests verts.

Commandes de vérification :

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
./gradlew clean build artifactVerify
./gradlew runGameTestServer   # puis relire run/logs/debug.log
```

---

## 8. Ressources et datagen

- Organiser proprement `src/main/resources/` (assets, data, META-INF).
- Modèles, recettes et tags répétitifs passent par la **data generation**, jamais par
  du JSON écrit à la main.
- Si une texture devient nécessaire : elle est d'abord écrite en **SVG à la main**,
  balise par balise, jamais générée par script. Validation humaine obligatoire avant
  toute conversion en PNG via ImageMagick
  (`magick -background none f.svg -filter point PNG32:f.png`, sans resize).

---

## 9. Gestion des dépendances tierces

Quand il faut comprendre le code d'un mod tiers :

1. chercher d'abord dans un dossier `mod dep source` s'il existe ;
2. sinon, l'API officielle ou le dépôt public ;
3. en dernier recours, désobfusquer le `.jar` de l'instance CurseForge.

Côté Rust, les bibliothèques courantes et maintenues sont autorisées, sous réserve de
licence compatible et d'ajout au SBOM de `SECURITY.md`. Côté Java, **aucune dépendance
d'exécution** n'est ajoutée au JAR sans nécessité démontrée.

---

## 10. Flux de travail

```text
LIRE LA SPEC → INSPECTER LE DÉPÔT → PLANIFIER → IMPLÉMENTER → BUILDER → TESTER
             → CORRIGER → MESURER → DOCUMENTER → EMPAQUETER
```

- Lire la fiche du composant en PARTIE 5 du cahier des charges **avant** d'écrire du code.
- Builder **et** tester immédiatement après chaque implémentation. Ne jamais empiler
  plusieurs composants non testés.
- Tenir `tasks/todo.md` à jour au fil de l'eau, et écrire dans `tasks/lessons.md` après
  chaque correction : `[date] | ce qui a mal tourné | règle pour l'éviter`.
- Un travail n'est pas terminé tant que sa Definition of Done (PARTIE 29) n'est pas cochée.
