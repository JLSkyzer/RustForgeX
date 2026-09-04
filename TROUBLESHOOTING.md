# TROUBLESHOOTING

Symptômes, causes et actions. Référence : cahier des charges, PARTIE 27.1.

## En développement

### Le client est déconnecté en rejoignant le serveur de `run/`

**Symptôme** — `runServer` puis `runClient` : la connexion échoue immédiatement.
Dans le log du client :

```text
[ClientHandshakePacketListenerImpl]: Failed to log in: Invalid session
(Try restarting your game and the launcher)
```

**Cause** — Aucun rapport avec RUSTFORGE-X. Le client lancé par `runClient` démarre
sans session Mojang authentifiée. Un serveur en `online-mode=true` demande à Mojang de
valider cette session, n'obtient rien, et refuse la connexion. `enforce-secure-profile`
produit le même effet en exigeant une clé de profil signée.

**Action** — La tâche `prepareDevServer`, dont `runServer` dépend, bascule
automatiquement `run/server.properties` en `online-mode=false` et
`enforce-secure-profile=false`. Si le problème persiste, vérifier ces deux lignes :

```bash
grep -E "online-mode|enforce-secure" run/server.properties
```

Ce basculement ne concerne que `run/`, environnement de développement jetable et
ignoré par git. **Ne jamais désactiver `online-mode` sur un serveur exposé** : il
n'authentifierait plus personne.

Se connecter ensuite à `localhost` — inutile de passer par l'adresse publique de la
machine.

### Les journaux du client écrasent ceux du serveur

**Symptôme** — Après avoir lancé le client, `run/logs/latest.log` ne contient plus le
log du serveur ; l'ancien est archivé en `run/logs/<date>-N.log.gz`.

**Cause** — `runClient` et `runServer` partagent le même répertoire de travail `run/`,
donc le même dossier `logs/`. Le second processus démarré prend la main sur
`latest.log`.

**Action** — Les logs précédents ne sont pas perdus, seulement compressés :

```bash
ls -t run/logs/*.log.gz | head
gunzip -c run/logs/2026-01-01-1.log.gz | less
```

### Le serveur refuse de démarrer : EULA

**Symptôme** — `You need to agree to the EULA in order to run the server.`

**Action** — Ouvrir `run/eula.txt` et passer `eula=false` à `eula=true`, ce qui vaut
acceptation du [contrat de licence Minecraft](https://aka.ms/MinecraftEULA). Cette
acceptation appartient à l'utilisateur : aucune tâche du build ne la fait à sa place.

### Aucun binaire natif dans le JAR

**Symptôme** — Au démarrage : `RUSTFORGE-X inactif (DEGRADED)`, avec `E-1005`.

**Cause** — `cargo` est introuvable, ou la cible native de la plateforme n'est pas
installée. Le build produit alors un JAR sans natif, volontairement (PARTIE 23.5).

**Action** —

```bash
cargo --version           # doit répondre
./gradlew nativeManifest  # relance la chaîne buildNative → copyNative → hashNative
cat src/main/resources/natives/manifest.json
```

Le jeu reste parfaitement jouable dans cet état : RUSTFORGE-X ne fait simplement rien.

### Un avertissement mixin au démarrage

**Symptôme** — `Compatibility level JAVA_x specified by rustforgex.mixins.json is
higher/lower than ...`

**Cause** — `compatibilityLevel` ne correspond pas à la version de mixin embarquée par
Forge.

**Action** — Lire le niveau que le service annonce dans le message, et aligner
`src/main/resources/rustforgex.mixins.json` dessus. Ne pas le déduire de la version du
langage Java.

## À l'exécution

### `/rfx status` affiche « non mesuré »

Ce n'est pas une anomalie. La sonde matérielle (C-45) laisse à zéro tout champ qu'une
plateforme ne permet pas de mesurer, et l'affichage le dit explicitement plutôt que de
présenter un zéro comme une mesure. Le cache L3 et les nœuds NUMA, par exemple, ne sont
pas sondés sur toutes les plateformes.

### `RUSTFORGE-X inactif (DEGRADED)` dans les journaux

Le mod n'a pas pu activer son runtime natif et s'est retiré. **Le jeu tourne
normalement**, exactement comme sans le mod. Le message porte un code de l'annexe A.2
qui en donne la raison :

| Code | Cause | Action |
|---|---|---|
| `E-1001` | version de Forge hors de la plage supportée | utiliser Forge 47.x (Minecraft 1.20.1) |
| `E-1003` | empreinte du binaire natif invalide | le JAR est altéré ou incomplet : le retélécharger |
| `E-1005` | aucun binaire natif pour cette plateforme | voir « Aucun binaire natif dans le JAR » |
| `E-1006` | le système a refusé de charger la bibliothèque | vérifier les droits d'écriture et qu'aucun montage `noexec` n'est en jeu |
| `E-1002` | version d'ABI incompatible | le JAR mélange des versions : le reconstruire entièrement |

### Produire un rapport utile

Joindre :

1. `run/logs/debug.log` (ou `latest.log` en production) ;
2. la sortie de `/rfx status` ;
3. `<gameDir>/rustforgex/config/rustforgex.toml` ;
4. la version de Forge et la plateforme.

RUSTFORGE-X n'envoie jamais rien sur le réseau : tout diagnostic est local et c'est à
vous de choisir ce que vous transmettez.
