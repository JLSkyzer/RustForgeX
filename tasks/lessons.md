# Leçons apprises — RUSTFORGE-X

Format : `[date] | ce qui a mal tourné | règle pour l'éviter`

Ce fichier est relu au démarrage de chaque session, avant toute action.

---

## Leçons issues du cahier des charges (préchargées, non négociables)

Ces règles viennent du FINAL AGENT EXECUTION CONTRACT du CDC. Ce ne sont pas des
leçons apprises à la dure : ce sont des interdits connus d'avance.

| # | Règle |
|---|---|
| L-01 | Ne jamais écrire TODO / FIXME / placeholder / implémentation factice dans un module déclaré STABLE. |
| L-02 | Ne jamais inventer un chiffre de performance. Tout chiffre vient d'un fichier de résultats du harnais de bench. |
| L-03 | Ne jamais écrire de branche conditionnelle sur un nom de mod dans le moteur (INV-12). |
| L-04 | Ne jamais muter l'état Minecraft hors du thread autoritatif (INV-02). |
| L-05 | Ne jamais affaiblir un invariant INV-xx pour faire passer un test. Si l'invariant gêne, c'est le code qui est faux. |
| L-06 | Ne jamais assouplir le principe UNKNOWN = CONSERVATIVE. |
| L-07 | Ne jamais laisser une panic Rust traverser la frontière FFI. |
| L-08 | Ne jamais désactiver un test pour le faire passer. |
| L-09 | Ne jamais ouvrir de connexion réseau depuis RUSTFORGE-X (INV-15). |
| L-10 | Tout commit cite ses identifiants normatifs : `feat(C-17): work stealing deque [R-420, T-260]`. |
| L-11 | Toute divergence volontaire par rapport au CDC exige un ADR daté. |
| L-12 | Builder ET tester immédiatement après chaque implémentation. Ne jamais empiler des composants non testés. |

---

## Leçons de session

### 2026-09-04 | Le MDK Forge généré ne correspondait pas au CDC | Vérifier la conformité au CDC avant d'écrire la moindre ligne

Le MDK générait `fr.eriniumgroup.rustforgex` avec du code d'exemple (`example_block`,
`example_item`, magic number) alors que le CDC impose `dev.rustforgex` et un mod qui
« ne fait rien d'autre » à M0. Corrigé en tout début de projet, coût quasi nul.

**Règle** : à la réception de tout squelette généré par un outil, le confronter au CDC
(PARTIES 3.6 et 3.7) AVANT d'écrire du code par-dessus.

### 2026-09-04 | Le CDC se contredit sur le layout du dépôt | Choisir l'option conservatrice et écrire un ADR

La PARTIE 3.6 place le Java dans `java/src/main/java/`, la PARTIE 23.2 référence
`src/main/resources/natives/` (racine). Contradiction interne au document.

**Règle** : quand le CDC est ambigu, appliquer le contrat agent 1.2 — choisir l'option
la plus conservatrice pour la correction, l'implémenter, et écrire un ADR. Ne jamais
trancher silencieusement.

### 2026-09-04 | Heredoc bash en échec sur du Markdown accentué long | Utiliser l'outil d'écriture de fichier pour les documents

L'écriture d'un document Markdown de ~180 lignes via `cat <<'EOF'` a échoué
(`unexpected EOF`), sans rien créer, et une première rédaction avait été faite en ASCII
sans accents pour contourner le shell.

**Règle** : les fichiers de documentation passent par l'outil d'écriture, jamais par un
heredoc. Le shell reste réservé aux petits fichiers de configuration ASCII. Ne jamais
dégrader l'orthographe française pour arranger un outil.

### 2026-09-04 | Un témoin de retour négatif confondu avec un code d'erreur | Ne jamais faire porter deux sens à une même valeur de retour

`transferProbe` renvoyait une somme de contrôle `u64` réinterprétée en `jlong`. Dès
que la somme dépassait `i64::MAX`, elle devenait négative, et Java la lisait comme un
code d'erreur : la mesure du débit de transfert échouait silencieusement et
`ffi_batch_ns_per_kb` restait à 0. Les 102 tests passaient : aucun ne vérifiait la
**couverture** `ffi_transfer`, seulement `ffi_call`. Le défaut n'est apparu qu'en
lançant le jeu.

**Règle** : sur la frontière FFI, une valeur de retour porte un seul sens. Si un
succès doit rapporter une donnée dont le domaine recouvre celui des codes d'erreur,
contraindre le domaine (ici, effacer le bit de signe) ou passer la donnée par un
paramètre de sortie. Et pour chaque champ qu'un composant est censé remplir, tester
qu'il est **effectivement** rempli, pas seulement que l'appel a réussi.

### 2026-09-04 | Test vert ≠ code qui marche | Exécuter le vrai jeu avant de déclarer un jalon terminé

Le bug ci-dessus et un avertissement mixin ne sont apparus qu'au premier
`./gradlew runGameTestServer`. Les tests unitaires et le test d'intégration natif
étaient tous verts.

**Règle** : un jalon n'est pas terminé tant que le mod n'a pas été lancé dans
Minecraft et que ses journaux n'ont pas été relus ligne à ligne. Les logs disent des
choses que les assertions ne pensent pas à demander.

### 2026-09-04 | Niveau de compatibilité mixin choisi au jugé | Vérifier la valeur admise par la version embarquée

En migrant le MDK, `compatibilityLevel` a été passé de `JAVA_8` à `JAVA_17` par
analogie avec la version du langage. Or mixin plafonne à `JAVA_13` dans Forge 47, et
son service annonce `JAVA_16` par défaut : le lancement affichait un avertissement.

**Règle** : une valeur de configuration d'un outil tiers se lit dans cet outil, pas par
analogie. En cas de doute, lancer et lire l'avertissement.

### 2026-09-05 | Fichier écrit avec un contenu provisoire, deux fois | Ne jamais matérialiser un fichier avant d'en avoir le contenu

Deux fois dans la même session, un fichier a été créé avec un contenu factice — le mot
« placeholder » — avant d'être aussitôt réécrit : d'abord `.github/workflows/ci.yml`,
puis `docs/decisions/ADR-016.md`. Le contenu réel a suivi immédiatement dans les deux
cas, mais entre-temps le dépôt contenait exactement ce que l'interdit n°1 du contrat
agent proscrit.

**Règle** : un fichier ne se crée qu'avec son contenu définitif. S'il n'est pas encore
rédigé, il n'est pas encore écrit. Le fait de « le remplir juste après » n'est pas une
excuse : si la session s'interrompt entre les deux, la fiction est commitée.

### 2026-09-05 | Une documentation affirmait une propriété que le code n'avait pas | Vérifier la propriété, pas l'intention

`ProbeBuffer::flush` portait en tête de module « un flush n'alloue ni ne bloque
(R-709) », et matérialisait un `Vec` par tick et par thread. Le commentaire décrivait
l'exigence, pas l'implémentation, et personne — moi compris — ne l'avait relu en se
demandant s'il était vrai.

**Règle** : quand un commentaire affirme une propriété mesurable — n'alloue pas, ne
bloque pas, ne lève pas — cette propriété se vérifie, par un test ou par la lecture de
chaque appel effectué. Une exigence citée en commentaire n'est pas une preuve qu'elle
est tenue ; c'est au mieux un rappel de ce qu'il faudra prouver.

### 2026-09-05 | Un test d'allocation faussé par le parallélisme des tests | Une mesure globale au processus exige un binaire de test qui ne contient qu'elle

Le test T-142 installe un allocateur global comptant les allocations. Deux tests
cohabitaient dans le même binaire d'intégration : exécutés en parallèle, chacun
comptait les allocations de l'autre, et les deux échouaient sur un chemin pourtant
exempt d'allocation. Fusionner les deux en un seul test a suffi.

**Règle** : toute mesure portant sur un état global au processus — allocateur, variable
d'environnement, répertoire courant, horloge simulée — doit vivre dans un binaire de
test qui ne contient qu'un seul test. Un échec de ce genre ressemble à un défaut du
code mesuré, et c'est ainsi qu'on va corriger du code correct.

### 2026-09-05 | Un plugin chargeable pris pour un plugin installé | Vérifier que le mécanisme a été emprunté, pas seulement qu'il est disponible

L'armement du transformateur de bytecode réussissait, et le mod journalisait
« instrumentation active » — alors que ModLauncher n'avait jamais instancié le plugin.
La classe était atteignable, donc l'appel passait ; le plugin n'était pas installé,
donc aucune classe ne lui était soumise. Aucune erreur nulle part, et un mod qui
annonce sondes actives et ne sonde rien.

**Règle** : « la classe se charge » ne prouve pas « le composant est en service ». Quand
un mécanisme dépend d'un tiers qui doit nous appeler, la seule preuve est qu'il nous a
appelés. Ici, le constructeur note son propre passage, et l'armement échoue
explicitement s'il n'a pas eu lieu. Un composant doit pouvoir répondre « on m'a
installé », pas seulement « j'existe ».

### 2026-09-05 | Deux fois le même bloc détruit par une découpe entre deux repères | Ne jamais découper un fichier « du repère A au repère B »

Pour retirer un bloc de `build.gradle`, j'ai deux fois découpé le fichier de son
commentaire d'ouverture jusqu'à un repère situé plus loin. Les deux fois, le bloc
`dependencies` se trouvait entre les deux et a disparu, avec pour seul symptôme un
« Missing 'minecraft' dependency » sans rapport apparent.

**Règle** : un retrait de bloc se fait sur son texte exact, ou sur des bornes de lignes
vérifiées par leur contenu avant modification. Une découpe entre deux repères suppose
qu'on sait ce qu'il y a entre eux — et c'est justement ce qu'on ne regarde pas.

### 2026-09-05 | Lire la source de l'outil plutôt que d'essayer ses réglages | Quand un outil refuse, chercher où il décide

Faire parvenir un JAR à la couche d'amorçage de ModLauncher sous ForgeGradle m'a coûté
sept configurations successives — classpath, `ignoreList`, manifeste, `run/mods/`,
réécriture du fichier de classpath, `runtimeClasspathArtifacts` — toutes plausibles,
toutes fausses. La réponse tenait dans deux méthodes de `RunConfigGenerator` :
`legacyClassPath.file` vaut `{minecraft_classpath_file}`, composé depuis
`getMinecraftArtifacts()`, et la tâche de run expose cette collection.

**Règle** : devant un outil qui ne fait pas ce qu'on attend, ouvrir son code avant
d'essayer ses réglages. Un `javap` sur la classe qui décide coûte moins qu'un seul
cycle d'essai, et rend une réponse au lieu d'une hypothèse.

### 2026-09-05 | Cinq configurations essayées avant de comparer avec un cas qui marche | Comparer d'abord avec l'exemple qui fonctionne

Le plugin de lancement n'était pas découvert. J'ai essayé successivement le classpath,
`ignoreList`, la suppression de `FMLModType`, `run/mods/`, et l'ajout au fichier de
classpath — sans succès et sans comprendre. La réponse a tenu en une commande : lister
le contenu du JAR de Mixin, dont le plugin **est** découvert, et constater qu'il embarque
un `module-info.class` que le nôtre n'avait pas.

**Règle** : devant un mécanisme qui refuse de fonctionner alors qu'il fonctionne pour
d'autres, comparer avec un cas qui marche **avant** de modifier quoi que ce soit. Une
seule comparaison structurelle vaut mieux que cinq essais successifs, et coûte moins
cher que le premier d'entre eux.

### 2026-09-04 | « Disconnected » attribué au mod alors que c'était l'authentification | Lire le log avant de supposer que le défaut vient de son propre code

Le client était déconnecté du serveur de développement. Le réflexe aurait été de
chercher un défaut de RUSTFORGE-X — handshake de registre, canal réseau, mixin. Le log
du client disait exactement autre chose : `Failed to log in: Invalid session`. Le
client de ForgeGradle démarre sans session Mojang, et un serveur en `online-mode=true`
le rejette. Le mod n'était pour rien dans l'affaire.

**Règle** : devant un symptôme, lire le message d'erreur réel avant de formuler la
moindre hypothèse. Le fait qu'on vienne de modifier quelque chose ne fait pas de ce
quelque chose la cause.

### 2026-09-04 | Les logs du serveur avaient disparu | `run/` est partagé entre client et serveur

`runClient` et `runServer` utilisent le même répertoire de travail : le second
processus démarré prend la main sur `latest.log` et archive le précédent en
`logs/<date>-N.log.gz`. Le log du serveur semblait s'arrêter au milieu.

**Règle** : quand un log paraît tronqué dans `run/logs/`, la suite est dans les `.gz`
horodatés. Vérifier aussi lequel des deux processus a écrit le fichier consulté.

### 2026-09-04 | Renommage global : la plateforme non compilée localement a été oubliée | Vérifier chaque cible avant de conclure

Le passage des identifiants en anglais a renommé `SondeSysteme` en `SystemProbe` dans
`hw/mod.rs` et `hw/windows.rs`, mais pas dans `hw/linux.rs`, compilé uniquement sous
`#[cfg(target_os = "linux")]`. Sur cette machine Windows, tout compilait, tous les
tests passaient — et le build Linux était cassé.

**Règle** : après un renommage transverse, vérifier chaque cible supportée, pas
seulement l'hôte. `rustup target add x86_64-unknown-linux-gnu` puis
`cargo check --target x86_64-unknown-linux-gnu` suffit : `check` n'a pas besoin d'un
éditeur de liens croisé. Plus généralement, tout code derrière un `#[cfg]` est du code
que le compilateur local ne relit pas.

### 2026-09-04 | `gradle.properties` est lu en ISO-8859-1 | Garder ce fichier en ASCII pur

Les fichiers `.properties` sont chargés par Java en ISO-8859-1 : un caractère accentué
écrit en UTF-8 y devient du mojibake, et `mod_description` est réinjecté tel quel dans
`mods.toml` puis affiché dans la liste des mods en jeu.

**Règle** : `gradle.properties` reste en ASCII. Si un texte accentué doit être affiché,
il passe par une ressource UTF-8, pas par une propriété Gradle.

### 2026-09-05 | Un transformateur qui ne transformait rien a cassé un serveur | Déclarer une cible est déjà un acte

Première énumération complète des classes de mods : 87 981 cibles, et six mods qui
échouent à se construire. L'accusation évidente — notre injecteur — était fausse : au
moment des échecs le journal ne portait aucun « Transformateur armé », `probeIds` valait
`null`, et `transform()` rendait chaque `ClassNode` inchangé. Pas un octet modifié.

`ClassTransformer` a un chemin rapide : sans preneur, les octets d'une classe passent
sans être décodés. La déclarer comme cible la fait entrer dans toute la chaîne, phase
`AFTER` comprise, donc Mixin — qui échoue sur ce qu'il ne sait pas résoudre.

**Règle** : avant d'accuser une transformation, vérifier dans le journal qu'elle a
seulement eu lieu. Et se souvenir que l'ensemble des cibles est lui-même un effet de
bord, indépendamment de ce qu'on en fait.

### 2026-09-05 | Sonder une classe patchée par un mixin l'a rendue impatchable | Ne jamais toucher ce qu'un autre outil va réécrire

`Critical injection failure: Variable modifier method ValkyrienSkies$entity(…) failed
injection check, (0/1) succeeded.` Un `@ModifyVariable` d'un mod, posé sur la classe
d'un autre mod, que nous avions instrumentée juste avant.

Nos `ITransformer` passent toujours avant Mixin — l'ordre n'est pas négociable. Toute
classe qu'un mixin patche doit donc nous rester interdite. Les cibles se lisent dans
l'annotation `@Mixin` : `value` (littéraux de classe) **et** `targets` (noms textuels,
la forme employée quand la cible n'est pas visible à la compilation, donc le cas courant
entre mods). Oublier la seconde forme laisserait passer précisément les cas inter-mods.

**Règle** : quand deux outils réécrivent le même bytecode et que l'ordre est imposé,
celui qui passe en premier n'a pas à « faire attention » — il doit s'interdire le
terrain de l'autre. Le coût mesuré ici est de 1 % des cibles.

### 2026-09-05 | 180 lignes FATAL passées sous 96 tests verts | Comparer les journaux des deux configurations, pas seulement leurs résultats

La campagne C-36 a produit dix journaux de serveur. Je n'ai lu que les fichiers de
résultat. Un essai ultérieur a révélé 180 lignes `FATAL` de `TransformerClassWriter`
— absentes de la configuration sans RUSTFORGE-X, donc causées par nous.

Le diagnostic a tenu en une commande : compter la même signature dans les deux journaux.
`b-mods-seuls` : 0. `c-rfx-actif` : 180. La comparaison était disponible depuis le début,
et gratuite.

**Règle** : quand une campagne produit deux configurations, la comparaison ne s'arrête
pas aux métriques. Passer un `grep -c` des motifs d'erreur sur les journaux des deux
côtés fait apparaître les régressions qu'aucune assertion ne pense à demander — c'est
exactement ce que la PARTIE 7 du CLAUDE.md appelle relire les journaux.
