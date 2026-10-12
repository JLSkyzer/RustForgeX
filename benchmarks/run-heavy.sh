#!/usr/bin/env bash
#
# T-140 : coût du profilage sous le profil `heavy` de la PARTIE 22 (8 000 entités,
# 2 500 chunks), et stabilité du classement entre deux exécutions identiques.
#
#   ./benchmarks/run-heavy.sh <racine_du_serveur> [runs] [ticks] [echauffement] [synth]
#
# Chaque exécution est un processus distinct, sur un monde régénéré à l'identique
# (`gp-t140`), RUSTFORGE-X actif. Le coût est relevé par la ligne de base du profileur
# (PARTIE 12.4) avec la cadence d'ADR-025 : 10 cycles de 2 048 + 512 ticks. Le profil
# coupe la sauvegarde automatique (`save-off`) : avec des milliers d'entités, elle a
# déjà produit un tick de 120 s et fait arrêter le serveur par son chien de garde.
# Le jugement du classement : `rfx-bench ranking` sur les deux premières exécutions.
#
# Le cinquième argument, `synth`, décrit un serveur sans modpack, chargé du seul mod
# synthétique de la PARTIE 22 (ADR-036) : 150 unités engendrées pour les 150 mods
# synthétiques de `heavy`. Sur le modpack de 273 mods, `heavy` bloque le serveur avec
# ou sans RUSTFORGE-X : un tick de collisions dépasse les 120 s du chien de garde.

set -u

SERVER="${1:?usage: run-heavy.sh <racine_du_serveur> [runs] [ticks] [echauffement] [synth]}"
RUNS="${2:-2}"
TICKS="${3:-26000}"
WARMUP="${4:-900}"
MODE="${5:-modpack}"

PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
# RUNS_SUBDIR : sous-dossier des résultats ; EXTRA_PROPS : propriétés JVM en plus, pour
# les expériences d'attribution (ADR-036), par exemple
# -Drustforgex.instrumentation.stack_sampling=false.
RUNS_DIR="$PROJECT/benchmarks/runs/${RUNS_SUBDIR:-t140}"
read -r -a EXTRA <<< "${EXTRA_PROPS:-}"
WORLD="gp-t140"

if [ ! -d "$SERVER/libraries/net/minecraftforge/forge" ]; then
    echo "Aucun serveur Forge sous $SERVER"
    exit 1
fi
FORGE_VERSION="$(ls "$SERVER/libraries/net/minecraftforge/forge" | head -1)"
ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/win_args.txt"
[ -f "$SERVER/$ARGS_FILE" ] || ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/unix_args.txt"

JARS="rustforgex-1.0-SNAPSHOT.jar rustforgex-launch-1.0-SNAPSHOT.jar"
SYNTH_PROPS=()
if [ "$MODE" = "synth" ]; then
    # 150 unités engendrées, une par mod synthétique : le générateur en produit 120 par
    # défaut. Fils propres et non-déterminisme : R-870.
    (cd "$PROJECT" && ./gradlew synthJar -q -PrfxSynthClasses=150) || exit 1
    JARS="$JARS rustforgex-bench-synth-1.0-SNAPSHOT.jar"
    SYNTH_PROPS=(
        -Drfxbench.synth.units=150
        -Drfxbench.synth.workloads=150
        -Drfxbench.synth.iterations=2000
        -Drfxbench.synth.allocation=1024
        -Drfxbench.synth.handlers=50
        -Drfxbench.synth.threads=4
        -Drfxbench.synth.nondeterministic=true
    )
fi

for jar in $JARS; do
    if [ ! -f "$PROJECT/build/libs/$jar" ]; then
        echo "$jar absent : lancer d'abord ./gradlew build"
        exit 1
    fi
    cp "$PROJECT/build/libs/$jar" "$SERVER/mods/$jar"
done

rm -rf "$RUNS_DIR"
mkdir -p "$RUNS_DIR"

echo "Serveur  : $SERVER (Forge $FORGE_VERSION), $(ls "$SERVER/mods"/*.jar 2>/dev/null | wc -l) mods"
echo "Campagne : $RUNS exécutions × ($WARMUP échauffement + $TICKS mesurés), profil heavy ($MODE)"
echo

for run in $(seq 1 "$RUNS"); do
    label="heavy-$run"
    echo "── exécution $run/$RUNS ($(date +%H:%M))"
    # Monde régénéré à chaque exécution : la même graine, donc la même charge.
    rm -rf "${SERVER:?}/$WORLD"
    (
        cd "$SERVER" || exit 1
        java "${SYNTH_PROPS[@]}" "${EXTRA[@]}" \
            -Drustforgex.bench.ticks="$TICKS" \
            -Drustforgex.bench.warmup="$WARMUP" \
            -Drustforgex.bench.label="$label" \
            -Drustforgex.bench.run="$run" \
            -Drustforgex.bench.load=heavy \
            -Drustforgex.bench.out="$RUNS_DIR/$label.json" \
            -Drustforgex.general.enabled=true \
            -Drustforgex.profiler.baseline_pause_ticks=512 \
            -Drustforgex.profiler.baseline_period_ticks=2048 \
            -Drustforgex.profiler.baseline_cycles=10 \
            "@user_jvm_args.txt" "@$ARGS_FILE" nogui --world "$WORLD"
    ) > "$RUNS_DIR/$label.log" 2>&1
    if [ -f "$RUNS_DIR/$label.json" ]; then
        echo "   ok ($(date +%H:%M))"
    else
        echo "   ÉCHEC : aucune mesure écrite. Journal : $RUNS_DIR/$label.log"
    fi
done

if [ -f "$RUNS_DIR/heavy-1.json" ] && [ -f "$RUNS_DIR/heavy-2.json" ]; then
    echo
    echo "Classement…"
    cd "$PROJECT" && cargo run -p rfx-bench --release --quiet -- ranking \
        "$RUNS_DIR/heavy-1.json" "$RUNS_DIR/heavy-2.json"
fi
