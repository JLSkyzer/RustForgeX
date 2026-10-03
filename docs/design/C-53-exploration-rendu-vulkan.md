# C-53 et au-delà — rendu multi-thread et moteur Vulkan : note d'exploration

Statut : **exploration, rien n'est décidé ni implémenté — matière du jalon M11 (ADR-031),
qui ne commence qu'après la 1.0.0** (décision de Killian, 2026-10-03). D'ici là, rien ici
ne doit infléchir C-53 ni aucun jalon de M0 à M10. Discussion entre Killian et l'assistant du
2026-10-03, tenue depuis une session AXION ENGINE et consignée ici à la demande de
Killian, pour qu'une session dédiée à RUSTFORGE-X l'analyse en détail le moment venu.

Cahier des charges concerné : C-53 Client Render Prep (PARTIE 16.7, `EXPERIMENTAL`, M7),
R-830, C-47 GPU Offload (`FUTURE`), R-100, R-102, R-700, INV-02, INV-12, H-02, H-04,
PARTIE 5.17.2. **Aucun gain n'est annoncé ici** : aucun ne peut l'être avant que le
harnais C-36 en ait mesuré un.

---

## 1. La vision de Killian

- À terme, faire passer dans un moteur Rust le rendu des entités et des blocs, le tick,
  la génération du monde, etc., et ne quasiment plus utiliser le moteur Java de
  Minecraft, jugé mono-cœur et mal armé pour le multi-thread.
- Idée discutée : un équivalent de VulkanMod, écrit en Rust, qui remplace le rendu OpenGL
  de Minecraft ; les rendus des mods sont interceptés pour passer dans ce moteur, qui
  répartit construction, calculs et dessin sur tous les cœurs.
- Intuition de Killian, **non mesurée** : le gain serait important si tout est bien
  fait, et les gros mods construiraient rarement leur géométrie à la main ou
  appelleraient rarement LWJGL en brut. Deux hypothèses à vérifier (§9).

## 2. Écarts avec le cahier des charges — à lire en premier

- **C-53 et R-830.** C-53 est « strictement limité à la préparation de données hors
  thread de rendu (culling géométrique, tri, construction de tampons), jamais aux appels
  graphiques » ; R-830 interdit tout appel OpenGL hors du thread de rendu, sans
  exception. Un moteur de rendu Vulkan qui remplace celui de Minecraft sort de ce
  périmètre : c'est une divergence au CDC, qui exige un ADR daté (contrat 1.4).
- **Bibliothèque native tierce.** Un chargeur Vulkan, `wgpu` ou `ash` est hors du
  périmètre de décision de l'agent (contrat 5) : la décision revient à Killian.
- **Correction prouvée d'abord.** Le principe du projet s'applique tel quel : on ne
  déporte que ce dont la correction est prouvée, et UNKNOWN = CONSERVATIVE — un rendu
  de mod non prouvé reste sur le chemin série.
- **INV-12.** Pendant la discussion, l'assistant a proposé des « correctifs ciblés pour
  les mods populaires ». Dans le moteur, ce n'est pas admissible : aucune logique ne
  dépend d'un nom de mod. Les adaptations doivent être génériques — déclenchées par un
  comportement observé ou par une déclaration du mod. Une compatibilité ciblée, si elle
  devait exister, serait à arbitrer à part.
- **Mixins.** Jamais `@Overwrite` (CLAUDE.md 4.4) : une redirection passe par un
  `@Inject` annulable ou un autre procédé admis.

## 3. Faits avancés pendant la discussion, et leur statut

| Fait | Statut |
|---|---|
| Un contexte OpenGL n'est actif que sur un thread à la fois : les appels de dessin d'une frame partent de ce thread | connaissance générale de l'API, non revérifiée ici |
| Vulkan, Direct3D 12 et Metal permettent d'enregistrer des listes de commandes (*command buffers*) sur plusieurs threads, puis de les soumettre en un envoi | connaissance générale |
| En Rust : `wgpu` (au-dessus de Vulkan, Metal et D3D12, utilisé par Firefox et Bevy) et `ash` (Vulkan brut) | connaissance générale |
| Zink implémente OpenGL au-dessus de Vulkan et en garde la sémantique : état global, exécution série | connaissance générale |
| Mojang a annoncé en février 2026 le passage de Java Edition d'OpenGL à Vulkan : snapshots prévues à l'été 2026, bascule OpenGL/Vulkan en snapshot et en version publique le temps de stabiliser, retrait d'OpenGL ensuite, beaucoup de mods touchés | **vérifié** par sources web le 2026-10-03 (voir Sources) ; avancement actuel non vérifié |
| VulkanMod (Fabric) remplace le moteur OpenGL de Minecraft par un moteur Vulkan ; projet expérimental | **vérifié** par sources web le 2026-10-03 ; sa compatibilité avec les mods de rendu : de mémoire, à vérifier |
| En 1.20.1, `ModelPart` multiplie lui-même chaque sommet par la matrice de la `PoseStack` avant d'appeler `VertexConsumer` | de mémoire du code 1.20.1, à vérifier dans le jar |
| `RenderSystem` vérifie l'appartenance au thread de rendu (`assertOnRenderThread`) | de mémoire, à vérifier |
| Vanilla fait déjà tourner hors du fil principal la génération des chunks (threads « Worker-Main »), l'éclairage et la construction des maillages de chunks côté client ; le goulet série est le tick serveur et le fil de rendu | de mémoire, à vérifier |
| Forge a introduit `ModelData` parce que la construction des maillages de chunks tourne hors du fil principal | de mémoire, à vérifier |
| Sodium (Embeddium sous Forge) tire ses gains surtout de moins d'appels de dessin, d'un meilleur culling et de la construction des chunks sur plusieurs threads, en restant en Java | de mémoire |
| Folia (fork de Paper) répartit le tick serveur par régions ; la plupart des plugins doivent être adaptés et se déclarer compatibles | de mémoire |
| Create rend une partie de ses objets via Flywheel, moteur d'instanciation sur OpenGL : contre-exemple possible à « les gros mods n'appellent pas OpenGL en brut » | de mémoire, à vérifier |
| PojavLauncher fait tourner Minecraft Java sur Android en traduisant OpenGL (gl4es, Zink) | de mémoire |

## 4. Ce qui se parallélise, et ce qui ne se parallélise pas

- **Parallélisable : ce que le moteur reprend lui-même** — chunks, modèles d'entités
  vanilla, particules, culling —, à condition de travailler sur une copie de l'état du
  monde prise à chaque frame (phase « extract », comme dans Bevy). Le tick client modifie
  le monde pendant ce temps, et INV-02 interdit de muter l'état autoritatif hors de son
  thread.
- **Parallélisable : le travail contenu dans les appels redirigés** — matrices,
  transformation des sommets, conversion de format, tri, culling, construction des
  listes de commandes Vulkan. L'appel n'écrit que quelques nombres dans un tampon ; le
  moteur fait le reste sur ses cœurs.
- **Non parallélisable : la production des appels.** Le code du mod les fait un par un,
  sur le fil de rendu ; le moteur ne peut ni traiter un appel avant qu'il soit fait, ni
  exécuter ce code ailleurs. Image retenue : dix cuisiniers ne servent pas plus vite si
  un seul serveur prend les commandes.
- **Non parallélisable : la géométrie construite à la main** dans le code d'un mod, et
  les **appels LWJGL bruts**, qui obligent à émuler OpenGL — une émulation qui hérite de
  sa sémantique série.

## 5. Pourquoi on n'exécute pas les rendus des mods en parallèle

Réécrire `PoseStack` et `RenderSystem` est la partie facile : une `PoseStack` par tâche,
un état de `RenderSystem` enregistré par thread, une vérification de fil assouplie. Le
code des mods qui s'en sert suppose qu'il tourne seul :

- **état partagé dans le mod** : champs statiques, caches, compteurs, `HashMap` remplies
  au premier usage ;
- **services de Minecraft chargés à la demande** : texture enregistrée au premier usage,
  glyphes de police, modèles d'items calculés à la volée — les verrouiller tous crée une
  file d'attente qui mange le gain ;
- **rendus qui modifient ce qu'ils lisent** : minuteur d'animation, cache sur l'entité,
  particule — une mutation hors thread autoritatif, que INV-02 interdit ;
- **ordre des dessins**, dont dépendent la transparence et l'état graphique — celui-là est
  soluble : chaque thread enregistre sa liste, rejouée dans l'ordre d'origine.

Ces conflits ne se voient pas forcément en test : ils donnent des défauts rares,
aléatoires, presque impossibles à reproduire.

## 6. La redirection

- **Au bon niveau.** `PoseStack` et `VertexConsumer` sont trop bas : le calcul par sommet
  est déjà fait quand l'appel arrive (§3). Rediriger plus haut : un `ModelPart` entier
  (« dessine cette pièce avec cette pose »), les quads des modèles précalculés de blocs et
  d'items, le texte. À ce niveau, la géométrie peut rester sur la carte, avec une matrice
  par pièce : le travail par sommet disparaît du CPU.
- **Par tampons entiers, jamais par appel** (R-700). L'enregistrement reste en Java, dans
  un tampon partagé avec le natif : un appel JNI par commande coûterait plus que l'appel
  d'origine (modèle de coût : H-04, C-45).
- **Shaders.** Les GLSL des mods et des shaderpacks doivent être traduits vers SPIR-V :
  l'un des plus gros chantiers de compatibilité.

## 7. Stratégie par paliers proposée

1. **En parallèle d'office** : ce que le moteur réimplémente, et les données pures
   (modèles de blocs précalculés).
2. **En parallèle sur adhésion** : les mods qui se déclarent sûrs par une API dédiée,
   comme les plugins Folia.
3. **En série pour tout le reste**, capturé par tampons entiers : UNKNOWN = CONSERVATIVE.
4. **Adaptations génériques**, jamais conditionnées par un nom de mod (INV-12).

## 8. Variante hybride

Rendre en Vulkan, sur plusieurs threads, dans une image partagée avec OpenGL
(`GL_EXT_memory_object`, `GL_EXT_semaphore`), que la frame OpenGL de Minecraft compose :
les mods OpenGL continuent de fonctionner. Impossible sur macOS, où OpenGL est figé en
4.1 sans ces extensions et où Vulkan passe par MoltenVK. Statut : connaissance générale,
non vérifiée.

## 9. Estimer le gain avant d'écrire du code

- **Plafond (loi d'Amdahl).** Si une part *p* du temps du fil de rendu peut quitter ce fil,
  il va au mieux 1/(1−*p*) fois plus vite. C'est un plafond mathématique, pas un gain
  annoncé.
- **Il dépend de la scène** : potentiellement élevé en vanilla chargé (beaucoup
  d'entités, de chunks), plafonné en modpack par la part des rendus de mods restés série,
  nul quand la carte graphique limite.
- **Protocole proposé.** Profiler (C-05, C-36) deux ou trois scènes représentatives —
  vanilla chargé, gros modpack — et découper le temps du fil de rendu en quatre parts :
  rendu vanilla que le moteur pourrait reprendre ; rendu des mods ; soumission au
  pilote ; attente du GPU. *p* en découle, scène par scène.
- **Hypothèses de Killian à mesurer** : la part de géométrie construite à la main et
  d'appels LWJGL bruts dans les gros mods (supposée faible), mod par mod — sans que le
  moteur ne s'en serve comme d'une liste de noms (INV-12).

## 10. Versions et maintenance

- La couche d'interception est à refaire à chaque version de Minecraft : les 1.21.x ont
  déjà beaucoup remanié le rendu (de mémoire), et Mojang passe à Vulkan.
- Sur 1.20.1, version figée et très moddée, le pari tient mieux. Sur les versions où
  Mojang fournit Vulkan, RUSTFORGE-X pourrait s'appuyer sur ce moteur au lieu de le
  remplacer.

## 11. Cohabitation avec AXION ENGINE

AXION est indépendant de RUSTFORGE-X — son invariant INV-06 lui interdit de le déclarer
ou de le nommer hors de son pont optionnel — et ne modifie ni Minecraft ni Forge. Pour
rester compatible avec lui, comme avec la plupart des mods, un moteur de rendu
RUSTFORGE-X devrait :

- continuer d'émettre les étapes de rendu de Forge (`RenderLevelStageEvent`), ou une
  accroche équivalente : AXION y dessine ;
- rendre l'état graphique tel qu'il l'a trouvé ;
- partager les cœurs. AXION garde `cœurs − 2` workers, et son option
  `integration.rustforgex.cpu_share` divise ce pool par deux en mode `auto` quand un autre
  consommateur est détecté — détection pas encore écrite côté AXION. RUSTFORGE-X réserve
  lui aussi deux cœurs (PARTIE 5.17.2) et réduit ses workers actifs sous charge externe :
  les deux règles sont à coordonner pour ne pas sursouscrire la machine.

Côté AXION, deux façons de dessiner sont prévues : un backend vanilla (par les
`RenderType`, sans appel OpenGL direct) et un backend natif (pipeline OpenGL dédié). Un
backend branché sur un moteur RUSTFORGE-X passerait par le pont optionnel d'AXION.

## 12. Décisions à prendre par Killian, et suite

1. Spécifier M11 par un ADR : étendre C-53 aux appels graphiques et à un moteur Vulkan,
   ou créer un composant dédié (contrat 1.4).
2. Choisir, ou non, une bibliothèque native tierce (`wgpu`, `ash`) : contrat 5.
3. Fixer la ou les versions de Minecraft visées, et la position vis-à-vis du Vulkan de
   Mojang.
4. Avant tout code : appliquer le protocole du §9 ; puis prototyper la capture de
   `ModelPart` vers un tampon d'enregistrement, mesurée par C-36.

## 13. Principe de M11 fixé par Killian (2026-10-03)

- Le rechargement des ressources passe aussi par le moteur.
- Toute méthode de Minecraft et de Forge qui finit par un appel OpenGL — pour un rendu ou
  autre chose — est redirigée vers le moteur, qui répartit le travail sur les threads.
- À la fin, OpenGL ne sert plus qu'aux mods qui l'appellent en brut, par LWJGL, au lieu de
  passer par les fonctions de Minecraft ou de Forge.

**Faisabilité, état au 2026-10-03.** Chaque brique a un précédent : relance du jeu
(CleanroomRelauncher, §13.9), choix de la bibliothèque OpenGL de LWJGL
(`org.lwjgl.opengl.libname`, §13.8), OpenGL sur Vulkan (Zink), moteur Vulkan pour Minecraft
(VulkanMod), point d'entrée unique de vanilla (`GlStateManager`, §13.1). Rien d'impossible
n'est apparu ; restent un chantier de l'ampleur de Zink et de VulkanMod réunis, et les
vérifications des §13.6, §13.8 et §13.9. La fenêtre reste celle de GLFW — clavier, souris
et événements inchangés —, mais l'image qui s'y affiche, la swapchain, est celle du moteur.

### 13.1 Le point d'entrée : Blaze3D

Relevé le 2026-10-03 dans le jeu patché par Forge 47.4.23
(`forge-1.20.1-47.4.23_mapped_official_1.20.1.jar`, 8 614 classes, pool de constantes) :
**huit classes seulement** référencent `org.lwjgl.opengl`.

- Sept sont de Blaze3D : `GlStateManager` (et `GlStateManager$BooleanState`), qui porte
  l'état et les appels de dessin (`GL11` à `GL32C`) ; `Window`, qui crée le contexte ;
  `GlDebug` ; `TimerQuery` et ses deux classes internes (minuteurs GPU). Tout le reste —
  `RenderSystem`, `BufferUploader`, `VertexBuffer`, `NativeImage`, `TextureUtil`,
  `RenderTarget` — passe par `GlStateManager`.
- Une est de Forge : `ForgeLoadingOverlay` (`GL30C`).
- Hors de ce jar, l'écran de chargement précoce de Forge
  (`fmlearlydisplay-1.20.1-47.4.23.jar`) appelle OpenGL en brut dans 9 de ses 26 classes
  (`GL32C`) ; `fmlcore` et `javafmllanguage` n'en ont aucune.

Rediriger `GlStateManager` capte donc Minecraft, Forge hors écrans de chargement, et tout
mod qui passe par eux ; c'est, de mémoire, le procédé de VulkanMod.

### 13.2 Deux niveaux, deux rôles

- **Niveau bas, la compatibilité** : traduire ces appels en Vulkan. Tout continue de
  fonctionner, mais les commandes arrivent dans l'ordre, depuis le fil de rendu : ce niveau
  change de backend, il ne répartit pas le travail.
- **Niveau haut, la performance** : reprendre des systèmes entiers, que le moteur prépare
  en parallèle en gardant la géométrie sur la carte. Les plus lourds dans un gros modpack
  (de mémoire du rendu 1.20.1) :
  - **terrain** : un appel de dessin par section de chunk et par type de rendu, et les
    faces translucides proches retriées sur le CPU ;
  - **block entities** : souvent redessinées entières à chaque frame, alors que leur
    géométrie bouge peu ;
  - **entités** : le travail par sommet du §6, animation comprise ;
  - **items dans les interfaces** : terminaux et listes d'items, un modèle à la fois ;
  - **texte**, et **particules**, mises à jour et dessinées une par une ;
  - **changements d'état** : un tampon vidé à chaque changement de type de rendu ; Vulkan
    précompile ses états (*pipelines*) et peut lier toutes les textures en une fois ;
  - **pression mémoire** : beaucoup de petits objets alloués par frame, qui nourrissent le
    ramasse-miettes et ses micro-saccades.
- **Répartition** : C-17 (scheduler, work stealing, ADR-005) équilibre dynamiquement, ce qui
  vaut mieux qu'un partage égal fixé d'avance. Le code Java des mods reste hors des
  workers : INV-06 interdit à un worker tout verrou JVM et tout moniteur Java.

### 13.3 Rechargement des ressources

- Vanilla décode déjà ses PNG en natif : `NativeImage` appelle `stbi_load_from_memory` de
  STBImage (relevé dans le jar). Le gain ne viendra donc pas du décodage, mais de ce qui
  reste en Java et en série — assemblage des atlas, cuisson des modèles — et du
  parallélisme. Une partie du rechargement tourne déjà sur des exécuteurs d'arrière-plan
  (de mémoire).
- Les accroches des mods (événements de cuisson des modèles, chargeurs de modèles propres,
  assemblage des atlas) sont du code Java qui s'exécute au milieu du rechargement : leur
  ordre et leur fil restent garantis.

### 13.4 OpenGL pour les seuls mods en brut

Deux voies, à arbitrer dans l'ADR de spécification de M11 :

- **Garder un vrai contexte OpenGL et composer.** Le moteur rend en Vulkan ; les dessins
  OpenGL des mods sont composés dans la frame par interop (`GL_EXT_memory_object`,
  `GL_EXT_semaphore`). Un mod qui dessine dans le monde attend le tampon de profondeur du
  monde : il faut le partager, avec une synchronisation au milieu de la frame à chaque
  passage. Impossible sur macOS.
- **Émuler OpenGL sur Vulkan pour ces mods**, à la manière de Zink, en donnant à LWJGL ses
  propres fonctions OpenGL (de mémoire, LWJGL laisse choisir la bibliothèque OpenGL qu'il
  charge). Plus d'OpenGL du tout, macOS compris par MoltenVK ; en échange, un émulateur à
  écrire ou à intégrer, et une exécution série pour ces mods.

### 13.5 Repli par capacité, macOS compris

- Principe de Killian : sur macOS, Minecraft garde OpenGL, et le moteur ne prend effet que
  sur ce que la machine permet.
- À décider par **capacité détectée** (C-45), pas par nom de système : la même règle couvre
  un PC Windows ou Linux sans Vulkan, avec un pilote défaillant, ou sans les extensions
  d'interop.
- Ce qui reste utile sans Vulkan : la préparation parallèle côté CPU — culling, maillages,
  données d'instances, rechargement des ressources — qui alimente OpenGL. macOS profite
  donc du multi-cœur, pas de la soumission Vulkan. OpenGL 4.1 y limite les techniques : ni
  compute shaders ni dessin indirect multiple (de mémoire).

### 13.6 Windows et Linux : presque tout, pas automatiquement tout

- Vulkan disponible et pilote fiable : à détecter, avec repli (§13.5).
- La couche de traduction doit reproduire toute la sémantique de Blaze3D : stencil, copies
  de framebuffer, relectures (cartes et minimaps), minuteurs GPU.
- Shaders GLSL des mods (`ShaderInstance`) et shaderpacks : à traduire vers SPIR-V.
- Mods OpenGL bruts : détectables sans liste de noms (INV-12), par leur bytecode — une
  classe qui référence `org.lwjgl.opengl`, comme le relevé du §13.1.
- Fenêtre : GLFW ne crée pas de surface Vulkan sur une fenêtre dotée d'un contexte OpenGL
  (documentation GLFW, de mémoire). Tant qu'un mod OpenGL brut est présent, c'est donc
  OpenGL qui présente la frame, Vulkan rendant hors écran dans des images partagées. Et
  l'écran de chargement précoce de Forge crée la fenêtre avec son contexte OpenGL avant
  Minecraft : M11 doit le remplacer, ou recréer la fenêtre après le chargement.

### 13.7 Mods OpenGL bruts : les détecter, les transformer

- **Détection au lancement**, générique (INV-12) : les classes qui référencent
  `org.lwjgl.opengl` dans leur bytecode. C-41 analyse déjà les mods.
- **Transformation au chargement**, comme C-04 pose ses sondes (ASM) : chaque appel
  `GL11.glXxx(…)` d'une classe de mod est redirigé vers l'implémentation OpenGL du moteur.
  Le mod n'en sait rien.
- **Limites** : ce que le bytecode ne montre pas — réflexion, `MethodHandle`, pointeurs de
  fonction obtenus à l'exécution, code natif propre au mod — échappe, d'où un repli
  nécessaire. Le code OpenGL de ces mods s'exécute toujours dans l'ordre : émulé par le
  moteur, pas parallélisé.

### 13.8 Le jar LWJGL et le « faux pilote »

- `org.lwjgl.opengl` vient de `lwjgl-opengl-3.3.1.jar`, dépendance de Minecraft installée
  par le launcher (relevé dans le cache Gradle) : une fine liaison vers l'OpenGL du pilote,
  pas OpenGL lui-même.
- Modifier ce jar est fragile : chaque launcher en a sa copie et contrôle ses bibliothèques
  (de mémoire), et ce serait redistribuer une bibliothèque tierce modifiée.
- Même effet sans toucher un fichier : LWJGL 3.3.1 laisse choisir la bibliothèque OpenGL
  qu'il charge (option `org.lwjgl.opengl.libname`, relevée dans sa classe
  `Configuration`). Lui donner une bibliothèque native du moteur qui implémente OpenGL sur
  Vulkan — le modèle de Zink — capte tous les appels, bruts compris. À poser avant la
  création de la fenêtre : argument JVM du profil de lancement, ou accroche précoce.
- Sans OpenGL réel, c'est le moteur qui présente l'image finale. Forge expose deux réglages
  de son écran de chargement précoce dans `config/fml.toml`, `earlyWindowControl` et
  `earlyWindowProvider` (relevé) : de quoi le couper, voire le remplacer (à vérifier).
- Coût : une implémentation d'OpenGL sur Vulkan, au moins du sous-ensemble qu'emploient
  les mods — un chantier de l'ampleur de Zink, ou Zink lui-même intégré (bibliothèque
  native tierce, contrat 5).

### 13.9 Relancer le jeu : la piste Cleanroom (proposée par Killian)

Relevé le 2026-10-03 dans les sources de CleanroomRelauncher (GitHub,
`CleanroomMC/CleanroomRelauncher`), le mod qui fait passer une instance Forge 1.12.2 sur
Cleanroom :

- c'est un coremod (`RelauncherEntryPoint implements IFMLLoadingPlugin`), et son fichier
  commence par `!` pour que Forge le charge en premier (README) ;
- `CleanroomRelauncher` construit une nouvelle ligne de commande Java (`-cp` vers les
  bibliothèques de Cleanroom, mises en cache dans `~/.cleanroom/`), la lance par
  `ProcessBuilder` avec `inheritIO()`, attend sa fin (`waitFor()`), puis termine le
  processus d'origine avec le même code de sortie (`ExitVMBypass.exit`) ;
- le launcher n'est pas modifié : il lance une JVM, qui lance le vrai jeu et lui passe sa
  console.

Transposé à RUSTFORGE-X en 1.20.1 :

- relancer la même commande, augmentée de `-Dorg.lwjgl.opengl.libname` vers la
  bibliothèque du moteur et du réglage de l'écran de chargement précoce. Dans la JVM
  relancée, le moteur est là avant toute fenêtre : « être premier » est résolu par
  construction ;
- sans téléchargement, puisque INV-15 interdit le réseau : tout voyage dans le jar, comme la
  bibliothèque native aujourd'hui (C-03) ;
- à vérifier : reconstituer fidèlement la ligne de commande de ModLauncher (Java 17 donne
  `ProcessHandle.current().info()`), marquer la JVM relancée pour ne pas boucler, réserver
  la relance au client, mesurer le coût du double démarrage, essayer chaque launcher ;
- point d'accroche le plus précoce en 1.20.1 : les services ModLauncher trouvés dans
  `mods/` (`ModDirTransformerDiscoverer`, présent dans `fmlloader-1.20.1-47.4.23`).
  Minecraft reprend la fenêtre de l'écran précoce : `Window` appelle
  `ImmediateWindowHandler.setupMinecraftWindow` (relevé dans le jeu patché).

### 13.10 Règles de la v1.0 à revoir dans l'ADR de M11

Écrites avant ce choix de route, elles protègent la 1.0 et ne se lèvent pas en silence :

- PARTIE 0 : la « réécriture de Minecraft » est hors portée de la V1.0 ;
- PARTIE 2 : RUSTFORGE-X n'est pas « un remplacement de Minecraft » ;
- R-310 : l'instrumentation ne modifie pas la sémantique observable — une redirection
  d'appel n'est admissible que si l'émulation est fidèle ;
- C-53 et R-830 (§2).

---

## Sources

- [minecraft.net — Another step towards Vibrant Visuals for Java Edition](https://www.minecraft.net/en-us/article/another-step-towards-vibrant-visuals-for-java-edition)
- [Neowin — Minecraft Java Edition is upgrading to Vulkan, but it's not great news for mods](https://www.neowin.net/news/minecraft-java-edition-is-upgrading-to-vulkan-but-its-not-great-news-for-mods/)
- [VulkanMod — GitHub](https://github.com/xCollateral/VulkanMod)
- [CleanroomRelauncher — GitHub](https://github.com/CleanroomMC/CleanroomRelauncher)
  (README et sources lus le 2026-10-03)
- [Cleanroom — installation du client](https://cleanroommc.com/wiki/end-user-guide/installation/install-client)
