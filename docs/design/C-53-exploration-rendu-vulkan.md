# C-53 et au-delà — rendu multi-thread et moteur Vulkan : note d'exploration

Statut : **exploration, rien n'est décidé ni implémenté.** Discussion entre Killian et
l'assistant du 2026-10-03, tenue depuis une session AXION ENGINE et consignée ici à la
demande de Killian, pour qu'une session dédiée à RUSTFORGE-X l'analyse en détail.

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

1. Étendre C-53 aux appels graphiques et à un moteur Vulkan, ou créer un composant
   dédié : ADR daté (contrat 1.4).
2. Choisir, ou non, une bibliothèque native tierce (`wgpu`, `ash`) : contrat 5.
3. Fixer la ou les versions de Minecraft visées, et la position vis-à-vis du Vulkan de
   Mojang.
4. Avant tout code : appliquer le protocole du §9 ; puis prototyper la capture de
   `ModelPart` vers un tampon d'enregistrement, mesurée par C-36.

---

## Sources

- [minecraft.net — Another step towards Vibrant Visuals for Java Edition](https://www.minecraft.net/en-us/article/another-step-towards-vibrant-visuals-for-java-edition)
- [Neowin — Minecraft Java Edition is upgrading to Vulkan, but it's not great news for mods](https://www.neowin.net/news/minecraft-java-edition-is-upgrading-to-vulkan-but-its-not-great-news-for-mods/)
- [VulkanMod — GitHub](https://github.com/xCollateral/VulkanMod)
