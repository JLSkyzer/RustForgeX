#!/usr/bin/env bash
#
# C-36 niveau B, en production (PARTIE 21.2 et 21.3).
#
# Contrairement à run-macro.sh, qui pilote le serveur de développement de ForgeGradle,
# ce script s'adresse à un serveur Forge installé, avec ses mods, tel qu'un joueur en
# exploite un. C'est le seul environnement où les mixins des mods de production
# s'appliquent réellement : leurs refmaps ne se résolvent pas contre les mappings de
# développement, et un modpack qui refuse de démarrer sous ForgeGradle démarre ici.
#
#   ./benchmarks/run-macro-prod.sh <racine_du_serveur> [runs] [ticks] [echauffement]
#
# La racine doit contenir un serveur Forge installé, ses mods, et les deux JAR de
# RUSTFORGE-X. Le script ne l'installe pas et n'y ajoute rien.

set -u

SERVER="${1:?usage: run-macro-prod.sh <racine_du_serveur> [runs] [ticks] [echauffement]}"
RUNS="${2:-5}"
TICKS="${3:-12000}"
WARMUP="${4:-3600}"

PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
RUNS_DIR="$PROJECT/benchmarks/runs"

if [ ! -d "$SERVER/libraries/net/minecraftforge/forge" ]; then
    echo "Aucun serveur Forge sous $SERVER"
    exit 1
fi

FORGE_VERSION="$(ls "$SERVER/libraries/net/minecraftforge/forge" | head -1)"
ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/win_args.txt"
[ -f "$SERVER/$ARGS_FILE" ] || ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/unix_args.txt"

rm -rf "$RUNS_DIR"
mkdir -p "$RUNS_DIR"

echo "Serveur   : $SERVER (Forge $FORGE_VERSION)"
echo "Mods      : $(ls "$SERVER/mods"/*.jar 2>/dev/null | wc -l)"
echo "Campagne  : $RUNS exécutions × ($WARMUP échauffement + $TICKS mesurés) × 2 configurations"
echo

for run in $(seq 1 "$RUNS"); do
    for config in off on; do
        if [ "$config" = "off" ]; then
            label="a-rfx-off"
            rfx="-Drustforgex.general.enabled=false"
        else
            label="b-rfx-on"
            rfx="-Drustforgex.general.enabled=true"
        fi

        echo "── exécution $run/$RUNS, configuration $label"
        (
            cd "$SERVER" || exit 1
            # shellcheck disable=SC2086
            java \
                -Drustforgex.bench.ticks="$TICKS" \
                -Drustforgex.bench.warmup="$WARMUP" \
                -Drustforgex.bench.label="$label" \
                -Drustforgex.bench.run="$run" \
                -Drustforgex.bench.out="$RUNS_DIR/$label-$run.json" \
                $rfx \
                "@user_jvm_args.txt" "@$ARGS_FILE" nogui
        ) > "$RUNS_DIR/$label-$run.log" 2>&1

        if [ -f "$RUNS_DIR/$label-$run.json" ]; then
            echo "   ok"
        else
            echo "   ÉCHEC : aucune exécution écrite. Journal : $RUNS_DIR/$label-$run.log"
        fi
    done
done

echo
echo "Agrégation…"
cd "$PROJECT" && cargo run -p rfx-bench --release --quiet -- macro
