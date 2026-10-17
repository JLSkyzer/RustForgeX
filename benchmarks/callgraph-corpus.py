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


def wanted(entry):
    return (entry.endswith(".class") and not entry.startswith("META-INF/")
            and not entry.endswith("module-info.class")
            and not entry.endswith("package-info.class"))


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
                for entry in jar.namelist():
                    if wanted(entry):
                        counts["archives"] += write(out, seen, entry[:-6], jar.read(entry))
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
