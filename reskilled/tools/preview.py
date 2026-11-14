#!/usr/bin/env python3
"""Renders the scenes the preview tests write (see native/api/src/client/preview.rs) to PNG pictures.

    SKILLS_PREVIEW_DIR=/some/dir cargo test -p mod-reskilled preview
    python3 -I tools/preview.py /some/dir <vanilla assets>/assets/minecraft

The second argument is a Minecraft resource folder (the one holding `font/` and `textures/`), used for the game's
font, item pictures and GUI sprites; nothing from it is copied anywhere but into the pictures. Needs Pillow.
"""
import json
import math
import pathlib
import sys
import zlib

from PIL import Image, ImageDraw

SCALE = 3


class Font:
    """Vanilla's bitmap font: each glyph as its opaque pixels, and its advance."""

    def __init__(self, assets):
        self.glyphs = {}
        providers = json.loads((assets / "font/include/default.json").read_text())["providers"]
        for provider in providers:
            if provider.get("type") != "bitmap":
                continue
            image = Image.open(assets / "textures" / provider["file"].split(":")[1]).convert("RGBA")
            rows = provider["chars"]
            cell_w, cell_h = image.width // len(rows[0]), image.height // len(rows)
            ascent = provider.get("ascent", 7)
            for row, chars in enumerate(rows):
                for col, char in enumerate(chars):
                    if char in ("\u0000", " ") or char in self.glyphs:
                        continue
                    cell = image.crop((col * cell_w, row * cell_h, (col + 1) * cell_w, (row + 1) * cell_h))
                    pixels = [(x, y) for y in range(cell_h) for x in range(cell_w) if cell.getpixel((x, y))[3] > 0]
                    if pixels:
                        width = max(x for x, _ in pixels) + 1
                        self.glyphs[char] = (pixels, width + 1, 7 - ascent)

    def width(self, text):
        return sum(self.glyphs.get(char, ((), 4, 0))[1] for char in text)

    def draw(self, canvas, text, x, y, rgb, shadow=True):
        x, y = round(x), round(y)
        dark = tuple(channel // 4 for channel in rgb)
        for char in text:
            pixels, advance, drop = self.glyphs.get(char, ((), 4, 0))
            for dx, dy in pixels:
                if shadow:
                    put(canvas, x + dx + 1, y + dy + drop + 1, dark)
            for dx, dy in pixels:
                put(canvas, x + dx, y + dy + drop, rgb)
            x += advance


def nearest(picture, size):
    """`picture` scaled to `size` pixel for pixel. Through an affine transform: some Pillow builds fail a nearest
    `resize`."""
    w, h = size
    if w <= 0 or h <= 0:
        return Image.new(picture.mode, (max(w, 1), max(h, 1)), (0, 0, 0, 0))
    return picture.transform((w, h), Image.AFFINE, (picture.width / w, 0, 0, 0, picture.height / h, 0),
                             Image.NEAREST)


def put(canvas, x, y, rgb):
    if 0 <= x < canvas.width and 0 <= y < canvas.height:
        canvas.putpixel((x, y), (*rgb, 255))


def rgb_of(colour):
    return ((colour >> 16) & 255, (colour >> 8) & 255, colour & 255)


def fill(canvas, rect, argb):
    x, y, w, h = (round(v) for v in rect)
    if w <= 0 or h <= 0:
        return
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    layer.paste((*rgb_of(argb), argb >> 24), (x, y, x + w, y + h))
    canvas.alpha_composite(layer)


def item(canvas, assets, name, at, size):
    path = name.split(":")[1]
    size = round(size)
    # An animated item (a clock, a compass) by its first frame.
    for folder, frame in (("item", ""), ("block", ""), ("item", "_00")):
        file = assets / "textures" / folder / f"{path}{frame}.png"
        if file.exists():
            picture = Image.open(file).convert("RGBA")
            picture = nearest(picture.crop((0, 0, picture.width, picture.width)), (size, size))
            canvas.alpha_composite(picture, (round(at[0]), round(at[1])))
            return
    # An item drawn from a model (a shield): a plain plate stands in for it.
    fill(canvas, [at[0] + 2, at[1] + 1, size - 4, size - 2], 0xFF8A6B45)
    fill(canvas, [at[0] + 4, at[1] + 3, size - 8, size - 6], 0xFFB38B5D)


def tooltip(canvas, font, spans, at):
    widths = [sum(font.width(text) for text, _ in line) for line in spans]
    w, h = max(widths) + 6, len(spans) * 10 + 4
    x = min(max(round(at[0]) + 12, 2), canvas.width - w - 2)
    y = min(max(round(at[1]) - 12, 2), canvas.height - h - 2)
    fill(canvas, [x, y, w, h], 0xF0100010)
    for edge, colour in (([x, y, w, 1], 0xFF5000FF), ([x, y + h - 1, w, 1], 0xFF28007F),
                         ([x, y, 1, h], 0xFF3C00BF), ([x + w - 1, y, 1, h], 0xFF3C00BF)):
        fill(canvas, edge, colour)
    for n, line in enumerate(spans):
        left = x + 3
        for text, colour in line:
            font.draw(canvas, text, left, y + 3 + n * 10, rgb_of(colour) if colour is not None else (255, 255, 255))
            left += font.width(text)


def sprite_file(assets, name):
    """The picture behind a GUI atlas sprite as the game names it: `gui/sprites/...` for widgets and HUD pieces,
    `mob_effect/...` for effect icons. None when the game's atlas would not have it."""
    if name.startswith("gui/sprites/") or name.startswith("mob_effect/"):
        file = assets / "textures" / f"{name}.png"
        if file.exists():
            return file
    return None


def nine_slice(canvas, assets, name, rect):
    """A GUI atlas sprite (`gui/sprites/widget/button`) over `rect` as the game draws it: corners at their size,
    edges and middle tiled; a sprite with no nine-slice border (a heart, an effect icon) stretched over it. A name
    the game's atlas would not find stops the preview, as the game would draw nothing."""
    file = sprite_file(assets, name)
    if file is None:
        raise SystemExit(f"no sprite {name!r} in the game's GUI atlas")
    picture = Image.open(file).convert("RGBA")
    meta = file.with_name(file.name + ".mcmeta")
    border = 0
    if meta.exists():
        scaling = json.loads(meta.read_text()).get("gui", {}).get("scaling", {})
        border = scaling.get("border", 0) if scaling.get("type") == "nine_slice" else 0
    x, y, w, h = (round(v) for v in rect)
    if not border:
        canvas.alpha_composite(nearest(picture, (w, h)), (x, y))
        return
    if isinstance(border, int):
        border = {"left": border, "top": border, "right": border, "bottom": border}
    sw, sh = picture.width, picture.height
    left, top, right, bottom = (border.get(k, 0) for k in ("left", "top", "right", "bottom"))
    columns = [(0, left, 0, left), (left, sw - right, left, w - right), (sw - right, sw, w - right, w)]
    rows = [(0, top, 0, top), (top, sh - bottom, top, h - bottom), (sh - bottom, sh, h - bottom, h)]
    for sx0, sx1, dx0, dx1 in columns:
        for sy0, sy1, dy0, dy1 in rows:
            if sx1 <= sx0 or sy1 <= sy0 or dx1 <= dx0 or dy1 <= dy0:
                continue
            piece = picture.crop((sx0, sy0, sx1, sy1))
            for tx in range(dx0, dx1, max(sx1 - sx0, 1)):
                for ty in range(dy0, dy1, max(sy1 - sy0, 1)):
                    part = piece.crop((0, 0, min(piece.width, dx1 - tx), min(piece.height, dy1 - ty)))
                    canvas.alpha_composite(part, (x + tx, y + ty))


# Rough colours of the blocks the shipped scenes use, as (top, side); others get a colour from their name.
BLOCK_COLOURS = {
    "grass_block": ((96, 160, 64), (134, 96, 67)),
    "dirt": ((134, 96, 67), (120, 85, 58)),
    "farmland": ((82, 52, 30), (120, 85, 58)),
    "stone": ((125, 125, 125), (112, 112, 112)),
    "stone_bricks": ((122, 121, 122), (104, 103, 104)),
    "cobblestone": ((127, 127, 127), (100, 100, 100)),
    "mossy_cobblestone": ((100, 130, 90), (90, 105, 85)),
    "oak_planks": ((162, 130, 78), (140, 112, 66)),
    "oak_fence": ((162, 130, 78), (140, 112, 66)),
    "bookshelf": ((162, 130, 78), (110, 70, 50)),
    "glass": ((200, 230, 240), (170, 210, 225)),
    "tnt": ((200, 60, 50), (219, 68, 52)),
    "red_bed": ((160, 30, 30), (200, 200, 200)),
    "wheat": ((200, 180, 70), (170, 150, 50)),
    "enchanting_table": ((160, 30, 40), (40, 30, 50)),
    "brewing_stand": ((120, 110, 90), (90, 80, 70)),
    "cauldron": ((70, 70, 75), (60, 60, 65)),
    "campfire": ((220, 140, 40), (110, 80, 50)),
    "lantern": ((230, 180, 90), (80, 80, 90)),
    "torch": ((250, 210, 90), (140, 110, 60)),
}
# The box each kind of block fills, as (inset at the sides, height); a whole block is (0, 1).
BLOCK_SHAPES = {
    "farmland": (0.0, 15 / 16), "red_bed": (0.0, 9 / 16), "oak_fence": (6 / 16, 1.0),
    "torch": (7 / 16, 10 / 16), "lantern": (5 / 16, 9 / 16), "campfire": (0.0, 7 / 16),
    "brewing_stand": (6 / 16, 14 / 16), "enchanting_table": (0.0, 12 / 16),
}


def project(point, view, rect):
    """`point` of a scene as the game's scene view draws it in `rect` (x0, y0, x1, y1): GUI x, y, and a depth
    that grows towards the viewer (native_scene.rs, `scene_matrix`)."""
    x, y, z = (point[n] - view["center"][n] for n in range(3))
    yaw, tilt = math.radians(view["yaw"]), math.radians(-view["pitch"])
    x, z = x * math.cos(yaw) + z * math.sin(yaw), -x * math.sin(yaw) + z * math.cos(yaw)
    y, z = y * math.cos(tilt) - z * math.sin(tilt), y * math.sin(tilt) + z * math.cos(tilt)
    scale = view["scale"]
    return ((rect[0] + rect[2]) / 2 - scale * x, (rect[1] + rect[3]) / 2 - scale * y, -scale * z)


def block_look(state):
    """A block state string's colours (top, side) and box (inset, height); crops grow with their age."""
    name = state.split(":")[-1].split("[")[0]
    properties = dict(pair.split("=") for pair in state[:-1].split("[")[1].split(",")) if "[" in state else {}
    colours = BLOCK_COLOURS.get(name)
    if colours is None:
        hashed = zlib.crc32(name.encode())
        side = (64 + hashed % 160, 64 + (hashed >> 8) % 160, 64 + (hashed >> 16) % 160)
        colours = (tuple(min(255, c + 30) for c in side), side)
    inset, height = BLOCK_SHAPES.get(name, (0.0, 1.0))
    if "age" in properties:
        ripe = 3 if name in ("beetroots", "nether_wart") else 7
        age = int(properties["age"]) / ripe
        inset, height = 0.2, 0.15 + 0.75 * age
        colours = ((70 + int(130 * age), 170, 50), (60 + int(110 * age), 140, 40))
    return colours, inset, height


def scene(canvas, paint, states):
    """A guide scene as a flat sketch: each block a shaded box at its place, seen from the scene's view, the top
    lighter than the sides, cut to its box. Enough to judge the layout, not the game's models."""
    rect = paint["rect"]
    view = {key: paint[key] for key in ("center", "yaw", "pitch", "scale")}
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    faces = []
    for pos, state_id in paint["blocks"]:
        (top, side), inset, height = block_look(states.get(str(state_id), "unknown"))
        x0, x1 = pos[0] + inset, pos[0] + 1 - inset
        z0, z1 = pos[2] + inset, pos[2] + 1 - inset
        y0, y1 = pos[1], pos[1] + height
        corners = {
            "up": [(x0, y1, z0), (x1, y1, z0), (x1, y1, z1), (x0, y1, z1)],
            "north": [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0)],
            "south": [(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)],
            "west": [(x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0)],
            "east": [(x1, y0, z0), (x1, y0, z1), (x1, y1, z1), (x1, y1, z0)],
        }
        normals = {"up": (0, 1, 0), "north": (0, 0, -1), "south": (0, 0, 1), "west": (-1, 0, 0), "east": (1, 0, 0)}
        shades = {"up": 1.0, "north": 0.8, "south": 0.8, "west": 0.6, "east": 0.6}
        origin = project((0, 0, 0), view, rect)
        for face, points in corners.items():
            tip = project(normals[face], view, rect)
            if tip[2] - origin[2] <= 1e-6:
                continue
            projected = [project(p, view, rect) for p in points]
            depth = sum(p[2] for p in projected) / 4
            colour = top if face == "up" else side
            shaded = tuple(int(c * shades[face]) for c in colour)
            faces.append((depth, [(p[0], p[1]) for p in projected], shaded))
    for _, points, colour in sorted(faces, key=lambda f: f[0]):
        draw.polygon(points, fill=(*colour, 255), outline=(0, 0, 0, 90))
    box = tuple(round(v) for v in rect)
    canvas.alpha_composite(layer.crop(box), box[:2])


def world(size):
    """A stand-in for the game behind the screen: sky, hills, and where the hotbar and hearts sit."""
    w, h = size
    canvas = Image.new("RGBA", (w, h), (0, 0, 0, 255))
    for y in range(h):
        sky = y < h * 0.55
        shade = y / h
        colour = (120 + int(60 * shade), 167 + int(40 * shade), 255) if sky else (74, 130 - int(40 * shade), 48)
        canvas.paste((*colour, 255), (0, y, w, y + 1))
    fill(canvas, [(w - 182) // 2, h - 22, 182, 22], 0xC0202020)
    fill(canvas, [(w - 182) // 2, h - 39, 81, 9], 0xC0B02020)
    fill(canvas, [(w - 182) // 2 + 101, h - 39, 81, 9], 0xC0A07030)
    return canvas


def render(picture, font, assets):
    canvas = world(picture["size"])
    states = picture.get("states", {})
    for paint in picture["paints"]:
        kind = paint["kind"]
        if kind == "fill":
            fill(canvas, paint["rect"], paint["argb"])
        elif kind == "text":
            font.draw(canvas, paint["text"], *paint["at"], rgb_of(paint["rgb"]), paint["shadow"])
        elif kind == "item":
            item(canvas, assets, paint["item"], paint["at"], paint["size"])
        elif kind == "quad":
            layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
            argb = paint["argb"]
            ImageDraw.Draw(layer).polygon([tuple(p) for p in paint["points"]], fill=(*rgb_of(argb), argb >> 24))
            canvas.alpha_composite(layer)
        elif kind == "background":
            fill(canvas, paint["rect"], 0xC0101010)
        elif kind == "sprite":
            nine_slice(canvas, assets, paint["name"], paint["rect"])
        elif kind == "scene":
            scene(canvas, paint, states)
        elif kind == "tooltip":
            tooltip(canvas, font, paint["spans"], paint["at"])
    return nearest(canvas, (canvas.width * SCALE, canvas.height * SCALE))


def main():
    scenes, assets = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    font = Font(assets)
    for file in sorted(scenes.glob("*.json")):
        render(json.loads(file.read_text()), font, assets).save(file.with_suffix(".png"))
        print(file.with_suffix(".png"))


if __name__ == "__main__":
    main()
