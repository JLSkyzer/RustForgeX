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

### 2026-09-04 | `gradle.properties` est lu en ISO-8859-1 | Garder ce fichier en ASCII pur

Les fichiers `.properties` sont chargés par Java en ISO-8859-1 : un caractère accentué
écrit en UTF-8 y devient du mojibake, et `mod_description` est réinjecté tel quel dans
`mods.toml` puis affiché dans la liste des mods en jeu.

**Règle** : `gradle.properties` reste en ASCII. Si un texte accentué doit être affiché,
il passe par une ressource UTF-8, pas par une propriété Gradle.
