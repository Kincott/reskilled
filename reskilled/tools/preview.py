#!/usr/bin/env python3
"""Renders the scenes the preview tests write (see native/api/src/client/preview.rs) to PNG pictures.

    SKILLS_PREVIEW_DIR=/some/dir cargo test -p mod-reskilled preview
    python3 -I tools/preview.py /some/dir <vanilla assets>/assets/minecraft

The second argument is a Minecraft resource folder (the one holding `font/` and `textures/`), used for the game's
font, item pictures and GUI sprites; nothing from it is copied anywhere but into the pictures. Needs Pillow.
"""
import json
import pathlib
import sys

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
    for folder in ("item", "block"):
        file = assets / "textures" / folder / f"{path}.png"
        if file.exists():
            picture = Image.open(file).convert("RGBA")
            picture = picture.crop((0, 0, picture.width, picture.width)).resize((size, size), Image.NEAREST)
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


def nine_slice(canvas, assets, name, rect):
    """A GUI atlas sprite (`gui/sprites/widget/button`) over `rect` as the game draws it: corners at their size,
    edges and middle tiled. A name the game's atlas would not find stops the preview, as the game would draw
    nothing."""
    file = assets / "textures" / f"{name}.png"
    if not name.startswith("gui/sprites/") or not file.exists():
        raise SystemExit(f"no sprite {name!r} in the game's GUI atlas ({file})")
    picture = Image.open(file).convert("RGBA")
    meta = file.with_name(file.name + ".mcmeta")
    border = 0
    if meta.exists():
        scaling = json.loads(meta.read_text()).get("gui", {}).get("scaling", {})
        border = scaling.get("border", 0) if scaling.get("type") == "nine_slice" else 0
    x, y, w, h = (round(v) for v in rect)
    if not border:
        canvas.alpha_composite(picture.resize((w, h), Image.NEAREST), (x, y))
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


def render(scene, font, assets):
    canvas = world(scene["size"])
    for paint in scene["paints"]:
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
        elif kind == "tooltip":
            tooltip(canvas, font, paint["spans"], paint["at"])
    return canvas.resize((canvas.width * SCALE, canvas.height * SCALE), Image.NEAREST)


def main():
    scenes, assets = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    font = Font(assets)
    for file in sorted(scenes.glob("*.json")):
        render(json.loads(file.read_text()), font, assets).save(file.with_suffix(".png"))
        print(file.with_suffix(".png"))


if __name__ == "__main__":
    main()
