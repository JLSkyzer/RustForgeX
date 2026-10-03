#!/usr/bin/env bash
#
# C-36 : tests de gameplay de la PARTIE 20.3.4 — égalité d'état et absence d'erreur.
#
#   ./benchmarks/run-gameplay.sh <racine_du_serveur> [scenarios]
#
# scenarios : liste séparée par des virgules, parmi g03 et g01 (défaut : g03,g01).
# Variable JAVA : binaire java à employer (défaut : java du PATH).
#
# Chaque scénario est joué trois fois sur un monde neuf, à la même graine : deux
# références sans RUSTFORGE-X, un candidat avec. `rfx-bench digest` juge ensuite si le
# candidat est plus souvent l'intrus que le hasard ne le permet (ADR-032).
#
# PROTOCOLE CONTRE LE BIAIS DE POSITION (ADR-032, mise à jour).
# Dans les deux premières campagnes, la première exécution était l'intruse bien plus
# souvent que les autres : 281 puis 340 chunks, contre ~200. Placer le candidat en
# dernier l'avantageait. Deux corrections, l'une ne suffisant pas à garantir l'autre :
#   1. une génération d'ÉCHAUFFEMENT ouvre la campagne, et son résultat est écarté ;
#   2. le candidat est joué AU MILIEU — référence, candidat, référence.
# La sortie de `rfx-bench digest` donne, pour chaque composante, combien de fois chaque
# position a été l'intruse : c'est là qu'on vérifie que le biais a disparu.
#
# Le script supprime, avant chaque exécution, le monde de test qu'il va créer — et
# seulement lui : un dossier `gp-<scénario>-<étiquette>` dans la racine du serveur. Le
# monde principal et la configuration du serveur ne sont jamais touchés.

set -u

SERVER="${1:?usage: run-gameplay.sh <racine_du_serveur> [g03,g01]}"
SCENARIOS="${2:-g03,g01}"
JAVA="${JAVA:-java}"

cd "$(dirname "$0")/.." || exit 1
PROJ="$(pwd)"
OUT="$PROJ/benchmarks/runs/gameplay"
mkdir -p "$OUT"

FV="$(ls "$SERVER/libraries/net/minecraftforge/forge" | head -1)"
AF="libraries/net/minecraftforge/forge/$FV/win_args.txt"
[ -f "$SERVER/$AF" ] || AF="libraries/net/minecraftforge/forge/$FV/unix_args.txt"

cp "$PROJ/build/libs/rustforgex-1.0-SNAPSHOT.jar" \
   "$PROJ/build/libs/rustforgex-launch-1.0-SNAPSHOT.jar" "$SERVER/mods/" || exit 1

# run <scenario> <étiquette> <true|false>
run() {
    local scenario=$1 label=$2 enabled=$3
    local world="gp-$scenario-$label"
    echo "$(date +%H:%M:%S) == $scenario / $label : RUSTFORGE-X $enabled"
    rm -rf "${SERVER:?}/$world"
    rm -f "$OUT/$scenario-$label.json"
    ( cd "$SERVER" && "$JAVA" \
        -Drustforgex.bench.scenario="$scenario" \
        -Drustforgex.bench.digest.out="$OUT/$scenario-$label.json" \
        -Drustforgex.bench.label="$scenario-$label" \
        -Drustforgex.general.enabled="$enabled" \
        "@user_jvm_args.txt" "@$AF" nogui --world "$world" ) > "$OUT/$scenario-$label.log" 2>&1
    if [ -f "$OUT/$scenario-$label.json" ]; then
        echo "$(date +%H:%M:%S)    fini"
    else
        echo "$(date +%H:%M:%S)    ÉCHEC : aucune empreinte. Journal : $OUT/$scenario-$label.log"
    fi
}

# Messages d'erreur, nombres normalisés : ce qui reste doit être identique d'une
# exécution à l'autre si RUSTFORGE-X n'ajoute aucune erreur.
errors() {
    grep '/ERROR\]' "$1" | sed -E 's/^\[[0-9:]+\] \[[^]]*\/ERROR\] //; s/[0-9]+/N/g' | sort -u
}

# L'échauffement est toujours une génération G-03, la plus courte : son rôle est de
# réchauffer les caches de la machine (jars des mods, système de fichiers), pas de
# jouer le scénario. Son résultat est écarté.
echo "Échauffement : une exécution de g03, écartée."
run g03 warmup false

IFS=',' read -r -a LIST <<< "$SCENARIOS"
for scenario in "${LIST[@]}"; do
    run "$scenario" ref1 false
    run "$scenario" rfx true
    run "$scenario" ref2 false
done

status=0
for scenario in "${LIST[@]}"; do
    echo
    echo "════ $scenario ════"
    cargo run -q -p rfx-bench -- digest \
        "$OUT/$scenario-ref1.json" "$OUT/$scenario-ref2.json" "$OUT/$scenario-rfx.json"
    verdict=$?
    [ "$verdict" -gt "$status" ] && status=$verdict

    echo "── erreurs"
    for label in ref1 rfx ref2; do
        log="$OUT/$scenario-$label.log"
        printf "   %-5s ERROR %4s   VerifyError/ClassFormat %s   RUSTFORGE-X %s\n" "$label" \
            "$(grep -c '/ERROR\]' "$log")" \
            "$(grep -cE 'VerifyError|ClassFormatError|IncompatibleClassChange|NoSuchMethodError' "$log")" \
            "$(grep '/ERROR\]' "$log" | grep -ci rustforge)"
    done
    if diff <(errors "$OUT/$scenario-ref1.log") <(errors "$OUT/$scenario-rfx.log") > /dev/null; then
        echo "   messages d'erreur identiques entre référence et candidat"
    else
        echo "   MESSAGES D'ERREUR DIFFÉRENTS :"
        diff <(errors "$OUT/$scenario-ref1.log") <(errors "$OUT/$scenario-rfx.log") | head -20
        status=1
    fi
done
exit "$status"
