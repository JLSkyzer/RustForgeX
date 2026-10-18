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

### 2026-09-11 | Une campagne a rendu « la couverture ne bouge pas » alors qu'elle mesurait un garde-fou qui s'était déclenché | Vérifier que l'instrument est resté allumé avant de lire ce qu'il rend

La campagne de couverture ouvrait une fenêtre de chronométrage sur toute la fenêtre
mesurée, puis lisait la part du tick expliquée : **0,12 % sans la correction, 0,09 %
avec**. Conclusion apparente : le vidage multi-thread n'achète rien.

Le journal disait pourtant « fenêtre ouverte » dans les deux exécutions. Ce qu'il ne
disait pas, c'est que `apply_verdict` appelle `clear_requested_depth()` dès que le budget
est dépassé — voulu, correct, et immédiat sous cette charge. La campagne mesurait le
déclenchement d'ADR-020.

Ce qui m'a sauvé n'est pas de la prudence : c'est que le résultat était *trop* net. Une
couverture qui ne bouge pas d'un iota quand la collecte passe de 1 à 17 threads
n'était pas crédible, et c'est cette invraisemblance qui m'a fait relire le chemin.

**Règles** :
- « la demande a été acceptée » et « la demande est encore en vigueur au moment de la
  lecture » sont deux faits différents. Une mesure qui dépend d'un mode doit **vérifier
  ce mode à l'instant de la lecture**, pas à celui de la demande ;
- un garde-fou qui s'annule silencieusement se lit comme une absence de résultat.
  Tout mécanisme qui révoque une demande doit le consigner, sinon il produira des
  campagnes qui mesurent son déclenchement en croyant mesurer leur sujet ;
- choisir la grandeur que le changement affecte **réellement**. Ici le vidage
  multi-thread change ce qui est *collecté* (`observed`, les appels), pas ce qui est
  *chronométré* (`measured`). Mesurer la seconde pour juger la première ne pouvait pas
  marcher, fenêtre ou pas.

### 2026-09-11 | CI était rouge depuis huit jours et je ne l'avais pas regardée | La chaîne de vérification du CLAUDE.md se lance en entier, et le résultat du CI se lit après chaque push

`cargo fmt --all --check` échouait sur deux fichiers depuis le commit `0a10bf6`
(`/rfx profile`), que j'avais écrit. Trois exécutions du CI en échec, et je ne l'ai vu
qu'en lançant la chaîne complète pour une autre raison.

Deux manquements, pas un :

1. `CLAUDE.md` liste `cargo fmt --check && cargo clippy --all-targets -- -D warnings &&
   cargo test`. Je lançais `cargo test`, parfois `cargo build`, et je considérais le
   Rust vérifié. Un test qui passe ne dit rien du formatage, et le CI, lui, refuse les
   deux.
2. Je pousse après chaque modification — c'est la règle — mais je ne relisais pas le
   résultat. Pousser sans regarder revient à ne pas savoir si le dépôt est vert, tout en
   croyant l'avoir vérifié.

Ce dépôt a déjà eu 40 exécutions de CI en échec sans que personne s'en aperçoive. C'est
la deuxième fois.

**Règles** :
- après une modification Rust, lancer **les trois** commandes, dans l'ordre du
  `CLAUDE.md`, et pas seulement celle qui répond à la question du moment ;
- après un push qui touche du code, vérifier l'état du CI avant de passer à autre chose
  (`gh run list --limit 3`) ;
- quand `cargo fmt --check` signale un fichier que je crois ne pas avoir touché,
  vérifier **qui l'a écrit** avant de conclure à une divergence de version d'outil :
  ici c'était bien mon code, et l'excuse de l'outil m'aurait fait laisser le CI rouge.

### 2026-10-03 | Mon comparateur a conclu à une DIVERGENCE de RF-X sur ce qui était le hasard du jeu, deux fois de suite | Cause : j'ai jugé contre une référence non déterministe sans modéliser son bruit | Règle : contre une référence bruitée, A/A d'abord, puis un test symétrique muni d'une loi

G-03 compare des générations de monde avec et sans RUSTFORGE-X. Deux références sans
RUSTFORGE-X diffèrent déjà sur la moitié des chunks pour les blocs.

Premier critère : « est attribuable tout écart du candidat hors des chunks où les
références diffèrent ». Verdict DIVERGENCE, 242 écarts. Faux : le hasard ne frappe pas
toujours les mêmes chunks, et une troisième exécution sans effet en produit
mécaniquement hors de cet ensemble.

Second critère : « le candidat ne doit pas être l'intrus plus souvent que la plus
bruitée des références ». Échec à 3 contre 2 et 1, sur 6 en tout. Une règle sans loi
prend le hasard des petits effectifs pour un effet.

Le test retenu est symétrique — laquelle des trois exécutions est l'intruse — et a une
loi : sans effet, le candidat l'est avec une probabilité de 1/3, d'où une binomiale et
une p-valeur. RF-X intrus 239 fois contre 281 et 153 pour les références : p = 0,124.

**Règles** :
- avant de comparer un candidat à une référence, mesurer la référence contre
  elle-même. Sans A/A, aucun écart n'est interprétable ;
- un critère de divergence doit avoir une loi sous l'hypothèse « aucun effet ». Une
  règle de bon sens (un ensemble, un maximum) n'en a pas, et c'est elle qui crie au loup ;
- quand un verdict tombe, le confronter au cas le plus simple : « une troisième
  exécution sans effet passerait-elle ? » Les deux critères faux échouaient à ce test.

### 2026-10-03 | « compile fini » alors que Gradle n'avait rien compilé (récidive) | Cause : `grep "error:"` sensible à la casse ; Gradle a écrit `ERROR: JAVA_HOME is not set` | Règle : un build se juge à son code de sortie, et l'outillage se vérifie en début de session

La règle existait déjà (« rust ok » imprimé pendant un échec de compilation). Elle n'a
pas suffi parce que je filtrais encore la sortie au lieu de lire `$?`. Et la cause de
fond était ailleurs : la machine n'a plus aucun Java sur le PATH depuis la dernière
session ; seuls restent les JDK provisionnés par Gradle (`~/.gradle/jdks`).

**Règles** :
- toute commande de build ou de test se termine par la lecture de son code de sortie,
  jamais par un `grep` sur sa sortie ;
- en début de session, vérifier que `java`, `cargo` et `gh` répondent avant de
  compter sur eux. Faute de Java système, `JAVA_HOME` pointe sur
  `~/.gradle/jdks/eclipse_adoptium-17-amd64-windows/jdk-17.0.20.1+1` pour Gradle, et le
  serveur de banc tourne avec `eclipse_adoptium-21-amd64-windows.2` (21.0.8).

### 2026-10-03 | Une installation MSI pour toute la machine a échoué sans jamais demander l'élévation | Cause : `/quiet` supprime toute interface, donc aussi l'invite UAC ; le MSI échoue (1925, puis 1603) au lieu de demander | Règle : depuis un processus non élevé, `/passive` et non `/quiet`, et vérifier l'état après chaque paquet

Mise à jour de l'entrée précédente : la machine a maintenant des JDK système, installés
le 2026-10-03 à la demande de l'utilisateur — Temurin 8, 17, 19, 25 et Zulu 15, sous
`C:\Program Files\Eclipse Adoptium\` et `C:\Program Files\Zulu\`. **Temurin 17 est le
Java par défaut** (`JAVA_HOME` machine, seule entrée Java du `PATH` machine). Un terminal
ouvert avant l'installation ne le voit pas ; Gradle exige toujours Java 17 pour ce dépôt.

Deux constats :
- winget passe `ADDLOCAL=ALL` à ces MSI : PATH, JAVA_HOME, JavaSoft et association
  `.jar` pour chaque JDK. `--override` remplace ces arguments ; `--custom` s'y ajoute.
- le MSI Zulu 15 a créé l'association `.jar` au niveau machine malgré
  `ADDLOCAL=ZuluInstallation` seul. Ce que dit `ADDLOCAL` ne suffit pas à prédire ce
  qu'un MSI écrit : relever l'état avant et après chaque paquet l'a montré, et c'est la
  seule façon de le savoir.

### 2026-10-03 | Une campagne de tests de gameplay a mélangé deux exécutions concurrentes et rejoué une partie de sa boucle | Cause : j'ai modifié `run-gameplay.sh` pendant qu'il tournait ; bash lit un script au fil de l'exécution, à une position dans le fichier | Règle : ne jamais modifier un script en cours d'exécution, et écrire tout script long sous la forme `main() { … }; main "$@"; exit`

Une première campagne avait été « arrêtée » par la limite de durée des tâches de fond :
seule l'enveloppe l'était, le script continuait. Je l'ai cru mort, j'ai modifié le
fichier pour changer l'échauffement, puis lancé une nouvelle campagne. Le vieux script,
relisant le fichier modifié à sa position courante, a rejoué une seconde série
d'exécutions ; le nouveau a lancé son échauffement en même temps sur le même serveur,
qui a planté au démarrage. Vingt-cinq minutes de résultats inutilisables.

**Règles** :
- un script lancé reste intouchable jusqu'à sa fin. Pour corriger, l'arrêter d'abord et
  vérifier qu'aucun processus n'en reste (`Get-CimInstance Win32_Process` sur la ligne
  de commande) ;
- un script long s'écrit `main() { … }` puis `main "$@"; exit $?` : bash lit une
  fonction en entier avant de l'exécuter, et l'`exit` l'empêche de relire la suite ;
- toute campagne qui pilote une ressource partagée (un serveur, un dossier de monde)
  prend un verrou, et le refuse s'il existe ;
- « la tâche a été arrêtée » se vérifie sur la liste des processus, pas sur la
  notification ;
- une campagne vide ses résultats précédents au départ : un fichier ancien lu comme
  nouveau a déjà fait déclencher une attente trop tôt.

### 2026-10-03 | Le CI était « en échec » sur quatre commits sans avoir jamais tourné | Cause : quota de minutes du dépôt privé épuisé ; je n'ai pas relu le CI après chaque poussée de la session (récidive de la leçon de septembre) | Règle : lire l'état du CI après chaque poussée, et lire l'annotation d'un échec avant d'en chercher la cause dans le code

Les cinq jobs échouaient en trois secondes, `fmt` compris, sans journal. L'annotation
disait : « The job was not started because recent account payments have failed or your
spending limit needs to be increased ». Zéro étape exécutée : le code n'était pas en
cause, le CI ne tournait plus. Le dépôt est public depuis le 2026-10-03 (ADR-010) : les
exécuteurs standard y sont gratuits, et le run relancé est vert sur les cinq jobs.

**Règles** :
- un job sans aucune étape et sans journal n'a pas échoué sur le code : lire
  `gh api repos/<dépôt>/check-runs/<job>/annotations` avant tout ;
- la vérification du CI après une poussée n'est pas une formalité de fin de session :
  quatre commits sont passés sans qu'elle soit faite, alors que la règle existait.

Depuis la publication, ce dépôt commite avec l'adresse noreply de GitHub
(`git config user.email`, local à ce dépôt) : l'adresse personnelle ne figure plus que
dans l'historique antérieur au 2026-10-03.

### 2026-10-04 | Un test de gameplay a montré un écart de la référence 2 seule, venu de mon test et non du jeu | Cause : un échec voulu (`fill` trop grand) sortait de la zone contrôlée, et son message dépendait du chargement d'un chunk voisin ; une première version jugeait aussi la réussite d'une commande sur la valeur rendue, que `scoreboard objectives remove` met à 0 en réussissant | Règle : tout ce qu'un test d'égalité exécute — succès comme échec voulu — reste dans la zone qu'il contrôle ; l'issue d'une commande se lit dans le rappel de Brigadier, jamais dans sa valeur

Les deux défauts ont été pris par des gardes posées exprès : l'issue attendue de chaque
commande (le garde a signalé « Removed objective » comme un échec), et le test
symétrique (un écart de la référence 2 seule ne peut pas venir de RUSTFORGE-X). Avant de
conclure qu'un écart vient du jeu, relire ce que le test lui-même rend variable.

### 2026-10-06 | Une campagne G-09 a échoué sur les messages d'erreur alors que le candidat n'en avait aucun en propre | Cause : le critère des erreurs comparait le candidat à la seule référence 1, alors que l'état était déjà jugé de façon symétrique ; un message né du bruit du jeu, présent dans ref1 seule, a suffi | Règle : chaque critère d'un test à trois exécutions est symétrique — est imputable au candidat ce qui n'apparaît dans aucune des deux références, jamais ce qui diffère d'une seule

### 2026-10-10 | Le test de l'intrus a déclaré RUSTFORGE-X divergent sur une foule de 500 cochons, et la même alarme sortait sans RUSTFORGE-X | Cause : la loi binomiale suppose des éléments indépendants ; dans une foule, un écart fortuit se propage aux voisins et fait basculer des dizaines d'éléments d'un coup | Règle : avant de juger des éléments qui interagissent, faire une contre-épreuve A/A (CANDIDATE_RFX=false) ; si une référence y franchit le seuil, la méthode n'a pas de modèle nul valide et le test est déclaré non concluant, ni réussi ni échoué

### 2026-10-10 | Le rattrapage FM-09 du transformateur rendait la classe « telle quelle » alors qu'il venait de la modifier en place | Cause : l'injection modifie la méthode dans le ClassNode même ; un échec au milieu laissait une méthode à moitié injectée, et le commentaire promettait l'inverse | Règle : toute transformation en place qui peut échouer garde une copie de ce qu'elle modifie et la remet en cas d'échec ; un « rendu tel quel » se vérifie par un test qui fait échouer l'opération après qu'elle a commencé

### 2026-10-10 | Pour annuler une mutation de test, j'allais restaurer par git un fichier (TickCycle.java) qui portait des modifications non commitées | Cause : réflexe « restaurer = revenir à HEAD », valable seulement quand le fichier est propre ; le hook de commande destructive l'a arrêté | Règle : avant une mutation, copier le fichier dans le scratchpad et restaurer depuis cette copie ; ne revenir à HEAD que sur un fichier sans modification en cours, vérifiée par `git status`.

### 2026-10-11 | Télémétrie coupée, le rapport publiait quand même la durée de la garde du démarrage — deux fois, la première correction ne suffisant pas | Cause : la configuration est appliquée aux gardes depuis l'intérieur de la garde du démarrage ; les tests unitaires coupaient le chronométrage entre deux exécutions, jamais pendant une | Règle : quand un réglage s'applique à un composant depuis l'une de ses propres exécutions, tester ce cas précis (le réglage change pendant l'appel), et relancer en jeu après chaque correction — c'est le lancement qui a montré les deux défauts.

### 2026-10-12 | Le premier `/rfx status` d'une campagne de T-421 frôlait puis dépassait 5 ms (3,9 à 5,2 ms), alors que sa médiane est de 0,5 ms | Cause : le banc chronométrait la toute première commande du serveur, qui paie le premier passage dans le pipeline de commandes de Forge — mesuré à 4,85 ms pour un `seed` vanilla | Règle : un banc qui chronomètre des premiers appels exécute d'abord une opération témoin du même chemin, la chronomètre et la publie à part ; ce que coûte le chemin commun n'est pas imputé à la première commande mesurée.

### 2026-10-12 | ADR-025 attribuait l'arrêt du serveur sous `heavy` à la sauvegarde automatique ; sauvegarde coupée, le serveur s'arrêtait toujours | Cause : la cause avait été déduite du premier symptôme visible, sans rejouer l'échec sans RUSTFORGE-X ni sans la cause supposée | Règle : un blocage ou un crash sous charge se rejoue d'abord **sans RUSTFORGE-X** (transformateur retiré) avant toute attribution ; et une cause n'est retenue qu'après avoir vu l'échec disparaître quand on la retire.

### 2026-10-12 | Une campagne « arrêtée » a continué : sa boucle a lancé une exécution de plus, qui a occupé le port du serveur et fait échouer les deux exécutions suivantes, et le nettoyage du script a effacé deux fichiers bruts | Cause : sous Windows, arrêter la tâche de fond (le shell englobant) ne tue pas les scripts `bash` ni les `java` qu'elle a lancés ; et le script avait été modifié pendant son exécution, ce que bash lit au fil de l'eau | Règle : après tout arrêt d'une campagne, lister les processus `bash`/`java` qui portent ses arguments et les arrêter nommément ; ne jamais modifier un script pendant qu'une instance tourne ; sortir les résultats d'une campagne de son dossier avant d'en relancer une qui le vide.

### 2026-10-13 | J'ai écrit en gras dans ADR-036 et dans un message de commit que l'échantillonnage était « le coût dominant sous heavy », sur la foi d'une seule exécution ; diviser l'échantillonnage par cinq n'a pas fait baisser le coût | Cause : un « zéro » tiré d'une mesure à 4 cycles positifs sur 10, dans un bruit de ±10 ms par cycle, pris pour un signal ; la réserve était écrite, mais pas tenue jusqu'à la conclusion | Règle : sous une charge où la ligne de base varie du simple au triple entre exécutions identiques, aucune cause n'est écrite sans au moins trois exécutions de chaque côté ; une exécution isolée oriente l'expérience suivante, elle ne conclut pas.

### 2026-10-15 | J'ai attribué l'effondrement de la cadence d'échantillonnage sous `medium` à une mesure de coût faussée, en m'appuyant sur une contradiction avec ADR-025 ; le contrôle à cadence fixe a montré l'inverse (coût plus élevé sans régulation) | Cause : comparer deux mesures prises dans des conditions différentes (monde, MSPT 13 contre 20,8 ms) et lire leur écart comme le défaut d'un instrument | Règle : une contradiction entre deux campagnes aux conditions différentes n'accuse pas un instrument ; seule une comparaison où la seule variable est l'élément suspect le peut — la faire avant d'annoncer une cause, même « probable ».

### 2026-10-16 | Un test d'ADR-041 attendait qu'un fil terminé soit écarté juste après `ExecutorService.awaitTermination` ; il a échoué, le fil était encore vivant | Cause : l'exécuteur passe à « terminé » depuis son dernier fil, avant que ce fil ne sorte de `run()` ; `awaitTermination` ne garantit donc pas `isAlive() == false` | Règle : un test qui dépend de la mort d'un fil garde une référence au `Thread` et fait `join` dessus ; la fin d'un exécuteur n'est pas la fin de ses fils.

### 2026-10-16 | La première version d'ADR-041 a « attribué » 9 000 à 11 000 prélèvements et classé 9 à 14 unités ; 95 % étaient des fils de fond endormis dans une méthode sondée, et le classement était faux | Cause : un échantillonneur de piles mesure du temps écoulé ; étendre la lecture à d'autres fils sans regarder leur état fait compter l'attente comme du coût | Règle : tout échantillonnage de pile hors du fil principal filtre l'état du fil (`RUNNABLE`) ; un nombre d'attributions qui bondit se vérifie sur la nature des piles attribuées avant d'être lu comme un progrès.

### 2026-10-16 | J'ai écrit dans ADR-041 que le contrôle sans échantillonnage désignait l'échantillonnage comme cause du coût ; Killian jouait sur la machine depuis 03:30, le contrôle entier avait tourné en concurrence avec un jeu | Cause : je n'ai vérifié l'absence de jeu qu'au lancement de la campagne, jamais pendant ni à la fin, et le fichier d'exécution ne dit rien de la charge de la machine | Règle : une exécution de banc consigne elle-même la charge processeur de la machine hors serveur ; tant que ce n'est pas le cas, demander avant d'écrire une conclusion si la machine a servi pendant la campagne.

### 2026-10-16 | Un défaut volontaire dans la contre-pression a « survécu » à mon harnais de mutation ; en fait le test s'était bloqué, et le harnais, qui jugeait sur un grep, a pris le délai dépassé pour un succès | Cause : le test retient un worker avec une « porte » qu'il rouvre en fin de test ; une assertion qui échoue avant la réouverture laisse le worker bloqué, et l'arrêt de l'ordonnanceur l'attend sans fin | Règle : tout ce qui retient un worker dans un test se libère dans `Drop` ; un harnais de mutation lit le code de sortie et distingue « échoue », « passe » et « délai dépassé ».

### 2026-10-16 | Le plafond de l'arène était testé avec une capacité initiale bien inférieure au plafond ; le cas égal, celui d'un test d'intégration, laissait passer 1 Mio sous un plafond de 256 Kio | Cause : `bumpalo` 3.20 désactive son plafond quand la réservation le dépasse déjà, et `with_capacity` arrondit son premier bloc vers le haut ; le test unitaire n'essayait pas la borne | Règle : tout plafond se teste à sa borne (valeur initiale égale au plafond, juste au-dessus, juste en dessous), pas seulement loin de lui ; et le comportement d'une bibliothèque à sa limite se vérifie dans ses sources avant de s'y fier.

### 2026-10-16 | J'ai écrit dans l'ADR-047 et dans `arena.rs` que les allocations infaillibles de `bumpalo` terminaient le processus (`handle_alloc_error`) ; ses sources montrent un simple `panic!("out of memory")` | Cause : une affirmation sur le comportement d'une bibliothèque écrite de mémoire, alors que ses sources étaient sur le disque | Règle : une affirmation sur le comportement d'une dépendance se vérifie dans ses sources (ou sa documentation à la version utilisée) avant d'entrer dans un ADR ou un commentaire ; sinon elle est marquée « supposé ».

### 2026-10-16 | Après une campagne de mutations, un test a échoué une fois sans mutation (4,01 s) puis plus jamais ; j'ai d'abord cru à un test instable | Cause : le harnais restaurait le fichier par `mv` depuis une copie faite *avant* la mutation ; sa date de modification était plus ancienne que la dernière compilation, et Cargo, qui compare les dates, a gardé le binaire muté. La durée (20 × 200 ms) était la signature exacte de la dernière mutation | Règle : un harnais de mutation fait `touch` sur le fichier restauré ; un échec isolé se compare d'abord à la dernière opération sur les sources avant d'être déclaré instable.

### 2026-10-16 | Pour expliquer les 64,1 Mio de la base, j'ai accusé l'historique (« la file passe de 32 à 64 cases ») et écrit le correctif avec ce commentaire ; une mesure qui retirait ce correctif a montré des octets identiques : la file ne dépassait jamais 32, puisque le plus ancien part avant l'insertion. Tout le gaspillage venait des emplacements en ligne dans un `Vec` qui double | Cause : une explication de coût déduite à la lecture, prise pour un fait avant d'être isolée par une mesure | Règle : un correctif de performance ou de mémoire se garde seulement si la mesure qui le retire régresse ; sinon il est retiré, avec son commentaire.

### 2026-10-17 | Le test de survie de la base affirmait « écrite à l'arrêt » ; un défaut qui retirait la fermeture de l'arrêt l'a laissé passer | Cause : le test synchronisait juste avant l'arrêt, si bien que le fichier existait déjà ; l'effet vérifié avait deux chemins possibles, et le message en nommait un sans exclure l'autre | Règle : un test qui prête un effet à une cause construit un cas où seule cette cause peut le produire — ici, une unité enregistrée après le dernier lot — et le défaut qui retire cette cause doit le faire échouer.

### 2026-10-17 | Les classes de test du lecteur de bytecode contenaient un `tableswitch` et un `lookupswitch`, mais un défaut qui supprimait le remplissage d'alignement passait | Cause : les deux `switch` commençaient par hasard à une position où le remplissage vaut zéro ; le cas limite était dans la source, pas dans le bytecode produit | Règle : un cas limite se vérifie dans la donnée réellement produite (ici, les positions données par `javap`), et le test en fige la valeur exacte.

### 2026-10-17 | Le fil d'analyse passait 55 tests verts puis mourait en jeu sur une `NullPointerException` : le lecteur de modules de Forge renvoie `null` pour `list()` | Cause : `ModuleReader` supposé conforme à son contrat ; les tests construisaient leurs couches avec les lecteurs du JDK, qui le respectent | Règle : un code qui parcourt les modules du jeu est essayé dans le jeu avant d'être déclaré fini, et le test reproduit ensuite le lecteur réel (ici, `list()` à `null`). Constat vérifié par `javap -c` sur securejarhandler 2.1.10.

### 2026-10-17 | J'allais déclarer absentes les cibles de mixin relevées au lancement par `TargetScanner` ; ce relevé ne voit que les jars de premier niveau de `mods` et s'arrête au plafond de cibles | Cause : réutiliser une donnée produite pour un autre usage (protéger C-04, où un oubli coûte une sonde) dans un usage où un oubli rend un résultat faux (un graphe « résolu » à tort) | Règle : avant de réutiliser un relevé, vérifier qu'il est complet pour le nouvel usage ; une borne inférieure ne sert jamais de liste d'exclusion.

### 2026-10-17 | Une classe mixin fabriquée par ASM pour un test était lue comme illisible, et tout le test de hiérarchie échouait | Cause : le tableau d'une annotation imbriquée n'avait pas reçu `visitEnd()` ; ASM n'écrit la taille d'un tableau qu'à ce moment, et la classe produite était malformée. L'analyseur avait raison | Règle : devant un échec sur une donnée fabriquée, vérifier la donnée (ici, la relire hors de Gradle et lire l'exception exacte) avant de toucher au code testé.

### 2026-10-17 | Le cache des sous-types du graphe d'appel, faussé exprès (une seule clé pour toutes les classes), passait tous les tests | Cause : aucun test ne construisait un graphe qui interroge deux classes différentes dans la même portée du cache ; un cache n'est éprouvé que par deux clés distinctes | Règle : toute mémorisation s'accompagne d'un test à deux clés dans la même portée, et d'un mutant « clé constante » qui doit le faire échouer ; sur données réelles, comparer les résultats avant et après, champ par champ.

### 2026-10-17 | Deux tests du graphe d'appel affirmaient qu'un appel vers une classe absente reste « résolu » vers la référence écrite ; l'oracle JVM y a trouvé un faux résolu (`super.m()` vers une méthode par défaut) | Cause : l'attente du test avait été écrite d'après le comportement du code, alors que FM-15 dit l'inverse (« classe non trouvée => arête unresolved ») | Règle : un test qui fige un comportement cite la règle du cahier qu'il vérifie ; une attente qui contredit le cahier est un défaut, même quand le test passe.

### 2026-10-17 | L'oracle comptait 16 324 faux résolus : le fichier d'échange séparait les cibles par `;`, que contient tout descripteur JVM | Cause : séparateur choisi sans regarder le domaine des valeurs | Règle : avant de choisir un séparateur, vérifier qu'il n'apparaît dans aucune valeur ; un résultat aberrant d'un outil de vérification se vérifie d'abord dans l'outil.

### 2026-10-17 | Trois sites de LWJGL étaient invérifiables : le corpus du banc prenait la version de base d'un jar multi-release, la JVM la version `META-INF/versions/9` | Cause : un corpus construit par `zipfile`, qui ignore la règle de sélection des versions de la JVM | Règle : tout outil qui lit des classes dans des jars applique la règle multi-release (plus haute version ≤ celle de Java visée) ; en jeu, `ModuleReader.open` le fait déjà.

### 2026-10-17 | Deux messages de `rfx-bench` sont sortis avec dix espaces au milieu : la continuation `\` de fin de ligne, écrite dans du Rust par un script Python, avait disparu | Cause : le piège est déjà décrit dans `D:\BDC` (« Une chaîne Python non brute efface les continuations antislash du code qu'elle écrit »), mais la note n'a pas été relue avant d'écrire un script d'édition | Règle : un script qui écrit du code ne contient aucune continuation `\` ; couper la chaîne en deux appels, ou éditer à la main. Relire la sortie des messages modifiés : c'est elle qui l'a montré.

### 2026-10-18 | CI rouge sous Linux sur 13 poussées du 2026-10-17 (de C-07 à C-11), sans que je le voie : `the_workload_database_survives_a_restart` échouait au hasard (`rfx_status` refusé, `-1000`) | Cause technique : l'assistant de test `read_status` réimplémentait le protocole en deux temps (taille, puis copie) sans le réessai du vrai client (`jni_bridge::read_sized_blob`) ; le fil d'écriture de la base fait grandir le statut entre les deux appels. Cause de méthode : quatrième récidive de « lire la CI après chaque poussée » (2026-09-06, 2026-09-11, 2026-10-03) | Règle : un test lit une interface par le même code que le client réel, jamais par une copie simplifiée ; un test instable se reproduit de façon déterministe avant d'être corrigé, et le test de régression doit échouer sans la correction (vérifié ici en ramenant le réessai à une tentative). Une leçon écrite ne suffit plus pour la CI : il faut un mécanisme. Mécanisme en place le 2026-10-18, avec l'accord de Killian : hook `PostToolUse` asynchrone (`~/.claude/hooks/ci-after-push.py`), qui attend la CI de chaque commit poussé et réveille Claude si elle n'est pas verte ou pas vérifiable ; journal dans `~/.claude/hooks/ci-after-push.log`.

### 2026-10-18 | C-13 a d'abord rendu `order_sensitive = YES` pour une unité aux écritures inconnues, contre l'ADR-058 qui exige un effet « connu » | Cause : `SideEffectSet::any()` inclut `mutates_world`, que C-11 pose par prudence dès que les écritures contiennent `Domain::Unknown` ; `any()` n'est donc pas « un effet connu » | Règle : un effet de bord n'est connu que si `!side.unknown && side.any()` (forme déjà employée par `classify`) ; avant de lire un drapeau d'un ensemble composé, vérifier s'il est observé ou dérivé par prudence.
