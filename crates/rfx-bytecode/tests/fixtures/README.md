# Classes de test de C-07, C-11, C-12 et C-13 (ADR-050, ADR-055, ADR-057, ADR-058)

`src/fixtures/*.java` sont écrites pour ce projet ; `classes/` en est la compilation,
versionnée pour que les tests Rust n'aient pas besoin d'un JDK. Aucune classe tierce
(Minecraft, Forge, mod, JDK) n'est redistribuée ici.

Après toute modification d'une source, recompiler depuis ce répertoire, puis relancer
`cargo test -p rfx-bytecode -p rfx-analyzer` :

```bash
javac --release 17 -encoding UTF-8 -d classes src/fixtures/*.java
```

Compilé avec `javac 17.0.20.1` (Eclipse Adoptium). Le bytecode couvre ce que les tests
attendent — à vérifier avec `javap -c -p -cp classes fixtures.Calls` après une
recompilation :

- `invokestatic`, `invokespecial` (privé, constructeur, `super`), `invokevirtual`,
  `invokeinterface`, méthode par défaut ;
- `invokedynamic` de deux lambdas (`LambdaMetafactory`) et d'une concaténation
  (`StringConcatFactory`) ;
- `tableswitch`, `lookupswitch`, `iinc_w` (préfixe `wide`) ;
- réflexion (`Method.invoke`, `MethodHandle.invokeExact`), récursion directe et
  mutuelle, une chaîne de vingt appels et un éventail de 8 × 8 ;
- références de méthode virtuelles (`REF_invokeVirtual` : `Dog::sound`,
  `String::length`), et une implémentation unique qui hérite sa méthode d'une classe
  du JDK (`ThreadJob extends Thread`) ;
- une méthode par défaut connue, dans une classe qui implémente aussi une interface
  du JDK (`Item implements Named, Comparable`) ;
- les mêmes cas par une **classe** plutôt qu'une interface (ADR-051) : trois et cinq
  sous-classes (`Vehicle`, `Tool`), méthode par défaut héritée (`Speaker`), interface
  du JDK voisine (`Labelled`), méthode héritée de `Thread` (`Worker`) ;
- une méthode exécutée dans une classe intermédiaire (`Top`, `Middle`, `Leaf`).

`Flow` sert au décodeur d'instructions de C-11 (ADR-055) : `dup_x1`, `dup_x2`, `dup2`,
`dup2_x1`, `dup2_x2`, `pop2` sur des valeurs longues, `try`/`catch`/`finally`,
`synchronized`, `multianewarray`, `ldc` de chaque nature, conversions, et plus de 256
emplacements locaux (`iload_w`, `dstore_w`, `iinc_w`). Vérifier avec
`javap -c -p -cp classes fixtures.Flow`.

`Effects` sert à l'analyse de flux de données de C-11 : écritures sur le receveur, un
paramètre ou la jonction de deux, alias d'un tableau lu dans un champ, statiques,
verrou, `athrow` hors gestionnaire ou sous un gestionnaire d'un autre type, exception
attrapée puis écrite, variable locale modifiée dans un bloc protégé, plusieurs
`return`, boucle ; tableaux restés locaux ou échappés par transitivité.

`Interproc` sert à la phase interprocédurale : constructeur et fabrique locaux,
publication, écriture par paramètre, échappement par un appelé, `return this`,
récursion, paramètre rangé dans un objet rendu, lambda, concaténation, interface
ouverte, et un cas où le point fixe doit repasser sur un appelant.

`Threads` sert à C-12 (ADR-057) : `ThreadLocal`, méthode `synchronized`, `Object.wait`,
et le confinement dérivé — classes de premier niveau du même fichier, hors de tout nid :
champ public mutable (`Counter`, et `CounterChild` qui en hérite), champ final
(`Frozen`), champ privé écrit hors constructeur (`Mutating`), écrit dans une méthode
synchronisée (`Guarded`), écrit seulement par le constructeur (`CtorOnly`) ; et
`Threads$InnerPrivate`, le même cas que `CtorOnly` mais dans un nid.

`Chance` sert à C-13 (ADR-058) : une méthode par source de non-déterminisme
(`Random.nextInt`, `System.nanoTime`, `System.identityHashCode`, `HashMap.keySet`,
`File.exists`, `Lock.lock`, `Method.invoke`, appel d'une méthode `native`), et des
méthodes sans source : arithmétique entière en lecture seule, `dadd`, écriture d'un
statique.
