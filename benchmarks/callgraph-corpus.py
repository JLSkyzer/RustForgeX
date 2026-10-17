"""Assemble le corpus de classes du banc du graphe d'appel (C-07, ADR-052).

Usage :
    python benchmarks/callgraph-corpus.py <classpath.txt> <jdk-extrait> <sortie>

- <classpath.txt> : liste des archives du jeu de développement, une par ligne, telle
  que ForgeGradle l'écrit (build/classpath/runGameTestServer_minecraftClasspath.txt) ;
- <jdk-extrait> : classes du JDK, extraites par `jimage extract --dir <dossier>` ;
- <sortie> : fichier du corpus, à placer sous target/ — jamais dans le dépôt : il
  contient des classes de Minecraft, de Forge et du JDK, qu'on ne redistribue pas.

Format : `RFXCORP1`, puis pour chaque classe `[u16 longueur du nom][nom UTF-8]`
`[u32 longueur][octets]`, en petit-boutiste. Une classe homonyme plus loin est sautée,
comme la masquerait le chargeur : archives du jeu d'abord, dans l'ordre de la liste,
puis le JDK.
"""

import os
import struct
import sys
import zipfile

MAGIC = b"RFXCORP1"

# Version de Java du jeu : celle dont la JVM choisit les entrées des jars multi-release.
RUNTIME = 17


def wanted(entry):
    return (entry.endswith(".class") and not entry.startswith("META-INF/")
            and not entry.endswith("module-info.class")
            and not entry.endswith("package-info.class"))


def release_versions(jar):
    """Jar multi-release (JEP 238) : pour chaque classe, l'entrée que charge la JVM —
    `META-INF/versions/N/` de plus haut N <= RUNTIME, sinon la version de base."""
    try:
        manifest = jar.read("META-INF/MANIFEST.MF").decode("utf-8", "replace")
    except KeyError:
        return {}
    if not any(line.strip().lower() == "multi-release: true" for line in manifest.splitlines()):
        return {}
    best = {}
    for entry in jar.namelist():
        parts = entry.split("/", 3)
        if len(parts) == 4 and parts[0] == "META-INF" and parts[1] == "versions" \
                and parts[2].isdigit() and int(parts[2]) <= RUNTIME and parts[3].endswith(".class"):
            version = int(parts[2])
            if version > best.get(parts[3], (0, ""))[0]:
                best[parts[3]] = (version, entry)
    return {base: entry for base, (_, entry) in best.items()}


def write(out, seen, name, data):
    if name in seen:
        return 0
    seen.add(name)
    encoded = name.encode("utf-8")
    out.write(struct.pack("<H", len(encoded)))
    out.write(encoded)
    out.write(struct.pack("<I", len(data)))
    out.write(data)
    return 1


def main(classpath, jdk, output):
    archives = []
    with open(classpath, encoding="utf-8") as listing:
        for line in listing:
            path = line.strip()
            if path and path not in archives:
                archives.append(path)
    seen = set()
    counts = {"archives": 0, "jdk": 0}
    with open(output, "wb") as out:
        out.write(MAGIC)
        for archive in archives:
            if not archive.endswith(".jar") or not os.path.isfile(archive):
                continue
            with zipfile.ZipFile(archive) as jar:
                versions = release_versions(jar)
                for entry in jar.namelist():
                    if wanted(entry):
                        source = versions.get(entry, entry)
                        counts["archives"] += write(out, seen, entry[:-6], jar.read(source))
        # jimage extract : <dossier>/<module>/<paquet>/<Classe>.class
        for module in sorted(os.listdir(jdk)):
            root = os.path.join(jdk, module)
            for directory, _, files in os.walk(root):
                for file in sorted(files):
                    relative = os.path.relpath(os.path.join(directory, file), root)
                    entry = relative.replace(os.sep, "/")
                    if wanted(entry):
                        with open(os.path.join(directory, file), "rb") as source:
                            counts["jdk"] += write(out, seen, entry[:-6], source.read())
    print(f"{len(archives)} archives listées ; {counts['archives']} classes des archives, "
          f"{counts['jdk']} du JDK -> {output}")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        print(__doc__)
        sys.exit(2)
    main(sys.argv[1], sys.argv[2], sys.argv[3])
