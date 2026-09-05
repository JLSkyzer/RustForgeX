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

# Le JAR du transformateur est retiré de `mods` pour la configuration B : sans lui,
# aucune classe n'est énumérée ni décodée. C'est la seule façon de mesurer le coût
# complet du système, transformation comprise — `general.enabled=false` n'éteint que
# le runtime, pas la chaîne de transformation.
LAUNCH_JAR_NAME="rustforgex-launch-1.0-SNAPSHOT.jar"
HOLD_DIR="$SERVER/rustforgex-bench-hold"

if [ ! -d "$SERVER/libraries/net/minecraftforge/forge" ]; then
    echo "Aucun serveur Forge sous $SERVER"
    exit 1
fi

restore_launch_jar() {
    if [ -f "$HOLD_DIR/$LAUNCH_JAR_NAME" ]; then
        mv "$HOLD_DIR/$LAUNCH_JAR_NAME" "$SERVER/mods/$LAUNCH_JAR_NAME"
        echo "Transformateur remis dans mods/."
    fi
    rmdir "$HOLD_DIR" 2>/dev/null || true
}

# Une campagne interrompue ne doit pas laisser l'installation amputée d'un JAR.
trap restore_launch_jar EXIT INT TERM

if [ ! -f "$SERVER/mods/$LAUNCH_JAR_NAME" ] && [ ! -f "$HOLD_DIR/$LAUNCH_JAR_NAME" ]; then
    echo "Le transformateur $LAUNCH_JAR_NAME est absent de $SERVER/mods"
    exit 1
fi
mkdir -p "$HOLD_DIR"
restore_launch_jar

FORGE_VERSION="$(ls "$SERVER/libraries/net/minecraftforge/forge" | head -1)"
ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/win_args.txt"
[ -f "$SERVER/$ARGS_FILE" ] || ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/unix_args.txt"

rm -rf "$RUNS_DIR"
mkdir -p "$RUNS_DIR"

echo "Serveur   : $SERVER (Forge $FORGE_VERSION)"
echo "Mods      : $(ls "$SERVER/mods"/*.jar 2>/dev/null | wc -l)"
echo "Campagne  : $RUNS exécutions × ($WARMUP échauffement + $TICKS mesurés) × 2 configurations"
echo "            B = modpack seul · C = modpack + RUSTFORGE-X (PARTIE 21.2)"
echo

for run in $(seq 1 "$RUNS"); do
    for config in off on; do
        if [ "$config" = "off" ]; then
            # PARTIE 21.2, configuration B : le modpack sans RUSTFORGE-X. Le JAR du mod
            # reste en place — il porte l'enregistreur, présent des deux côtés, donc
            # sans effet sur la différence — mais le transformateur s'en va.
            label="b-mods-seuls"
            rfx="-Drustforgex.general.enabled=false"
            mkdir -p "$HOLD_DIR"
            mv "$SERVER/mods/$LAUNCH_JAR_NAME" "$HOLD_DIR/$LAUNCH_JAR_NAME"
        else
            # PARTIE 21.2, configuration C : le même modpack, RUSTFORGE-X observant.
            label="c-rfx-actif"
            rfx="-Drustforgex.general.enabled=true"
            restore_launch_jar
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

        restore_launch_jar

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
