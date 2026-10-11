#!/usr/bin/env bash
#
# T-401 : coût de la télémétrie contre R-561 (« moins de 0,2 % du MSPT »). Méthode : ADR-034.
#
#   ./benchmarks/run-telemetry.sh <racine_du_serveur> [runs] [ticks_mesures] [ticks_echauffement]
#
# Chaque exécution est un processus distinct, sur un monde régénéré à l'identique, avec
# RUSTFORGE-X actif. La part Java est mesurée en place par `TelemetryCost` ; la part
# native par le micro-benchmark `tick_window/cycle`, lancé après les exécutions ; puis
# `rfx-bench telemetry` juge. Par défaut : 5 exécutions × (1 200 + 6 000) ticks, environ
# quarante minutes.

set -u

SERVER="${1:?usage: run-telemetry.sh <racine_du_serveur> [runs] [ticks] [echauffement]}"
RUNS="${2:-5}"
TICKS="${3:-6000}"
WARMUP="${4:-1200}"

PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
RUNS_DIR="$PROJECT/benchmarks/runs/t401"
WORLD="gp-t401"

if [ ! -d "$SERVER/libraries/net/minecraftforge/forge" ]; then
    echo "Aucun serveur Forge sous $SERVER"
    exit 1
fi
FORGE_VERSION="$(ls "$SERVER/libraries/net/minecraftforge/forge" | head -1)"
ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/win_args.txt"
[ -f "$SERVER/$ARGS_FILE" ] || ARGS_FILE="libraries/net/minecraftforge/forge/$FORGE_VERSION/unix_args.txt"

for jar in rustforgex-1.0-SNAPSHOT.jar rustforgex-launch-1.0-SNAPSHOT.jar; do
    if [ ! -f "$PROJECT/build/libs/$jar" ]; then
        echo "$jar absent : lancer d'abord ./gradlew build"
        exit 1
    fi
    cp "$PROJECT/build/libs/$jar" "$SERVER/mods/$jar"
done

rm -rf "$RUNS_DIR"
mkdir -p "$RUNS_DIR"

echo "Serveur  : $SERVER (Forge $FORGE_VERSION), $(ls "$SERVER/mods"/*.jar 2>/dev/null | wc -l) mods"
echo "Campagne : $RUNS exécutions × ($WARMUP échauffement + $TICKS mesurés), RUSTFORGE-X actif"
echo

for run in $(seq 1 "$RUNS"); do
    label="t401-$run"
    echo "── exécution $run/$RUNS"
    # Monde régénéré à chaque exécution : la même graine, donc la même charge.
    rm -rf "${SERVER:?}/$WORLD"
    (
        cd "$SERVER" || exit 1
        java \
            -Drustforgex.bench.telemetry.out="$RUNS_DIR/$label.json" \
            -Drustforgex.bench.telemetry.ticks="$TICKS" \
            -Drustforgex.bench.telemetry.warmup="$WARMUP" \
            -Drustforgex.bench.label="$label" \
            "@user_jvm_args.txt" "@$ARGS_FILE" nogui --world "$WORLD"
    ) > "$RUNS_DIR/$label.log" 2>&1
    if [ -f "$RUNS_DIR/$label.json" ]; then
        echo "   ok — $(grep -o '"java_ratio_pct": [0-9.]*' "$RUNS_DIR/$label.json")"
    else
        echo "   ÉCHEC : aucune mesure écrite. Journal : $RUNS_DIR/$label.log"
    fi
done

echo
echo "Part native…"
(cd "$PROJECT" && cargo bench -p rfx-bench --bench tick --quiet > "$RUNS_DIR/native.log" 2>&1)
echo
echo "Jugement…"
cd "$PROJECT" && cargo run -p rfx-bench --release --quiet -- telemetry
