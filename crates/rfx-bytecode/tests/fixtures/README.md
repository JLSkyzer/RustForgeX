# Classes de test de C-07 (ADR-050)

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
  du JDK (`Item implements Named, Comparable`).
