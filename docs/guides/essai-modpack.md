# Essai de RUSTFORGE-X dans un vrai modpack

Procédure de la mesure demandée par l'ADR-072 et l'ADR-073 : quelles unités chaudes sont
pures et ce qu'elles coûtent (choix entre les options A et B), étalonnage A/A en jeu,
démarrage à chaud (T-384).

**Ce que fait cette version** : elle observe, mesure et analyse. Elle pose des sondes de
mesure dans les classes chargées, mais **ne change rien à la logique du jeu** : toutes
ses décisions restent `JAVA_ONLY`. C'est néanmoins un essai : faire une **copie de
l'instance** et jouer sur un monde qu'on peut perdre.

## 1. Préparer

1. Choisir une instance **Minecraft 1.20.1, Forge 47.x** (ATM9, From The Fog, Skyzer
   Adventure V2, Sky Walker…). Plus elle est chargée, plus la mesure est utile.
2. Dans CurseForge : clic droit sur l'instance → **Copier / Dupliquer**. Travailler sur
   la copie.
3. Copier **les deux** JAR de `D:\Mods Minecraft\RustForgeX\build\libs\` dans le dossier
   `mods\` de la copie :
   - `rustforgex-1.0-SNAPSHOT.jar`
   - `rustforgex-launch-1.0-SNAPSHOT.jar`

   Leurs empreintes SHA-256 sont dans `build\libs\SHA256SUMS`.
4. Les commandes `/rfx` demandent les **triches activées**. Sur un monde sans triches :
   menu Échap → **Ouvrir au LAN** → « Autoriser les triches : OUI ».

## 2. Première session (environ 45 minutes)

1. Lancer le jeu, ouvrir un monde **chargé** : une base avec des machines, des fermes,
   des mobs. Jouer normalement.
2. Vers **5 minutes** : `/rfx status` (dit si le runtime natif est actif).
3. Vers **30 minutes** :
   - `/rfx top 20` : les unités les plus coûteuses ;
   - `/rfx why` pour les **trois premières** lignes : **cliquer sur le nom** de l'unité
     dans `/rfx top` inscrit la commande complète dans la barre de saisie, il suffit de
     valider ;
   - `/rfx report` : écrit un rapport dans `rustforgex\reports\`.
4. Continuer à jouer jusqu'à 45 minutes, puis **quitter proprement** : menu → Sauvegarder
   et quitter, puis Quitter le jeu. C'est à l'arrêt que la base des unités est écrite.

## 3. Seconde session (environ 10 minutes, même monde)

1. Relancer le jeu et ouvrir **le même monde**.
2. Tout de suite : `/rfx status`.
3. Vers **5 minutes** : `/rfx report`.
4. Quitter proprement.

## 4. Envoyer

Depuis le dossier de la copie de l'instance, compresser en un seul `.zip` :

- le dossier `rustforgex\` en entier (`cache\workload.db`, `reports\`, `config\`) ;
- `logs\latest.log` et `logs\debug.log` (et le `.log.gz` de la première session s'il a
  été archivé) ;
- s'il y en a, les fichiers de `crash-reports\` de ces deux sessions.

Ajouter, en quelques mots : le modpack, la durée de jeu, ce que faisait la base, et toute
gêne ressentie (saccade, temps de chargement).

## Si quelque chose se passe mal

- **Le jeu ne démarre pas ou plante** : retirer les deux JAR de `mods\`, relancer, et
  envoyer le `crash-reports\` et `logs\latest.log`.
- **RUSTFORGE-X dit qu'il est inactif** : le jeu tourne normalement sans lui ; envoyer
  quand même les journaux, ils disent pourquoi.
- Pour le couper sans retirer les JAR : `general.enabled = false` dans
  `rustforgex\config\rustforgex.toml`.
