# C-06 — Event Observer : fiche de conception

Statut : **plan, rien n'est implémenté.** Prépare l'étape F du `tasks/todo.md`.

Cahier des charges : PARTIE 5.6. Exigences : R-330 (l'ordre observable ne change jamais),
R-331 (annulation respectée, exécution anticipée en shadow), R-332 (un événement à
résultat est `ORDER_SENSITIVE` par défaut). Défaillance : FM-14. Tests : T-150 à T-154.
Emplacement : `src/main/java/dev/rustforgex/forge/`.

---

## 0. Ce que l'API de Forge permet réellement

Relevé sur `eventbus-6.2.33.jar` du serveur de production, en lisant le pool de
constantes et les tables de membres des classes — pas une documentation.

| Membre | Visibilité | Ce qu'il donne |
|---|---|---|
| `IEventListener.invoke(Event)` | interface publique | un auditeur est une interface à une méthode : l'envelopper est trivial |
| `IEventListener.listenerName()` | méthode par défaut | nom lisible, déjà prévu pour les enveloppes |
| `NamedEventListener` | classe de Forge | **Forge enveloppe déjà ses propres auditeurs** — le procédé est attendu, pas subversif |
| `Event.getListenerList()` | publique | la liste des auditeurs d'un événement, depuis une instance |
| `EventListenerHelper.getListenerList(Class)` | publique statique | la même, depuis la classe d'événement |
| `ListenerList.getListeners(int busID)` | publique | le tableau **ordonné** des auditeurs, toutes priorités confondues |
| `ListenerList.register(int, EventPriority, IEventListener)` | publique | ajoute **à la fin** de la priorité donnée |
| `ListenerList.unregister(int, IEventListener)` | publique | retire |
| `ASMEventHandler.getPriority()` | publique | la priorité d'un auditeur issu de `@SubscribeEvent` |
| `EventBus.post(Event, IEventBusInvokeDispatcher)` | publique | dispatch avec un intercepteur fourni par l'appelant |
| `EventBus.busID` | **privé** | identifiant du bus, nécessaire à `getListeners` |
| `EventListenerHelper.listeners` | **privé statique** | le cache `Class -> ListenerList`, seule voie vers l'ensemble des types d'événements |
| `ListenerList$ListenerListInst.priorities` | **privé** | les listes par priorité, seule voie vers la priorité d'un auditeur lambda |

### La voie élégante, et pourquoi elle est fermée

`IEventBusInvokeDispatcher` est une interface à une méthode, `invoke(IEventListener,
Event)`, et `EventBus.post` en accepte une. Un dispatcher à nous mesurerait chaque
appel **sans toucher à un seul auditeur** : aucune identité modifiée, donc FM-14
disparaîtrait, et l'ordre serait préservé par construction puisque nous ne toucherions
pas aux tableaux.

Mais c'est l'**appelant** de `post` qui fournit le dispatcher, et l'appelant est
Minecraft. Y accéder demanderait un mixin sur `EventBus.post(Event)` — impossible :
les classes d'`eventbus` vivent dans la couche d'amorçage et ne passent pas par le
chargeur transformant. Les traces de production le confirment, elles ne portent aucun
marqueur de transformation :

```text
at net.minecraftforge.eventbus.EventBus.post(EventBus.java:312) ~[eventbus-6.2.33.jar%2352!/:?] {}
at net.minecraft.server.MinecraftServer.m_130011_(…) {re:mixin,pl:accesstransformer:B,…}
```

Ni Mixin ni notre `ITransformer` n'atteignent ces classes. La voie est notée ici pour
qu'on ne la redécouvre pas.

### La question qui décide de tout le reste

`eventbus` est un **module nommé** dans la couche d'amorçage : il embarque un
`module-info`. Un `setAccessible(true)` sur un champ privé d'un paquet non ouvert lève
`InaccessibleObjectException`.

> **Première chose à vérifier, avant d'écrire une ligne :** le paquet
> `net.minecraftforge.eventbus` est-il ouvert à notre module ? Une ligne suffit, au
> `LOAD_COMPLETE` :
>
> ```java
> LOGGER.info("eventbus ouvert : {}", IEventListener.class.getModule()
>         .isOpen("net.minecraftforge.eventbus", RfxRuntime.class.getModule()));
> ```

La réponse commande le plan : si c'est `false`, tout ce qui dépend de la réflexion sur
les internes tombe, et il reste l'étape 1 ci-dessous, qui n'en a pas besoin.

> **Réponse, relevée sur le serveur de production le 2026-09-06 :**
>
> ```text
> Module net.minecraftforge.eventbus : paquet interne ouvert à rustforgex — true.
> ```
>
> Les étapes 2 et 3 sont donc techniquement possibles. Cela ne les rend pas souhaitables
> pour autant : lire des champs privés d'une bibliothèque tierce reste une dépendance à
> son implémentation, que rien n'oblige à rester stable.
Forge fournit `net.minecraftforge.unsafe` pour contourner ce genre d'encapsulation :
**ne pas l'employer.** Contourner l'encapsulation d'une bibliothèque tierce pour
observer ses internes n'est pas une dépendance qu'un mod d'optimisation doit prendre.

---

## 1. Ce que C-04 mesure déjà, et ce qui manque vraiment

Un point souvent négligé : **les corps des handlers sont déjà sondés.** Un
`@SubscribeEvent` est une méthode d'une classe de mod, et C-04 instrumente les classes
des mods depuis le 2026-09-05. Le temps passé dans un handler est donc déjà attribué à
son `WorkId`.

Ce que les sondes ne voient pas, et que C-06 doit apporter :

1. **La structure.** Quel handler écoute quel événement, sur quel bus, à quelle
   priorité, dans quel ordre. Une sonde voit une méthode, pas sa place dans une chaîne.
2. **L'annulation et le résultat.** `cancel()` et `setResult()` s'exercent sur l'objet
   événement, pas sur la méthode : aucune sonde ne les distingue d'un appel ordinaire.
3. **Le coût du dispatch lui-même** — la boucle de `post`, les filtres de généricité —
   qui n'appartient à aucun handler.

Cette répartition dicte le découpage en étapes : la structure ne demande aucune
enveloppe, les deux autres si.

---

## 2. Plan en trois étapes, de la moins risquée à la plus

### Étape 1 — Observer sans rien modifier (aucun risque R-330) — **faite**

*Vérifiée sur le serveur de production le 2026-09-06 : 38 000 événements distribués sur
12 types, 576 chronométrés, 0 chronométrage abandonné. Coût non distinguable du bruit sur
une paire courte — p50 +4,6 %, p95 +0,7 %, p99 −1,1 %, contre +5,8 / +2,7 / +3,1 sans
l'observateur.*

*Douze types seulement : c'est la limite d'un serveur au repos, pas celle de
l'instrument. Un serveur joué en poste des centaines.*

Aucune enveloppe, aucune écriture dans les listes. On construit le modèle
`HandlerInstance` de la PARTIE 5.6 à partir de ce qui se lit.

Voie **publique uniquement** : enregistrer nos propres auditeurs sur les bus, pour
`Event.class`, en `HIGHEST` et en `LOWEST`. `IEventBus.addListener` est publique, et un
auditeur du type de base reçoit tout ce qui passe sur le bus.

Cela donne, sans réflexion :

- la liste des types d'événements réellement postés, et leur fréquence ;
- la durée totale du dispatch par type — entre notre `HIGHEST` et notre `LOWEST` ;
- l'état final : `isCanceled()`, `getResult()` — observés à `LOWEST`, donc après la
  chaîne, ce qui répond déjà à R-332 pour le classement `ORDER_SENSITIVE` ;
- via `event.getListenerList()`, publique, l'objet dont on tirera l'ordre à l'étape 2.

Ce que cela ne donne pas : *quel* handler a annulé, et la priorité d'un auditeur lambda.

**Attention à ce que coûte cet auditeur.** Il s'exécute à chaque événement du serveur,
soit des milliers de fois par tick sur un modpack chargé. Il ne doit rien faire d'autre
qu'un horodatage et un incrément dans une table préallouée, indexée par type d'événement
— jamais de `HashMap` construite dans le chemin, jamais d'allocation (INV-14, R-320). Son
propre coût entre dans le budget de C-05 et **doit être visible dans la ligne de base de
la PARTIE 12.4** : si l'installer fait bouger la mesure de coût, c'est qu'il est trop
cher.

Livrables : `HandlerInstance` sans `priority` ni `owner` fiables, ordre par type,
compteurs `rfx.events.dispatch_ns{event_type}` et `rfx.events.cancels`.

### Étape 2 — Reconstituer l'ordre et la propriété

Demande la réflexion, donc dépend de la question de la section 0.

- `EventBus.busID` (lecture seule d'un `int` privé) → permet
  `ListenerList.getListeners(busID)`, publique, qui rend le tableau **déjà ordonné**.
- Pour chaque auditeur du tableau : `ASMEventHandler.getPriority()` quand il s'agit d'un
  `@SubscribeEvent` ; sinon la priorité se lit dans `ListenerListInst.priorities`, un
  champ privé.
- `owner` : le modid se déduit du chargeur de classe de l'auditeur, par le même
  `ModOwnerResolver` que C-04 utilise déjà — **aucune condition sur un nom de mod**
  (INV-12), on lit une table paquet → modid construite depuis `ModList`.

Rien n'est écrit ici : c'est de la lecture. R-330 reste intact par construction.

### Étape 3 — Envelopper, et seulement alors

C'est la seule étape qui touche à la sémantique. Elle n'a de sens que pour attribuer
l'annulation et la mutation à un handler précis.

**Le piège de l'ordre.** `ListenerList.register` ajoute **à la fin** de la priorité.
Remplacer un auditeur par `unregister` puis `register` le déplacerait en queue de sa
priorité : violation directe de R-330. La seule manière correcte est de traiter une
priorité entière d'un coup — désenregistrer tous ses auditeurs, puis réenregistrer les
enveloppes **dans l'ordre d'origine**. T-150 doit vérifier exactement cela, et pas
seulement que « ça marche ».

**FM-14.** L'enveloppe délègue `equals`, `hashCode` et `listenerName`. Elle ne peut rien
faire pour un mod qui teste le type concret ou transtype : dans ce cas, désenveloppement
du handler et mise en quarantaine. Une garde compte les désenveloppements et, au-delà
d'un seuil, rend la totalité des enveloppes — c'est le `Fallback` de la PARTIE 5.6.

**R-331.** Une enveloppe ne décide jamais d'exécuter ou non : elle délègue. Le filtre
d'annulation est déjà porté par le prédicat que Forge attache à l'auditeur. Toute
exécution anticipée devra être une shadow execution sans effet — hors périmètre de M1,
et à ne pas esquisser tant que C-23 n'existe pas.

---

## 3. Ce qui est hors périmètre, et doit le rester

- **L'exécution parallèle des handlers.** Les points 4 et 5 de l'algorithme de la
  PARTIE 5.6 dépendent des read/write sets de C-11 et de C-23, qui n'existent pas. C-06
  se contente de **marquer** `ORDER_SENSITIVE` ; personne ne consomme encore ce
  marquage, et il ne faut pas écrire le consommateur « en attendant ».
- **La shadow execution** de R-331 : même raison.
- Le classement `ORDER_SENSITIVE` lui-même reste conservateur : un événement dont on
  n'a rien observé est `ORDER_SENSITIVE` (UNKNOWN = CONSERVATIVE).

---

## 4. Tests, et ce qu'ils doivent vraiment vérifier

| Test | Ce qu'il ne suffit pas de vérifier | Ce qu'il doit vérifier |
|---|---|---|
| T-150 | que les 1000 événements passent | que la **séquence** des handlers, priorités mixtes comprises, est identique à celle relevée avant enveloppement |
| T-151 | qu'un `cancel()` est détecté | qu'aucun handler sans `receiveCanceled` ne s'exécute après |
| T-152 | que `getResult()` est lisible | que la valeur finale est **identique** à celle d'une exécution sans enveloppe |
| T-153 | qu'un désenveloppement est possible | qu'un mod qui transtype l'auditeur déclenche le désenveloppement **et** que le handler continue de fonctionner |
| T-154 | que le proxy est rapide | que son surcoût est **mesuré**, sous 30 ns, par le harnais C-36 — jamais estimé |

T-154 impose une mesure : ce sera un micro-benchmark JMH côté Java, qui n'existe pas
encore. Voir la dette T-131, de même nature.

**Acceptance de la PARTIE 5.6** : comparaison de traces entre une exécution instrumentée
et une exécution de référence. Cela suppose un mode d'enregistrement de trace d'ordre
d'événements, à prévoir dès l'étape 1 — c'est bien plus simple à écrire avant les
enveloppes qu'après.

---

## 5. Ordre de travail proposé

1. Vérifier l'ouverture du module `net.minecraftforge.eventbus`. Une ligne, un
   lancement. Elle décide de l'étendue des étapes 2 et 3.
2. Étape 1, avec sa trace d'ordre, et mesurer son coût par la ligne de base de C-05
   avant d'aller plus loin.
3. Étape 2 si le module est ouvert ; sinon, écrire l'ADR qui acte la limite et s'en
   tenir à ce que l'API publique permet.
4. Étape 3 en dernier, et seulement si l'attribution de l'annulation à un handler se
   révèle nécessaire à une décision réelle. Envelopper pour envelopper ajouterait un
   risque à un modpack sans rien rapporter.

Date : 2026-09-05
Auteur : agent d'implémentation
