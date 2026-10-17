#!/usr/bin/env bash
# Acceptance de C-07 (ADR-053) : 200 methodes de reference, taux de resolution et
# zero faux resolu, verifie par la JVM.
#
#   ./benchmarks/run-callgraph-acceptance.sh <java> [corpus]
#
# <java>   : java du JDK 17 dont viennent les classes du corpus (jimage)
# [corpus] : fichier ecrit par callgraph-corpus.py, target/callgraph-corpus.bin par defaut
#
# Prerequis : un `./gradlew runGameTestServer` (classpath ecrit par ForgeGradle) et le
# corpus (voir benchmarks/README.md). Code de sortie : 0 accepte, 1 refuse, 2 invalide.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
java="${1:?usage : run-callgraph-acceptance.sh <java> [corpus]}"
corpus="${2:-$root/target/callgraph-corpus.bin}"
classpath="$root/build/classpath/runGameTestServer_minecraftClasspath.txt"
sites="$root/target/callgraph-acceptance-sites.tsv"

[[ -f "$classpath" ]] || { echo "classpath absent : lancer ./gradlew runGameTestServer" >&2; exit 2; }
[[ -f "$corpus" ]] || { echo "corpus absent : $corpus (voir benchmarks/README.md)" >&2; exit 2; }

asm="$(tr -d '\r' < "$classpath" | grep -E '[/\\]asm-[0-9][0-9.]*\.jar$' | head -1)"
[[ -n "$asm" ]] || { echo "ASM introuvable dans le classpath" >&2; exit 2; }

commit="$(git -C "$root" rev-parse HEAD)"
dirty=false
[[ -z "$(git -C "$root" status --porcelain)" ]] || dirty=true
stamp="$(date +%s)"
out="$root/benchmarks/results/callgraph-acceptance-$stamp-${commit:0:7}.json"

(cd "$root" && cargo run -q -p rfx-bench --release -- callgraph-acceptance "$corpus" "$sites")
set +e
"$java" -Xmx4g --add-modules ALL-SYSTEM -cp "$asm" "$root/benchmarks/CallGraphOracle.java" \
    "$classpath" "$sites" "$sites.report.json" "$commit" "$dirty" "$out" 2>&1 \
    | grep -v -e 'incubator modules' -e '^1 warning$'
status=${PIPESTATUS[0]}
set -e
echo "$out"
exit "$status"
