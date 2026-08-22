#!/usr/bin/env python3
"""Convertisseur d'art ASCII en mesh « meshes-designer » (assets/*.json).

Le format cible est celui des fichiers mesh du catalogue d'armes
(`src/vaisseau.rs` / `src/cosmonaut.rs` le parsent avec serde) : une racine
`{"planes": [...]}` - chaque plan porte des `verts` (2D, y vers le haut,
convention de l'éditeur) et des `faces` triangulaires avec leur couleur RGBA.
Les autres champs (`app`, `zoom`, …) ne servent qu'à l'éditeur de gestion.

Le dessin (assets/asciiart-fr.txt) est une munition (missile) pointant vers
le haut dans l'art : il est tourné de 90° pour que le **nez pointe vers +x** -
la convention du catalogue (une munition est dessinée nez en avant,
`ammo_orientation_degrees = 0`).

Conversion : chaque cellule non-espace est un « pixel » ; le remplissage se
fait par **barres verticales** (une colonne × une plage de rangées contiguës
= 2 triangles), ce qui reproduit **exactement** le dessin - les vides (trou
entre les bandes du corps, espace entre ailerons et corps) restent vides.
Les barres adjacentes de même plage de rangées et de même couleur sont
fusionnées horizontalement pour limiter le nombre de faces. Chaque barre est
colorée selon la zone du dessin qu'elle couvre (nez / ailerons / bandes
denses / corps).

Usage :
    python3 tools/ascii-art-to-mesh.py [art.txt] [sortie.json]
"""

import json
import sys

# ─── Zones du dessin (rangées de l'art) et couleurs RGBA 0..1 ───────────────
# Le dessin alterne bandes denses (rangées pleines) et zones claires :
# - le NEZ (cône, en haut) - rouge (ogive) ;
# - les AILERONS (rangées 34-46 hors colonnes du corps) - orange ;
# - les BANDES denses du corps - acier sombre ;
# - le CORPS (et le cylindre central des ailerons) - acier clair.
NOSE_ROWS = range(6, 14)      # cône de nez
FIN_ROWS = range(34, 47)      # ailerons (rangées)
# colonnes du corps au milieu de la zone des ailerons (cylindre central)
FIN_BODY_COLS = range(50, 71)
DENSE_ROWS = {19, 23, 29, 30, 31, 32, 48, 49, 50, 51, 60, 61}

COLORS = {
    "nose": [0.90, 0.32, 0.25, 1.0],    # rouge (ogive)
    "fin": [0.95, 0.60, 0.15, 1.0],     # orange (ailerons)
    "dense": [0.30, 0.35, 0.45, 1.0],   # acier sombre (bandes)
    "body": [0.76, 0.81, 0.89, 1.0],    # acier clair (corps)
}


def row_class(y):
    if y in NOSE_ROWS:
        return "nose"
    if y in FIN_ROWS:
        return "fin"
    if y in DENSE_ROWS:
        return "dense"
    return "body"


def cell_class(x, y):
    """Classe d'une cellule : dans la zone des ailerons, les colonnes hors du
    cylindre central sont des ailerons, le cylindre reste du corps."""
    if y in FIN_ROWS and x not in FIN_BODY_COLS:
        return "fin"
    return row_class(y)


def load_grid(path):
    with open(path, encoding="utf-8") as f:
        return f.read().splitlines()


def build_mesh(lines, scale=1.0):
    """Trace l'art en barres verticales (fusionnées horizontalement) et
    renvoie (verts, faces, miny, nose_x)."""
    filled = set()
    for y, line in enumerate(lines):
        for x, ch in enumerate(line):
            if ch != " ":
                filled.add((x, y))
    if not filled:
        raise SystemExit("art vide : aucune cellule remplie")

    miny = min(y for _, y in filled)
    nose_x = (
        min(x for x, y in filled if y == miny)
        + max(x for x, y in filled if y == miny)
    ) / 2.0

    # rotation 90° : le haut de l'art (le nez) devient +x du mesh, y est
    # retourné (l'éditeur travaille y vers le haut)
    def rot(x, y):
        return (round(scale * (y - miny), 6), round(-scale * (x - nose_x), 6))

    verts = []
    vert_index = {}
    faces = []

    def quad(x1, x2, y1, y2, color):
        # barre verticale : colonnes x1..x2, cellules y1..y2 incluses - en
        # coordonnées continues, cela couvre les rangées y1..y2+1 (le bas
        # d'une cellule y est y+1 dans l'art, y vers le bas)
        def vert_p(x, y):
            p = rot(x, y)
            if p not in vert_index:
                vert_index[p] = len(verts)
                verts.append([p[0], p[1]])
            return vert_index[p]

        i0 = vert_p(x1, y1)
        i1 = vert_p(x2 + 1, y1)
        i2 = vert_p(x2 + 1, y2 + 1)
        i3 = vert_p(x1, y2 + 1)
        faces.append({"v": [i0, i1, i2], "color": color})
        faces.append({"v": [i0, i2, i3], "color": color})

    # par colonne : découpe les cellules remplies en plages contiguës de
    # même classe (une bande dense au milieu du corps découpe la barre pour
    # garder le zèbre du dessin)
    cols = sorted({x for x, _ in filled})
    runs = []  # (x, y1, y2, classe)
    for x in cols:
        cells = sorted(y for xx, y in filled if xx == x)
        run_start = cells[0]
        run_class = cell_class(x, run_start)
        prev = run_start
        for y in cells[1:]:
            if y == prev + 1 and cell_class(x, y) == run_class:
                prev = y  # la plage continue
            else:
                runs.append((x, run_start, prev, run_class))
                run_start = y
                prev = y
                run_class = cell_class(x, y)
        runs.append((x, run_start, prev, run_class))

    # fusionne horizontalement les barres de même plage de rangées et de même
    # classe (colonnes adjacentes) : une grande barre de 2 triangles au lieu
    # d'une par colonne - le compte de faces reste raisonnable pour une
    # munition du jeu
    runs.sort(key=lambda r: (r[1], r[2], r[3], r[0]))
    i = 0
    while i < len(runs):
        x, y1, y2, cls = runs[i]
        x2 = x
        j = i + 1
        while (
            j < len(runs)
            and runs[j][1] == y1
            and runs[j][2] == y2
            and runs[j][3] == cls
            and runs[j][0] == x2 + 1
        ):
            x2 = runs[j][0]
            j += 1
        quad(x, x2, y1, y2, COLORS[cls])
        i = j
    return verts, faces, miny, nose_x


def point_in_triangle(p, a, b, c):
    def cross(o, u, v):
        return (u[0] - o[0]) * (v[1] - o[1]) - (u[1] - o[1]) * (v[0] - o[0])

    d1 = cross(p, a, b)
    d2 = cross(p, b, c)
    d3 = cross(p, c, a)
    neg = (d1 < 0) or (d2 < 0) or (d3 < 0)
    pos = (d1 > 0) or (d2 > 0) or (d3 > 0)
    return not (neg and pos)


def render_back(verts, faces, miny, nose_x, lines):
    """Reprojection du mesh sur la grille de l'art (vérification par
    aller-retour) : chaque cellule d'art dont le **centre** est couvert par
    une face triangulaire est marquée - le rendu doit reproduire le dessin."""
    grid = [[" "] * 100 for _ in lines]
    for face in faces:
        pts = [(nose_x - verts[i][1], miny + verts[i][0]) for i in face["v"]]
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        for x in range(int(min(xs)), int(max(xs)) + 2):
            for y in range(int(min(ys)), int(max(ys)) + 2):
                if (
                    0 <= y < len(grid)
                    and 0 <= x < len(grid[y])
                    and point_in_triangle((x + 0.5, y + 0.5), *pts)
                ):
                    grid[y][x] = "#"
    return grid


def main():
    art_path = sys.argv[1] if len(sys.argv) > 1 else "assets/asciiart-fr.txt"
    out_path = sys.argv[2] if len(sys.argv) > 2 else "assets/missileWeapon.json"
    lines = load_grid(art_path)
    verts, faces, miny, nose_x = build_mesh(lines)
    mesh = {
        "app": "meshes-designer",
        "name": out_path,
        "zoom": 2,
        "cx": 0,
        "cy": 0,
        "grid": False,
        "gridStep": 1,
        "active": 0,
        "planes": [{"verts": verts, "faces": faces}],
    }
    with open(out_path, "w") as f:
        json.dump(mesh, f, separators=(",", ":"))
        f.write("\n")
    print(f"{out_path} : {len(verts)} sommets, {len(faces)} faces")

    # aller-retour : compare la reprojection à l'art original (contrôle)
    grid = render_back(verts, faces, miny, nose_x, lines)
    filled = set()
    for y, line in enumerate(lines):
        for x, ch in enumerate(line):
            if ch != " ":
                filled.add((x, y))
    missed = sum(1 for (x, y) in filled if grid[y][x] != "#")
    extra = sum(
        1
        for y in range(len(grid))
        for x in range(len(grid[y]))
        if grid[y][x] == "#" and (x, y) not in filled
    )
    print(f"aller-retour : {len(filled)} cellules, {missed} manquées, {extra} en trop")
    print("--- reprojection (#' = cellule couverte) ---")
    for row in grid:
        print("".join(row).rstrip())


if __name__ == "__main__":
    main()
