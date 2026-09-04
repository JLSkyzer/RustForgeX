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
