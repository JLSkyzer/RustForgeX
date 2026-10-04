#!/usr/bin/env bash
#
# C-36 : tests de gameplay de la PARTIE 20.3.4 — égalité d'état et absence d'erreur.
#
#   ./benchmarks/run-gameplay.sh <racine_du_serveur> [scenarios]
#
# scenarios : liste séparée par des virgules, parmi g03, g01 et g08 (défaut : g03,g01).
# Variable JAVA : binaire java à employer (défaut : java du PATH).
# Variable DETAILS=true : diagnostic des entités de bloc, type et champ par champ —
# toujours actif pour g08, dont le jugement en dépend.
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
# DEUX GARDE-FOUS, posés après une campagne gâchée le 2026-10-03.
#   - Tout le script tient dans la fonction `main`, suivie d'un `exit`. Bash lit un
#     script au fil de l'exécution : modifier le fichier pendant qu'une campagne tourne
#     lui faisait rejouer une partie de la boucle, et lancer un second serveur en même
#     temps qu'un autre. Une fonction est lue en entier avant d'être exécutée.
#   - Un verrou interdit deux campagnes simultanées sur la même sortie.
#
# Le script supprime, avant chaque exécution, le monde de test qu'il va créer — et
# seulement lui : un dossier `gp-<scénario>-<étiquette>` dans la racine du serveur. Le
# monde principal et la configuration du serveur ne sont jamais touchés. Il vide aussi,
# au départ, les résultats de la campagne précédente dans `benchmarks/runs/gameplay/`,
# pour qu'un fichier ancien ne soit jamais lu comme un nouveau.

set -u

main() {
    local server="${1:?usage: run-gameplay.sh <racine_du_serveur> [g03,g01]}"
    local scenarios="${2:-g03,g01}"
    local java="${JAVA:-java}"

    cd "$(dirname "$0")/.." || return 1
    local proj
    proj="$(pwd)"
    local out="$proj/benchmarks/runs/gameplay"
    mkdir -p "$out"

    if ! mkdir "$out/.lock" 2>/dev/null; then
        echo "Une campagne tourne déjà (verrou $out/.lock). Abandon."
        echo "Si aucune ne tourne, le verrou est un reste d'arrêt brutal : le supprimer."
        return 3
    fi
    # Chemin développé MAINTENANT : le piège s'exécute après la sortie de main, quand la
    # variable locale n'existe plus — et sous `set -u`, il échouait sans retirer le verrou.
    # shellcheck disable=SC2064
    trap "rmdir '$out/.lock' 2>/dev/null" EXIT

    rm -f "$out"/*.json "$out"/*.log

    local fv af
    fv="$(ls "$server/libraries/net/minecraftforge/forge" | head -1)"
    af="libraries/net/minecraftforge/forge/$fv/win_args.txt"
    [ -f "$server/$af" ] || af="libraries/net/minecraftforge/forge/$fv/unix_args.txt"

    cp "$proj/build/libs/rustforgex-1.0-SNAPSHOT.jar" \
       "$proj/build/libs/rustforgex-launch-1.0-SNAPSHOT.jar" "$server/mods/" || return 1

    # run <scenario> <étiquette> <true|false> [load]
    #
    # Sans quatrième argument, l'exécution part d'un monde neuf. Avec « load » (G-08,
    # seconde phase), elle REPREND le monde de la phase précédente — qu'il ne faut alors
    # surtout pas supprimer — et écrit son empreinte sous `<scenario>-<étiquette>-load`.
    run() {
        local scenario=$1 label=$2 enabled=$3 phase=${4:-save}
        local world="gp-$scenario-$label"
        local name="$scenario-$label"
        # G-08 juge les entités de bloc par type et par champ : leur détail est requis.
        local details="${DETAILS:-false}"
        [ "$scenario" = g08 ] && details=true
        if [ "$phase" = load ]; then
            name="$name-load"
        else
            rm -rf "${server:?}/$world"
        fi
        echo "$(date +%H:%M:%S) == $name : RUSTFORGE-X $enabled"
        ( cd "$server" && "$java" \
            -Drustforgex.bench.scenario="$scenario" \
            -Drustforgex.bench.g08.phase="$phase" \
            -Drustforgex.bench.digest.details="$details" \
            -Drustforgex.bench.digest.out="$out/$name.json" \
            -Drustforgex.bench.label="$name" \
            -Drustforgex.general.enabled="$enabled" \
            "@user_jvm_args.txt" "@$af" nogui --world "$world" ) > "$out/$name.log" 2>&1
        if [ -f "$out/$name.json" ]; then
            echo "$(date +%H:%M:%S)    fini"
        else
            echo "$(date +%H:%M:%S)    ÉCHEC : aucune empreinte. Journal : $out/$name.log"
        fi
    }

    # Une configuration d'un scénario ; pour G-08, la sauvegarde puis le rechargement.
    play() {
        run "$1" "$2" "$3"
        if [ "$1" = g08 ]; then
            run "$1" "$2" "$3" load
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

    local list scenario label log status=0 verdict
    IFS=',' read -r -a list <<< "$scenarios"
    for scenario in "${list[@]}"; do
        play "$scenario" ref1 false
        play "$scenario" rfx true
        play "$scenario" ref2 false
    done

    for scenario in "${list[@]}"; do
        echo
        echo "════ $scenario ════"
        cargo run -q -p rfx-bench -- digest \
            "$out/$scenario-ref1.json" "$out/$scenario-ref2.json" "$out/$scenario-rfx.json"
        verdict=$?
        [ "$verdict" -gt "$status" ] && status=$verdict

        if [ "$scenario" = g08 ]; then
            # Le critère propre à G-08 : l'aller-retour du candidat ne doit rien changer
            # que celui des références ne change déjà. L'égalité stricte échoue aussi
            # pour elles — des mods réécrivent leurs entités de bloc au rechargement —,
            # d'où un jugement par classe de changement (ADR-032).
            echo "── allers-retours"
            cargo run -q -p rfx-bench -- roundtrips \
                "$out/g08-ref1.json" "$out/g08-ref1-load.json" \
                "$out/g08-ref2.json" "$out/g08-ref2-load.json" \
                "$out/g08-rfx.json" "$out/g08-rfx-load.json"
            verdict=$?
            [ "$verdict" -gt "$status" ] && status=$verdict
        fi

        echo "── erreurs"
        for label in ref1 rfx ref2; do
            log="$out/$scenario-$label.log"
            printf "   %-5s ERROR %4s   VerifyError/ClassFormat %s   RUSTFORGE-X %s   chien de garde %s\n" \
                "$label" \
                "$(grep -c '/ERROR\]' "$log")" \
                "$(grep -cE 'VerifyError|ClassFormatError|IncompatibleClassChange|NoSuchMethodError' "$log")" \
                "$(grep '/ERROR\]' "$log" | grep -ci rustforge)" \
                "$(grep -c 'ServerHangWatchdog\|single server tick took' "$log")"
        done
        if diff <(errors "$out/$scenario-ref1.log") <(errors "$out/$scenario-rfx.log") > /dev/null; then
            echo "   messages d'erreur identiques entre référence et candidat"
        else
            echo "   MESSAGES D'ERREUR DIFFÉRENTS :"
            diff <(errors "$out/$scenario-ref1.log") <(errors "$out/$scenario-rfx.log") | head -20
            status=1
        fi
    done
    echo "CAMPAGNE TERMINÉE, statut $status"
    return "$status"
}

main "$@"
exit $?
