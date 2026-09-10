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

### 2026-09-06 | Sept heures et demie de campagnes pour des réponses de douze minutes | Certifier n'est pas décider

Quatre campagnes C-36 de 2 h 30 dans la même journée. La question qui motivait les
deuxième et troisième était « est-ce que ce changement a fait baisser le coût ? », c'est
à dire distinguer +36 % de +3 %. Un facteur dix ne demande aucune précision : une paire
B/C de douze minutes y répondait.

La PARTIE 21.3 définit une méthodologie pour **publier** un chiffre. Je l'ai appliquée
chaque fois que je voulais **savoir** quelque chose. Ce sont deux questions avec des
exigences de précision sans rapport, et le script acceptait ses paramètres depuis le
début — je m'en servais déjà en mode court pour les essais à blanc, sans jamais le voir
comme un instrument de décision.

À la décharge de deux d'entre elles : la variation entre exécutions a produit des
constats qu'une paire ne pouvait pas donner — la ligne de base rendant zéro quatre fois
sur cinq, et `THROTTLED` indistinguable de `DEEP`. Mais ces constats auraient pu être
cherchés **après** avoir obtenu la réponse principale pour douze minutes.

**Règle** : avant toute mesure, demander quelle **précision** la décision exige. Si elle
tient dans un facteur deux ou plus, une paire courte suffit. La campagne normative ne
sert qu'à certifier un chiffre qu'on va publier, et elle vient en dernier.

### 2026-09-06 | Une exigence lue comme une consigne | Un plancher n'est pas une politique

R-311 énonce que « le sondage DOIT être refusé pour les méthodes de moins de 12
instructions ». Nous avions instrumenté tout ce qui dépassait douze, et payé +36 % de
MSPT pendant tout un jalon. La phrase dit ce qu'il ne faut pas faire ; elle n'ordonne
rien du reste.

Le cahier des charges se contredisait d'ailleurs à deux pages d'écart : sa table des
risques note qu'un `nanoTime` coûte vingt à trente nanosecondes et que le niveau `TIMED`
ne se pose que sur des méthodes « suffisamment longues ». Douze instructions ne l'est
manifestement pas.

**Règle** : devant un « DOIT être refusé si X », se demander si l'on en a déduit « DOIT
être accepté si non-X ». Et confronter les seuils chiffrés d'une spécification à ses
propres affirmations sur les coûts : ils doivent être cohérents entre eux.

### 2026-09-06 | Un mod synthétique qui n'ajoutait que deux méthodes sondées | Reproduire la largeur, pas seulement la charge

Le premier mod synthétique tickait fort — 64 unités, 40 gestionnaires, 2 fils — et
ajoutait **deux** méthodes sondées, là où un pack de 272 mods en apporte 2 649. Il
produisait du travail, pas de la surface instrumentée, alors que les campagnes venaient
d'établir que le coût vient du **nombre** de méthodes portant une sonde (ADR-021).

La table de la PARTIE 22 dit « 150 mods synthétiques » : c'est une demande de largeur —
des centaines de classes — pas un mod tické fort. Il a fallu un générateur de sources.

**Règle** : quand on reproduit une charge, se demander de quelle *dimension* elle est
faite. Un modpack, ce n'est pas une grosse boucle, c'est beaucoup de code différent.

### 2026-09-06 | Les classes chargées avant l'armement ne sont jamais sondées | Une classe ne se charge qu'une fois

Les 120 unités engendrées se chargeaient à la construction du mod, 23 secondes avant que
le transformateur s'arme. Elles passaient devant lui sans identifiant de sonde à
distribuer, ressortaient inchangées — et ne pouvaient plus jamais être sondées.

La conséquence dépasse le mod synthétique : **toutes les classes chargées pendant la
construction et l'enregistrement des mods échappent à l'instrumentation.** Les 2 649
méthodes mesurées ne sont pas « celles du modpack », ce sont « celles chargées après le
setup commun ».

**Règle** : pour tout mécanisme qui s'arme en cours de démarrage, se demander ce qui est
déjà passé avant, et si cela repassera. Pour le chargement de classes, la réponse est
non.

### 2026-09-06 | J'ai supposé « beaucoup », le compteur a dit 32 712 | Un compteur qui ne compte que le cas nominal ne mesure rien

La leçon précédente identifiait le bon problème et s'arrêtait à une intuition. Il a
suffi d'un compteur pour la chiffrer — et le chiffre n'était pas devinable :

- 32 712 des 76 327 classes visées passent avant l'armement, soit 43 % ;
- mais 4 875 seulement passent avant la construction du mod.

Autrement dit, **85 % de la perte tient à une fenêtre de trente-huit secondes** qu'on
pouvait fermer avec un réglage, pas à une limite d'architecture. J'aurais pu conclure
« c'est structurel, il faudrait déplacer le natif dans la couche d'amorçage » — une
refonte, pour un problème qui se réglait autrement.

Ce qui empêchait de le voir : `CLASSES_SEEN` était incrémenté **après** le retour
anticipé du cas non armé. Le compteur ne comptait que ce qui allait bien.

**Règle** : un compteur placé dans le chemin nominal ne dit rien du chemin qui échoue.
Quand on veut savoir ce qu'on rate, compter les ratés — pas les réussites. Et avant de
conclure qu'une limite est structurelle, mesurer où elle mord réellement.

### 2026-09-06 | La CI n'avait jamais reussi, et je n'avais jamais regarde | Vert en local ne veut rien dire

Zero succes sur quarante runs. Trois defauts, tous invisibles depuis Windows :

1. `gradlew` et les scripts de banc enregistres en `100644` — « Permission denied »
   sur Linux, code 126, avant d'executer quoi que ce soit ;
2. l'etape `lint-no-fiction` citait `FondationsTest`, renomme `FoundationsTest` lors
   du passage du code a l'anglais ;
3. `the_hot_path_never_allocates` **instable** : le meme code Rust rendait 0 ou 4
   allocations d'un commit a l'autre. L'allocateur compteur etait global au processus
   et comptait aussi le harnais de test, qui alloue sur un autre fil.

Le troisieme est le plus instructif. Le test ne mesurait pas ce que R-320 enonce : il
mesurait le processus, pas le chemin chaud. Rendu par fil — cellules initialisees en
`const`, donc sans destructeur ni initialisation paresseuse — il mesure enfin son
invariant. Et il porte desormais un second test qui verifie que le compteur compte,
parce qu'un comptage par fil mal cable rendrait le premier vide.

**Regle** : annoncer « tests verts » sur la foi d'une execution locale n'engage rien.
Lire la CI apres chaque poussee, ou ne pas en avoir. Et un test instable est un defaut
du test — presque toujours parce qu'il mesure plus large que ce qu'il affirme.

### 2026-09-06 | Un blob CBOR valide et indéchiffrable | Suivre la convention qui est déjà là

`/rfx top` répondait « aucun classement disponible » alors que le natif en produisait
un. J'avais déclaré `calls_per_tick: f64`. Or `CborReader` ne décode du type majeur 7
que les booléens — choix assumé, un décodeur qui traverse une frontière de confiance a
d'autant moins de défauts possibles qu'il accepte moins de choses. C'est exactement
pourquoi `RuntimeStatus` écrit déjà `overhead_pct_x100`.

La convention était dans le fichier voisin. Je ne l'ai pas lue avant d'écrire.

Plus grave : **rien ne l'a signalé avant un lancement de serveur.** Corrigé par un test
qui sérialise le modèle et parcourt le CBOR produit avec un décodeur reflétant les
limites de celui de Java, en échouant sur tout ce que Java refuserait.

**Règle** : avant d'ajouter un modèle qui traverse une frontière, lire un modèle qui la
traverse déjà. Et toute contrainte d'interopérabilité doit avoir un test des deux côtés,
sinon elle se vérifie en production.

### 2026-09-06 | Le protocole en deux temps n'était pas atomique | Une taille lue n'est pas une taille garantie

Demander la taille d'un blob, allouer, puis copier : entre les deux appels, un fil de
mod peut appeler `transferProbe` et faire changer ce qu'il y a à lire. Le second appel
refuse un tampon devenu trop petit, et Java croit le runtime muet alors qu'il répond.

`rfx_status` avait le même défaut **depuis le début**, latent : sa taille varie moins,
donc la course ne s'était jamais manifestée. Je ne l'ai vu qu'en écrivant une seconde
fonction sur le même patron, et en la voyant échouer une fois sur deux.

**Règle** : un protocole « interroger puis lire » suppose que rien ne change entre les
deux. Si l'état est partagé, cette supposition est fausse — reprendre la taille que le
refus rapporte, avec un nombre d'essais borné. Et un défaut trouvé dans du code neuf
doit être cherché dans l'ancien qui partage le même patron.

### 2026-09-06 | Trois hypothèses, une seule mesurée | Instrumenter au premier échec, pas au troisième

`/rfx top` échouait une fois sur deux. J'ai avancé trois causes :

1. une convention non suivie (`f64` là où le décodeur ne lit pas les réels) — juste,
   mais ce n'était pas *ce* défaut ;
2. une course dans le protocole « interroger la taille puis copier » — fausse pour ce
   symptôme, quoique le défaut fût réel et ait été corrigé au passage ;
3. un `u64` au-delà de 2^63 — la bonne, trouvée en une minute par un journal.

Le motif aurait dû me mettre sur la voie plus tôt : l'échec **alternait avec une période
de deux**, et une course ne produit pas de période. J'ai raisonné deux fois de plus
qu'il n'aurait fallu, pour deux lancements de serveur de douze minutes chacun.

Pire : le garde-fou CBOR écrit le jour même pour empêcher ce genre de défaut **a laissé
passer celui-ci**. Il vérifiait la forme — types majeurs, longueurs — jamais le domaine
des valeurs. Et son exemple de test employait un identifiant qui tenait dans un `long`.

**Règles** :
- un symptôme régulier (période, seuil, alternance) désigne une cause déterministe ;
  cesser d'envisager des courses ;
- au premier échec inexpliqué, ajouter le journal qui distingue les causes possibles,
  avant d'émettre la deuxième hypothèse ;
- un garde-fou doit être mis en échec par un test dédié. Sans quoi il ne prouve rien, et
  il rassure — ce qui est pire.

### 2026-09-06 | Cinq campagnes ont mesuré un système dont les sondes étaient éteintes | Mesurer ce que le système FAIT, pas ce qu'il déclare

`/rfx top` ne rapportait qu'une unité mesurée sur 2 541, toujours par échantillonnage.
Cause : **deux défauts empilés**, tous deux silencieux.

1. `RfxProbes.install(sink, levels)` n'était appelé **que par les tests**. En production
   `sink` restait nul, et `enter()` sortait à sa première ligne. Le bytecode injecté
   s'exécutait pour rien.
2. `pending_probe_levels()` ne recharge la table que si l'attente est vide. Une
   interrogation faite avant qu'une unité n'existe y laissait `Some(vide)`, que
   personne n'accuse jamais — et plus aucune table n'était livrée de toute la partie.

Pendant tout ce temps le mod annonçait « 2 647 méthodes sondées ». C'était vrai : elles
étaient *transformées*. Aucune n'était *armée*. Deux choses très différentes que le
journal et le fichier de campagne confondaient sous un seul nombre.

Conséquence : le surcoût de +6,5 % certifié par la campagne de ce soir est celui de
sondes qui ne mesuraient rien.

**Règles** :
- un compteur de ce qui est *installé* ne dit rien de ce qui *fonctionne* ; publier les
  deux, côte à côte, et faire en sorte que leur écart saute aux yeux ;
- un fichier de campagne doit porter de quoi invalider la campagne elle-même. Il porte
  désormais `probes_armed`, `probe_sink_installed` et `probe_records_failed` : la
  signature de cette panne est `probes_armed: 0` avec `methods_probed` élevé ;
- un cache qui n'est vidé que par un accusé de réception doit refuser de retenir une
  valeur vide, sinon un appelant qui n'a rien reçu le bloque pour toujours.

### 2026-09-07 | Case cochée sur une exécution, décochée par la suivante | Une observation n'est pas un résultat

J'ai coché « les zéros de la ligne de base sont expliqués » après **une** exécution qui
donnait 3,01 %. La suivante, même configuration, a donné 0,0 % et un profileur monté à
`DEEP` au lieu d'être éteint. Les sondes mortes étaient une cause, pas la cause.

Le pire est que je venais d'écrire, deux heures plus tôt, qu'une paire isolée ne prouve
rien — et j'ai conclu sur un point unique parce que le chiffre allait dans le sens que
j'espérais.

**Règle** : avant de cocher une case sur une observation, se demander combien
d'exécutions la soutiennent. Une seule ne ferme rien, surtout quand elle confirme ce
qu'on souhaitait. Écrire « observé une fois » plutôt que « expliqué ».

### 2026-09-07 | Trois fois le même défaut de frontière en une journée | Un vérificateur de contrat, pas trois corrections

Trois valeurs ont rendu illisible **tout un blob CBOR**, jamais seulement leur champ :

| valeur | ce qui devenait muet |
|---|---|
| `f64` dans le classement | `/rfx top` en entier |
| `u64` ≥ 2^63 pour le `work_id` | une invocation sur deux |
| `i64` dans le statut | statut, niveau du profileur, ligne de base |

À chaque fois j'ai corrigé l'instance et écrit un garde-fou un peu meilleur ; à chaque
fois le suivant est passé au travers. Le garde-fou du premier vérifiait la forme, pas le
domaine — le deuxième est passé. Le deuxième vivait dans les tests de `rfx-model` — le
troisième, dans `rfx-core`, n'y avait pas accès.

**Règle** : quand un même défaut revient une troisième fois, cesser de corriger
l'instance. Le contrat d'interopérabilité devient une **fonction publique** que tout
module peut appeler sur son propre modèle, avec un message d'erreur qui dit quoi
corriger. Et l'appliquer aux modèles **déjà en service**, pas seulement au dernier écrit
— c'est là que dormait celui-ci.

Corollaire : un décodeur minimal est une bonne chose, mais sa minimalité est un
**contrat**, pas un détail d'implémentation. Il doit être testé des deux côtés — ce que
Rust produit, et ce que Java accepte.

### 2026-09-08 | « Ça ne converge pas » alors que ça convergeait par en dessous | Ranger par qualité de mesure, pas par paramètre

Cinq cadences ont rendu 0 · 1,77 · 1,85 · 4,08 · 6,29 %. Rangées par taille de fenêtre,
ça ressemblait à une divergence, et c'est ce que j'ai conclu à voix haute.

Rangées par **niveau de bruit**, elles montrent une approche monotone par en dessous.
Une médiane de différences **signées** est biaisée vers zéro quand le bruit domine : le
bruit négatif la tire vers le bas et le signal, trop petit, ne peut pas la ramener. À la
limite elle rend exactement 0 — ce qu'a fait la fenêtre de vingt ticks.

Les petits chiffres n'étaient pas des coûts faibles, c'étaient des mesures ratées. J'ai
annoncé successivement 3,01 % puis « 70 µs, presque quatre fois moins », en croyant
corriger une erreur alors que j'en ajoutais une.

**Règles** :
- quand une série de mesures semble diverger, la ranger par **qualité de mesure** avant
  de conclure. Le paramètre qu'on a fait varier n'est pas forcément l'axe pertinent ;
- publier avec chaque estimation de quoi juger si elle vaut quelque chose. Ici trois
  nombres — queue négative, queue positive, proportion de cycles du bon signe — ont
  suffi à retourner l'interprétation ;
- se méfier d'un estimateur robuste appliqué à un signal noyé : la médiane ne protège
  pas du biais, elle protège des valeurs aberrantes.

### 2026-09-08 | L'horloge était lue pour remplir un champ que personne ne lit | Vérifier qui consomme, pas seulement qui produit

`RfxProbes.record()` appelait `System.nanoTime()` pour horodater chaque enregistrement
de sonde. Or `timestamp_ns` n'est lu **nulle part** côté natif : `ingest` ne consulte
que l'identifiant, le genre et la valeur. Le code Rust le dit même explicitement —
« niveau COUNTER : le passage est enregistré sans lecture d'horloge » — et le
commentaire Java disait la même chose. **Les deux étaient faux.**

Pire, `exit()` lisait l'horloge deux fois par appel chronométré : une pour la durée,
une seconde à l'intérieur de `record` pour l'horodatage.

Au niveau `LIGHT`, toutes les sondes sont à `COUNTER` : c'était donc vingt à trente
nanosecondes (PARTIE 15) par appel de chaque méthode sondée, jetées aussitôt.

**Règles** :
- un champ d'un format publié n'est pas forcément lu. Avant de payer pour le remplir
  sur un chemin chaud, chercher son **consommateur** — pas sa définition ;
- deux commentaires concordants ne valent pas une vérification. Ici le commentaire Java
  et le commentaire Rust affirmaient tous deux l'absence de lecture d'horloge, et
  l'appel était entre les deux ;
- une valeur qu'on calcule déjà ne se recalcule pas trois lignes plus bas.

### 2026-09-09 | Le coût est passé sous la résolution de son propre instrument | Une mesure qui ne détecte plus rien n'est pas une mesure à zéro

Deux corrections trouvées par lecture — l'horloge lue pour un champ que personne ne lit,
puis le comptage qui franchissait la frontière un appel à la fois au lieu d'un lot par
tick — ont fait passer le relevé de **248 900 ns à 7 550 ns**.

Mais les cycles positifs sont tombés de 17/20 à 11/20, presque un tirage à pile ou face,
avec un plancher de bruit à 628 µs. Autrement dit **le signal est passé sous la
résolution de la méthode**. Le chiffre de 7 550 ns ne dit pas « le coût vaut 7,5 µs » ; il
dit « la médiane de différences bruitées se pose près de zéro », ce qui est exactement le
comportement décrit par ADR-024.

**Règle** : quand une optimisation fait tomber les indicateurs de qualité de la mesure en
même temps que le chiffre, ne pas annoncer le chiffre. Annoncer la **borne** : le coût est
désormais inférieur à ce que l'instrument sait distinguer. C'est une affirmation plus
faible et plus vraie, et elle reste utile — elle dit que le sujet est clos.

### 2026-09-09 | « rust ok » annoncé alors que la compilation des tests échouait | Un filtre ne voit que le mode d'échec qu'il cherche

J'ai vérifié la santé du Rust par `cargo test | grep -c FAILED`, obtenu zéro, et annoncé
« rust ok ». Or une **erreur de compilation n'est pas un `FAILED`** : l'ajout d'un champ
`tick_ns` cassait la construction d'un `TopWorkloads` dans un test de `rfx-model`, et le
filtre ne pouvait pas le voir. Le message a même été imprimé dans la sortie du run, juste
au-dessus de l'erreur qu'il contredisait.

C'est la même famille que `CLASSES_SEEN` incrémenté du mauvais côté du retour, et que le
compteur d'accroches qui confond dépassement et pause GC : **un contrôle qui ne cherche
qu'un mode d'échec déclare sain tout ce qui échoue autrement.**

**Règle** : pour juger qu'une chaîne d'outils est verte, se fier au **code de sortie**,
pas à l'absence d'un motif dans la sortie. Et quand on filtre malgré tout, inclure les
erreurs de compilation — `^error`, `-->` — au même titre que les échecs de test.

### 2026-09-09 | Forcer `heat` dans un test ne prouve rien | Produire l'état comme la production le produit

Pour vérifier qu'une unité chaude peut être chronométrée, j'ai écrit
`entry.dynamics.heat = Heat::Critical`. Le test échouait sans que le code soit en cause :
la consolidation **recalcule la chaleur à chaque tick** depuis le coût observé, et une
unité sans mesure retombe froide immédiatement.

Le test ne testait donc pas ce qu'il annonçait — il vérifiait qu'un champ écrasé une
milliseconde plus tard avait un effet.

**Règle** : pour amener un composant dans un état, emprunter le chemin que la production
emprunte. Écrire l'état final à la main ne vérifie que la ligne qu'on vient d'écrire, et
masque les recalculs qui, eux, décident vraiment.

### 2026-09-09 | J'ai comparé une réduction de coût unitaire à une multiplication de total | Deux facteurs d'un produit ne se compensent pas par décret

ADR-022 refusait l'armement anticipé parce qu'il multiplie par cinq le nombre de sondes.
Le coût par sonde ayant depuis été divisé par trente, j'ai conclu : *« multiplier par
cinq un coût trente fois moindre ne redonne pas le coût d'origine »*, et j'ai basculé le
défaut.

La mesure a donné **4,78 % du MSPT contre 1,41 %** — exactement le rapport des nombres de
sondes. Le coût par sonde est le même dans les deux configurations : **104 ns** contre
**111 ns**. La réduction unitaire était réelle, mais elle s'applique aux deux côtés de la
comparaison ; elle ne finance pas la multiplication du nombre.

J'ai raisonné comme si un facteur pouvait en annuler un autre parce qu'ils étaient du
même ordre de grandeur, alors qu'ils portent sur des dimensions indépendantes du même
produit.

**Règles** :
- avant de conclure qu'une amélioration en finance une autre, écrire le produit et
  vérifier lequel des facteurs chacune touche ;
- une décision renversée doit l'être par une mesure, jamais par un raisonnement — j'avais
  écrit l'ADR avant la vérification et j'ai eu raison de ne pas le commiter ;
- une mesure qui infirme une hypothèse vaut plus qu'une qui la confirme : celle-ci a
  produit un chiffre durable, 105 ns par sonde armée et par tick, qui convertit une
  décision binaire en budget.

### 2026-09-10 | ADR-021 choisissait un seuil sans savoir combien de méthodes il écarte | Un paramètre choisi par raisonnement doit devenir balayable

ADR-021 a porté le seuil de sondage de 12 à 64 instructions sur une intuition juste — une
méthode courte est appelée bien plus souvent, donc sa sonde coûte plus qu'elle n'apprend.
Mais l'ADR ne disait pas combien de méthodes ce seuil écarte, ni ce que coûterait l'autre
choix. Un an de code plus tard, personne n'aurait pu le rouvrir sans tout refaire.

Le recensement dit **14 113 méthodes écartées** que R-311 autoriserait, soit 84,5 % de
cette population. Le balayage dit **14,58 % du MSPT** au seuil 12 contre un budget de
1,5 %. Et il donne le chiffre que l'intuition ne pouvait pas donner : les méthodes
écartées coûtent **412 ns par sonde et par tick** contre **104 ns** pour celles qu'on
garde — quatre fois plus chacune.

**Règles** :
- quand une décision fixe une constante, exposer cette constante avant d'écrire l'ADR :
  un seuil configurable se balaye en deux exécutions, un seuil en dur se re-débat ;
- le releveur le mieux mesuré du projet est celui où le coût explose — 9 cycles positifs
  sur 10. Quand on cherche à valider une hypothèse coûteuse, la configuration *mauvaise*
  se mesure mieux que la bonne, et c'est elle qui tranche ;
- trois mesures indépendantes désignent maintenant la même impasse : on ne peut pas
  acheter de la couverture en armant plus de sondes. Une conclusion répétée trois fois
  n'est plus une hypothèse, c'est une contrainte de conception — il faut arrêter de la
  retester et changer de mécanisme.

### 2026-09-10 | J'ai relancé des builds Gradle pendant une campagne A/B, et le biais allait dans le sens de ma conclusion | Une mesure de coût exige que rien d'autre ne tourne, et l'ordre doit être équilibré

Pour savoir si consigner les trames inconnues coûte quelque chose, j'ai lancé deux
exécutions du serveur — recensement éteint, puis allumé — et pendant la **première**,
j'ai enchaîné `compileJava`, `build` et `test`. La seconde a tourné sur une machine
libre. Elle est sortie meilleure, et j'ai failli en conclure que le recensement était
gratuit.

C'est la deuxième fois : j'avais déjà lancé `/rfx profile 180` au milieu d'une mesure de
couverture. La règle avait été écrite, elle n'a pas suffi — parce qu'elle disait « ne pas
confondre deux expériences » et que je ne voyais pas un `gradlew build` comme une
expérience.

Un second défaut se lisait dans les mêmes chiffres : quatre exécutions de la nuit
donnent 36,8 puis 45,8 puis 52,2 puis 50,4 ms de MSPT médian. La dérive suit **l'ordre
des exécutions**, pas le code. Comparer une exécution de minuit à une exécution de deux
heures du matin ne mesure donc rien.

**Règles** :
- pendant une campagne de mesure, **la machine ne fait rien d'autre** — y compris
  compiler, y compris lancer les tests, y compris ce qui « ne dure que trente
  secondes » ;
- un A/B en deux exécutions successives ne survit pas à une dérive de la machine.
  Ordonner **sans / avec / avec / sans** : la dérive porte alors autant sur les deux
  configurations, et sa présence se lit dans l'écart entre les deux paires ;
- quand un résultat va dans le sens qu'on espérait, chercher le biais **avant** de le
  publier. C'est là qu'on regarde le moins.

### 2026-09-11 | Trois ADR ont cherché de la couverture là où elle ne pouvait pas être | Quand trois tentatives échouent de la même façon, le défaut est ailleurs que là où on cherche

ADR-021 (le seuil), ADR-026 (l'armement anticipé) et ADR-027 (le balayage du seuil) ont
tous trois essayé d'élargir la couverture en changeant **le nombre de sondes**. Tous
trois ont conclu que ça ne marchait pas. Chacun a produit une mesure correcte et une
conclusion juste — et aucun n'a demandé pourquoi le levier ne répondait pas.

Le défaut était en aval : `flush()` ne vidait que le tampon du thread qui l'appelait, et
seul le fil autoritatif l'appelait. Les sondes étaient posées, elles comptaient, et leur
comptage était jeté — 152 millions de passages, dix-sept threads.

Ce qui l'a révélé n'est pas une quatrième tentative du même genre, mais un instrument
qui regardait **ce qu'on ne mesurait pas** : le classement des trames inconnues a mis en
tête une attente sur un pool de threads, ce qui posait la question du sort des
enregistrements produits sur ces threads-là.

**Règles** :
- trois échecs du même levier ne se traitent pas par un quatrième essai : c'est le
  moment de mesurer **le chemin complet** entre la mesure et le résultat, maillon par
  maillon ;
- un compteur qui n'est jamais lu ne prouve rien. `dropped` existait, s'incrémentait, et
  n'était annoncé au natif **qu'au vidage** — donc jamais, pour les threads qui ne
  vidaient pas. Un compteur de pertes doit être publiable sans dépendre du chemin qu'il
  surveille ;
- une variable de thread ne s'énumère pas, donc rien ne peut interroger l'ensemble des
  threads. Quand un état est par thread, prévoir dès le départ le moyen de le recenser,
  sinon le défaut qui le concerne sera invisible par construction.
