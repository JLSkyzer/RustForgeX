# CHANGELOG

Toutes les évolutions notables de RUSTFORGE-X.
Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).
Versionnement : cahier des charges, PARTIE 25.1.

## [Non publié]

### Jalon M0 — Bootstrap

Premier jalon : le mod se charge, charge sa bibliothèque native, mesure la machine et
sait rendre compte de son état. **Il ne transforme rien** — c'est exactement ce qui
est attendu de ce jalon.

#### Ajouté

- **C-27 Rust Runtime Core** — état global du runtime, instance unique par processus,
  handles opaques validés par génération, comptage des panics et arrêt d'urgence
  au-delà de 10 panics en 60 secondes.
- **C-45 Hardware Probe** — cœurs physiques et logiques, cache L3, nœuds NUMA,
  capacités SIMD et mémoire totale, mesurés par appels système directs sous Windows et
  par `/proc` et `/sys` sous Linux. Coût d'un aller-retour FFI et débit de transfert
  Java vers natif mesurés depuis Java.
- **C-03 Native Loader** — extraction du binaire dans un chemin versionné par son
  condensé SHA-256, vérifié avant tout chargement, avec repli sur le répertoire
  temporaire si la racine de jeu refuse l'écriture.
- **C-02 Bootstrap** — séquence `INIT → PROBE → LOAD_NATIVE → HANDSHAKE → CONFIGURE →
  READY | DEGRADED | DISABLED`, dont aucune exception ne remonte jusqu'à Forge.
- **C-37 Configuration** — `rustforgex.toml` commenté, créé au premier lancement,
  valeurs validées contre un schéma borné, clés inconnues conservées et signalées,
  surcharges par propriétés système.
- **C-01 Forge Integration** — démarrage et arrêt du runtime, enregistrement des
  commandes, refus d'activation hors de la plage de versions de Forge supportée,
  gardes qui désactivent une accroche après cinq échecs.
- **C-38 `/rfx status`** — état du runtime, mode, classe matérielle mesurée et
  maturité des composants.
- **C-40 Release System (partiel)** — construction du binaire natif intégrée à Gradle,
  empreintes SHA-256, manifeste des plateformes incluses, archives reproductibles,
  vérification d'artefact et intégration continue.
- **IF-01** — ABI C versionnée, chaque point d'entrée protégé contre les panics.

#### Documentation

- `ARCHITECTURE.md`, `CONFIGURATION.md`, `SECURITY.md`, `BUILDING.md`,
  `docs/AGENT.md`, et la copie versionnée du cahier des charges dans `docs/spec/`.
- ADR-013 (layout du dépôt), ADR-014 (numérotation des ADR), ADR-015 (points d'entrée
  ABI du jalon).

#### Notes

- Aucun hook de tick n'est installé : le jeu tourne exactement comme sans le mod.
- Aucun chiffre de performance n'est publié : le harnais de benchmark (C-36) arrive au
  jalon M1, et aucune valeur ne sera annoncée avant d'avoir été mesurée.
- Les sections de configuration `profiler`, `scheduler`, `decision` et suivantes
  n'existent pas encore : elles apparaîtront avec les composants qu'elles pilotent.
