#!/usr/bin/env bash
#
# C-36 niveau B : campagne de macro-benchmarks (PARTIE 21.2 et 21.3).
#
# Lance le serveur dédié plusieurs fois, dans deux configurations, puis agrège les
# exécutions en un résultat unique.
#
#   ./benchmarks/run-macro.sh [runs] [ticks_mesures] [ticks_echauffement]
#
# Sans argument, la campagne suit la méthodologie normative : 5 exécutions,
# 12 000 ticks mesurés, 3 600 ticks d'échauffement — soit plus de deux heures.
# Des valeurs plus courtes servent à vérifier le harnais lui-même ; l'agrégateur
# marque alors le résultat non conforme, et il ne peut pas être publié.
#
# Chaque exécution est un processus distinct (PARTIE 21.3, point 5) : c'est ce qui
# rend les répétitions indépendantes, JIT et caches compris.

set -u

RUNS="${1:-5}"
TICKS="${2:-12000}"
WARMUP="${3:-3600}"

cd "$(dirname "$0")/.." || exit 1

RUNS_DIR="benchmarks/runs"
rm -rf "$RUNS_DIR"
mkdir -p "$RUNS_DIR"

echo "Campagne : $RUNS exécutions × ($WARMUP échauffement + $TICKS mesurés) × 2 configurations"
echo "Durée approximative par exécution : $(( (WARMUP + TICKS) / 20 )) s de jeu, hors démarrage"
echo

for run in $(seq 1 "$RUNS"); do
    for config in off on; do
        if [ "$config" = "off" ]; then
            label="a-rfx-off"
            extra="-PbenchRfxOff"
        else
            label="b-rfx-on"
            extra=""
        fi

        echo "── exécution $run/$RUNS, configuration $label"
        # shellcheck disable=SC2086
        ./gradlew runServer -q \
            -PbenchTicks="$TICKS" \
            -PbenchWarmup="$WARMUP" \
            -PbenchLabel="$label" \
            -PbenchRun="$run" \
            -PbenchOut="$RUNS_DIR/$label-$run.json" \
            $extra > "$RUNS_DIR/$label-$run.log" 2>&1

        if [ ! -f "$RUNS_DIR/$label-$run.json" ]; then
            echo "   ÉCHEC : aucune exécution écrite. Journal : $RUNS_DIR/$label-$run.log"
            echo "   La campagne continue ; l'exécution manquante se verra dans run_indices."
        else
            echo "   ok"
        fi
    done
done

echo
echo "Agrégation…"
cargo run -p rfx-bench --release --quiet -- macro
