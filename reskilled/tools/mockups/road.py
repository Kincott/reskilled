#!/usr/bin/env python3
"""Mockups of the "road" interface for Reskilled: Home, a skill's road, and Options, at the GUI size of 427 x 240,
drawn with the game's font, item icons and GUI sprites through the helpers of ../preview.py.

    python3 -I reskilled/tools/mockups/road.py <assets>/assets/minecraft <out dir>

These are pictures to agree a design on, not the mod's code: the screens the mod draws are previewed by the
preview tests (see ../preview.py)."""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
import preview  # noqa: E402
from PIL import Image  # noqa: E402

ASSETS = pathlib.Path(sys.argv[1])
OUT = pathlib.Path(sys.argv[2])
FONT = preview.Font(ASSETS)
W, H = 427, 240

BLACK = 0xFF07080B
BODY = 0xFF1A1D26
BODY_SPECK = 0xFF1E222C
CARD = 0xFF232733
CARD_HI = 0xFF2B3040
INSET = 0xFF0F1116
BRASS_LIGHT = 0xFFE6C36A
BRASS = 0xFFAD8436
BRASS_DARK = 0xFF5A4116
GOLD = 0xF2C14E
WHITE = 0xFFFFFF
GRAY = 0xA8ADB8
DIM = 0x6C7280
GREEN = 0x7FDB5A
RED = 0xFF6B5E
GO = (0xFF3F8F2A, 0xFF67C14A, 0xFF24561A)
PLAIN = (0xFF394050, 0xFF525B70, 0xFF222733)
OFF = (0xFF272B36, 0xFF323846, 0xFF1B1E26)

ACCENT = {"Mining": 0xFF8FA6B8, "Melee": 0xFFD9534A, "Defense": 0xFF6E8FD9, "Arcane": 0xFFB07CF2,
          "Gathering": 0xFF7FC75A, "Agility": 0xFF5AD1C7, "Building": 0xFFD98C4A, "Ranged": 0xFFD9C24A}
SKILLS = [("Agility", "minecraft:feather", 1, 0), ("Arcane", "minecraft:nether_star", 32, 0),
          ("Building", "minecraft:bricks", 1, 0), ("Defense", "minecraft:iron_chestplate", 3, 1),
          ("Gathering", "minecraft:copper_axe", 6, 3), ("Melee", "minecraft:iron_sword", 9, 3),
          ("Mining", "minecraft:iron_pickaxe", 12, 5), ("Ranged", "minecraft:bow", 1, 0)]
PERK_LEVELS = [4, 8, 12, 16, 24]
OWNED = {"Mining": [4], "Melee": [4], "Arcane": [4, 8, 12, 16, 24]}

FACE = ["hhhhhhhh", "hhhhhhhh", "hsssssshh"[:8], "ssssssss", "sWesseWs", "sssnnsss", "ssmmmmss", "ssssssss"]
FACE_PAL = {"h": 0xFF3B2A1A, "s": 0xFFC69473, "W": 0xFFFFFFFF, "e": 0xFF3D5AA8, "n": 0xFF9A6B52, "m": 0xFF7A4B3B}
GEAR = ["..O.O..", ".OOOOO.", "OOO.OOO", ".O...O.", "OOO.OOO", ".OOOOO.", "..O.O.."]
ARROW = ["....O....", "...OAO...", "..OAAaO..", ".OAAAaaO.", "OOOAAaOOO", "..OAAaO..", "..OAAaO..", "..OOOOO.."]


class Canvas:
    def __init__(self):
        self.im = preview.world([W, H])
        preview.fill(self.im, [0, 0, W, H], 0x99000000)

    def fill(self, x, y, w, h, argb):
        preview.fill(self.im, [x, y, w, h], argb)

    def edge(self, x, y, w, h, argb):
        self.fill(x, y, w, 1, argb)
        self.fill(x, y + h - 1, w, 1, argb)
        self.fill(x, y + 1, 1, h - 2, argb)
        self.fill(x + w - 1, y + 1, 1, h - 2, argb)

    def bevel(self, x, y, w, h, light, dark):
        self.fill(x, y, w, 1, light)
        self.fill(x, y + 1, 1, h - 1, light)
        self.fill(x + 1, y + h - 1, w - 1, 1, dark)
        self.fill(x + w - 1, y + 1, 1, h - 2, dark)

    def text(self, s, x, y, rgb, shadow=True):
        FONT.draw(self.im, s, round(x), round(y), preview.rgb_of(rgb), shadow)

    def right(self, s, right, y, rgb, shadow=True):
        self.text(s, right - FONT.width(s), y, rgb, shadow)

    def centred(self, s, x, y, w, rgb, shadow=True):
        self.text(s, x + (w - FONT.width(s)) / 2, y, rgb, shadow)

    def item(self, name, x, y, size=16, dim=False):
        preview.item(self.im, ASSETS, name, [x, y], size)
        if dim:
            self.fill(x, y, size, size, 0xA8101218)

    def sprite(self, name, x, y, w, h):
        preview.nine_slice(self.im, ASSETS, f"gui/sprites/{name}", [x, y, w, h])

    def tile(self, texture, x, y, w, h, darkness):
        tile = Image.open(ASSETS / "textures" / f"{texture}.png").convert("RGBA")
        tile = tile.crop((0, 0, tile.width, tile.width))
        for ty in range(0, h, tile.height):
            for tx in range(0, w, tile.width):
                part = tile.crop((0, 0, min(tile.width, w - tx), min(tile.height, h - ty)))
                self.im.alpha_composite(part, (x + tx, y + ty))
        self.fill(x, y, w, h, (int(255 * darkness) << 24) | 0x0B0C10)

    def art(self, rows, palette, x, y, scale=1):
        for r, line in enumerate(rows):
            for c, ch in enumerate(line):
                if ch in palette:
                    self.fill(x + c * scale, y + r * scale, scale, scale, palette[ch])

    def window(self, x, y, w, h):
        self.fill(x, y, w, h, BLACK)
        self.fill(x + 1, y + 1, w - 2, h - 2, BRASS)
        self.bevel(x + 1, y + 1, w - 2, h - 2, BRASS_LIGHT, BRASS_DARK)
        self.fill(x + 3, y + 3, w - 6, h - 6, BRASS_DARK)
        self.fill(x + 4, y + 4, w - 8, h - 8, BODY)
        for yy in range(y + 4, y + h - 4):
            for xx in range(x + 4, x + w - 4):
                if (xx * 7 + yy * 13) % 23 == 0:
                    self.im.putpixel((xx, yy), (*preview.rgb_of(BODY_SPECK), 255))
        for cx, cy in [(x + 1, y + 1), (x + w - 4, y + 1), (x + 1, y + h - 4), (x + w - 4, y + h - 4)]:
            self.fill(cx, cy, 3, 3, BRASS_LIGHT)
            self.fill(cx + 1, cy + 1, 1, 1, BRASS_DARK)

    def tab(self, x, y, selected, icon=None, pip=False, face=False, gear=False):
        h = 30 if selected else 26
        top = y if selected else y + 4
        self.fill(x, top, 28, h, BLACK)
        self.fill(x + 1, top + 1, 26, h - 1, BRASS if selected else BRASS_DARK)
        if selected:
            self.fill(x + 1, top + 1, 26, 1, BRASS_LIGHT)
            self.fill(x + 1, top + 2, 1, h - 2, BRASS_LIGHT)
        self.fill(x + 3, top + 3, 22, h - 3, BODY if selected else 0xFF14161D)
        iy = top + 6
        if icon:
            self.item(icon, x + 6, iy, 16)
        if face:
            self.art(FACE, FACE_PAL, x + 6, iy, 2)
        if gear:
            self.art(GEAR, {"O": 0xFFC9CED8}, x + 7, iy + 1, 2)
        if not selected:
            self.fill(x + 3, top + 3, 22, h - 3, 0x55090A0D)
        if pip:
            self.fill(x + 19, top + 4, 5, 5, BLACK)
            self.fill(x + 20, top + 5, 3, 3, 0xFFF2C14E)

    def button(self, x, y, w, h, label, look=GO, rgb=WHITE, arrow=False):
        face, light, dark = look
        self.fill(x, y, w, h, BLACK)
        self.fill(x + 1, y + 1, w - 2, h - 2, face)
        self.bevel(x + 1, y + 1, w - 2, h - 2, light, dark)
        tw = FONT.width(label) + (12 if arrow else 0)
        tx = round(x + (w - tw) / 2)
        ty = round(y + (h - 8) / 2) + 1
        if arrow:
            self.art(ARROW, {"O": 0xFF173A0F, "A": 0xFFF0FFE0, "a": 0xFFB4E39A}, tx, ty - 1)
            tx += 12
        self.text(label, tx, ty, rgb if look is not OFF else DIM)

    def chip(self, x, y, label, rgb, icon=None):
        w = FONT.width(label) + 8 + (11 if icon else 0)
        self.fill(x, y, w, 13, BLACK)
        self.fill(x + 1, y + 1, w - 2, 11, INSET)
        tx = x + 4
        if icon:
            self.item(icon, x + 2, y + 1.5, 10)
            tx += 11
        self.text(label, tx, y + 3, rgb)
        return w

    def padlock(self, x, y):
        self.fill(x, y + 3, 7, 5, BLACK)
        self.fill(x + 1, y + 4, 5, 3, 0xFFD9A62B)
        self.edge(x + 1, y, 5, 4, BLACK)
        self.fill(x + 2, y + 1, 3, 1, 0xFFB9BEC9)
        self.fill(x + 2, y + 2, 1, 2, 0xFFB9BEC9)
        self.fill(x + 4, y + 2, 1, 2, 0xFFB9BEC9)

    def slot(self, x, y, item=None, locked=False):
        self.sprite("container/slot", x, y, 18, 18)
        self.fill(x + 1, y + 1, 16, 16, 0xFF30343F)
        if item:
            self.item(item, x + 1, y + 1, 16, dim=locked)
        if locked:
            self.padlock(x + 10, y + 9)

    def node(self, x, y, item, state, capstone=False):
        """A perk in an advancement frame: `owned`, `open` (can be unlocked) or `locked`."""
        kind = "challenge" if capstone else "task"
        got = "obtained" if state == "owned" else "unobtained"
        if state == "open":
            self.edge(x - 1, y - 1, 28, 28, 0xFF67C14A)
        self.sprite(f"advancements/{kind}_frame_{got}", x, y, 26, 26)
        self.item(item, x + 5, y + 5, 16, dim=state == "locked")
        if state == "locked":
            self.padlock(x + 17, y + 16)
        if state == "owned":
            self.fill(x + 17, y + 17, 8, 8, BLACK)
            self.text("✔", x + 18, y + 17, GOLD, False)

    def save(self, name):
        preview.scaled(self.im, (W * preview.SCALE, H * preview.SCALE)).save(OUT / f"{name}.png")


WX, WY, WW, WH = 22, 30, 383, 204
TABS = ["Home"] + [s[0] for s in SKILLS]


def draw_tab(c, i, name, selected):
    x = WX + 6 + i * 31
    if name == "Home":
        c.tab(x, 2, selected, face=True)
    else:
        skill = next(s for s in SKILLS if s[0] == name)
        c.tab(x, 2, selected, icon=skill[1], pip=skill[3] > 0)


def chrome(c, selected):
    """The window and its tabs; `selected` is "Home", a skill's name, or "Options"."""
    for i, name in enumerate(TABS):
        if name != selected:
            draw_tab(c, i, name, False)
    if selected != "Options":
        c.tab(WX + WW - 34, 2, False, gear=True)
    c.window(WX, WY, WW, WH)
    if selected == "Options":
        c.tab(WX + WW - 34, 2, True, gear=True)
        c.fill(WX + WW - 31, WY, 22, 4, BODY)
    else:
        i = TABS.index(selected)
        draw_tab(c, i, selected, True)
        c.fill(WX + 9 + i * 31, WY, 22, 4, BODY)


def perk_state(skill, level, lvl):
    if lvl in OWNED.get(skill, []):
        return "owned"
    return "open" if level >= lvl else "locked"


def road():
    c = Canvas()
    chrome(c, "Mining")
    x0, x1 = WX + 8, WX + WW - 8
    hy = WY + 9
    c.sprite("advancements/goal_frame_obtained", x0, hy, 26, 26)
    c.item("minecraft:iron_pickaxe", x0 + 5, hy + 5)
    c.text("MINING", x0 + 32, hy + 3, GOLD)
    c.text("Level", x0 + 32, hy + 15, DIM, False)
    c.text("12", x0 + 32 + FONT.width("Level "), hy + 15, WHITE)
    c.text("of 32", x0 + 32 + FONT.width("Level 12 "), hy + 15, DIM, False)
    c.text("◆ 5 perk points", x0 + 110, hy + 9, GOLD)
    c.button(x1 - 96, hy + 3, 96, 20, "Level up · 12 XP", GO, arrow=True)
    xp_w = FONT.width("23 XP") + 19
    c.chip(x1 - 96 - 6 - xp_w, hy + 6, "23 XP", GREEN, "minecraft:experience_bottle")
    cy, ch = WY + 42, 96
    c.fill(x0, cy, x1 - x0, ch, BLACK)
    c.tile("gui/advancements/backgrounds/stone", x0 + 1, cy + 1, x1 - x0 - 2, ch - 2, 0.62)
    c.fill(x0 + 1, cy + 1, x1 - x0 - 2, 2, 0x66000000)
    c.fill(x0 + 1, cy + 1, 2, ch - 2, 0x66000000)
    level = 12
    step = (x1 - x0 - 30) / 31

    def at(lv):
        return round(x0 + 15 + (lv - 1) * step)

    ry = cy + 45
    c.fill(at(1) - 5, ry - 1, at(32) - at(1) + 10, 5, BLACK)
    c.fill(at(1) - 4, ry, at(32) - at(1) + 8, 3, 0xFF3A4152)
    c.fill(at(1) - 4, ry, at(level) - at(1) + 4, 3, 0xFFF2C14E)
    c.fill(at(1) - 4, ry, at(level) - at(1) + 4, 1, 0xFFFFE79A)
    for lv in range(1, 33):
        c.fill(at(lv), ry + 3, 1, 2, 0xFF8A6B2A if lv <= level else 0xFF2A2F3B)
    gear = [(2, "minecraft:golden_pickaxe"), (4, "minecraft:copper_pickaxe"), (8, "minecraft:iron_pickaxe"),
            (16, "minecraft:diamond_pickaxe"), (24, "minecraft:netherite_pickaxe")]
    for lv, item in gear:
        reached = lv <= level
        c.fill(at(lv), cy + 25, 1, 19, 0xFFF2C14E if reached else 0xFF3A4152)
        c.slot(at(lv) - 9, cy + 7, item, locked=not reached)
    c.fill(at(32), cy + 31, 1, 13, 0xFF3A4152)
    c.sprite("advancements/challenge_frame_unobtained", at(32) - 13, cy + 5, 26, 26)
    c.item("minecraft:nether_star", at(32) - 8, cy + 10, 16, dim=True)
    for lv in [1, 4, 8, 12, 16, 20, 24, 28, 32]:
        label = str(lv)
        pw = FONT.width(label) + 5
        px = at(lv) - pw // 2
        here = lv == level
        c.fill(px, ry - 4, pw, 11, BLACK)
        c.fill(px + 1, ry - 3, pw - 2, 9, 0xFFF2C14E if here else (0xFF4A3A16 if lv < level else 0xFF1B1E26))
        c.text(label, px + 3, ry - 2, 0x1A1408 if here else (GOLD if lv < level else DIM), False)
    for i in range(4):
        c.fill(at(level) - 3 + i, ry - 9 + i, 7 - 2 * i, 1, 0xFFFFFFFF)
    perks = [(4, "minecraft:stone_pickaxe", "owned", 2), (8, "minecraft:raw_iron", "open", 2),
             (12, "minecraft:spyglass", "open", 2), (16, "minecraft:water_bucket", "locked", 4),
             (24, "minecraft:note_block", "locked", 6)]
    for lv, item, state, cost in perks:
        colour = {"owned": 0xFFF2C14E, "open": 0xFF67C14A, "locked": 0xFF3A4152}[state]
        c.fill(at(lv), ry + 5, 1, 9, colour)
        c.node(at(lv) - 13, ry + 14, item, state, capstone=lv == 24)
        if state != "owned":
            label = "◆" * cost if cost <= 3 else f"◆{cost}"
            c.centred(label, at(lv) - 20, ry + 41, 40, GOLD if state == "open" else DIM, False)
    c.edge(at(8) - 15, ry + 12, 30, 30, 0xFFFFFFFF)
    dy = cy + ch + 5
    dh = WY + WH - 8 - dy
    c.fill(x0, dy, x1 - x0, dh, BLACK)
    c.fill(x0 + 1, dy + 1, x1 - x0 - 2, dh - 2, CARD)
    c.fill(x0 + 1, dy + 1, 2, dh - 2, ACCENT["Mining"])
    c.sprite("advancements/task_frame_unobtained", x0 + 8, dy + 7, 26, 26)
    c.item("minecraft:raw_iron", x0 + 13, dy + 12)
    c.text("Deep Veins", x0 + 40, dy + 6, GOLD)
    c.text("Mining perk · level 8", x0 + 40 + FONT.width("Deep Veins  "), dy + 6, DIM, False)
    c.text("•", x0 + 40, dy + 18, GOLD)
    c.text("15%", x0 + 47, dy + 18, WHITE)
    c.text("chance of extra drops from coal, copper", x0 + 47 + FONT.width("15% "), dy + 18, GRAY, False)
    c.text("and iron ore.", x0 + 47, dy + 28, GRAY, False)
    c.text("✔ Mining 8", x1 - 150, dy + 28, GREEN, False)
    c.text("◆ 2 points", x1 - 150 + FONT.width("✔ Mining 8   "), dy + 28, GREEN, False)
    c.button(x1 - 70, dy + 7, 64, 16, "Unlock", GO)
    c.save("road_skill")


def home():
    c = Canvas()
    chrome(c, "Home")
    x0, x1, y0, y1 = WX + 8, WX + WW - 8, WY + 9, WY + WH - 8
    strip_h = 16
    top_y1 = y1 - strip_h - 6
    # you
    cw = 104
    c.fill(x0, y0, cw, top_y1 - y0, BLACK)
    c.fill(x0 + 1, y0 + 1, cw - 2, top_y1 - y0 - 2, CARD)
    mx = x0 + cw // 2
    c.sprite("advancements/goal_frame_obtained", mx - 16, y0 + 6, 32, 32)
    c.art(FACE, FACE_PAL, mx - 8, y0 + 14, 2)
    c.centred("xKincott", x0, y0 + 42, cw, WHITE)
    c.centred("Total level 65", x0, y0 + 52, cw, GOLD)
    c.fill(x0 + 8, y0 + 64, cw - 16, 1, BRASS_DARK)
    c.text("GEAR", x0 + 8, y0 + 69, DIM, False)
    gx, gy = mx - 29, y0 + 80
    gear = ["minecraft:iron_helmet", "minecraft:iron_chestplate", "minecraft:diamond_pickaxe",
            "minecraft:iron_leggings", "minecraft:iron_boots", None]
    for i, item in enumerate(gear):
        c.slot(gx + (i % 3) * 20, gy + (i // 3) * 20, item, locked=item == "minecraft:diamond_pickaxe")
    c.centred("\u2718 Pickaxe: Mining 16", x0, gy + 45, cw, RED, False)
    # the eight roads
    rx = x0 + cw + 6
    rw = x1 - rx
    rh = (top_y1 - y0 - 7 * 2) / 8
    for i, (name, icon, lvl, pts) in enumerate(SKILLS):
        y = round(y0 + i * (rh + 2))
        h = round(rh)
        hover = name == "Mining"
        mastered = lvl == 32
        c.fill(rx, y, rw, h, BLACK)
        c.fill(rx + 1, y + 1, rw - 2, h - 2, 0xFF332B1D if mastered else (CARD_HI if hover else CARD))
        c.fill(rx + 1, y + 1, 2, h - 2, ACCENT[name])
        if hover:
            c.edge(rx, y, rw, h, BRASS_LIGHT)
        ty = y + (h - 8) // 2 + 1
        c.item(icon, rx + 6, y + (h - 16) // 2)
        c.text(name, rx + 26, ty, WHITE)
        c.right(str(lvl), rx + 108, ty, GOLD if mastered else WHITE)
        c.right("Lv ", rx + 108 - FONT.width(str(lvl)), ty, DIM, False)
        mx0, mx1 = rx + 116, rx + rw - 62
        my = y + h // 2 - 1

        def mat(lv):
            return round(mx0 + (lv - 1) * (mx1 - mx0) / 31)

        c.fill(mx0 - 1, my - 1, mx1 - mx0 + 2, 4, BLACK)
        c.fill(mx0, my, mx1 - mx0, 2, 0xFF3A4152)
        c.fill(mx0, my, mat(lvl) - mx0, 2, 0xFFF2C14E)
        for lv in PERK_LEVELS:
            state = perk_state(name, lvl, lv)
            colour = {"owned": 0xFFF2C14E, "open": 0xFF67C14A, "locked": 0xFF2A2F3B}[state]
            px = mat(lv)
            c.fill(px - 3, my - 3, 7, 8, BLACK)
            c.fill(px - 2, my - 2, 5, 6, colour)
            if state == "owned":
                c.fill(px - 1, my - 1, 3, 1, 0xFFFFE79A)
        if mastered:
            c.right("\u2605 Mastered", rx + rw - 6, ty, GOLD)
        else:
            if pts:
                c.right(f"\u25c6{pts}", rx + rw - 44, ty, GOLD)
            c.button(rx + rw - 40, y + (h - 13) // 2, 35, 13, str(lvl), GO, arrow=True)
    # focus, along the bottom
    fy = y1 - strip_h
    c.fill(x0, fy - 4, x1 - x0, 1, BRASS_DARK)
    c.text("FOCUS", x0 + 2, fy + 4, GOLD)
    bx0 = x0 + FONT.width("FOCUS") + 10
    bw = (x1 - bx0 - 2 * 4) / 3
    for i, (name, on) in enumerate([("Fighting", False), ("Roaming", False), ("Working", True)]):
        c.button(round(bx0 + i * (bw + 4)), fy, round(bw), strip_h, ("\u2714 " if on else "") + name,
                 GO if on else PLAIN)
    c.save("road_home")


def options():
    c = Canvas()
    chrome(c, "Options")
    x0, x1, y0, y1 = WX + 8, WX + WW - 8, WY + 9, WY + WH - 8
    half = (x1 - x0 - 8) // 2
    row_h = 12

    def column(x, title, sections, locked=False):
        c.text(title, x, y0, GOLD)
        c.fill(x + FONT.width(title) + 5, y0 + 4, half - FONT.width(title) - 5, 1, BRASS_DARK)
        y = y0 + 12
        for name, rows in sections:
            if name:
                c.text(name, x, y + 1, DIM, False)
                y += 10
            for label, value, on in rows:
                hover = label == "Padlock corner"
                c.fill(x, y, half, row_h, BLACK)
                c.fill(x + 1, y + 1, half - 2, row_h - 2, CARD_HI if hover else CARD)
                c.text(label, x + 5, y + 2, GRAY if locked else WHITE, not locked)
                look = OFF if locked else (GO if on else PLAIN)
                vw = max(FONT.width(value) + 10, 30)
                c.button(x + half - vw - 1, y + 1, vw, row_h - 2, value, look)
                y += row_h + 1
            y += 2
        return y

    column(x0, "DISPLAY", [
        ("Lock warning", [("Position", "Above hotbar", False), ("Stays for", "3 s", False)]),
        ("Lock icon", [("Padlock on items", "On", True), ("Padlock corner", "Bottom left", False),
                       ("Dim locked items", "On", True)]),
        ("Tooltips", [("Hide met needs", "Off", False), ("Show your level", "On", True)]),
        ("Sounds", [("Clicks", "On", True), ("Level-up", "On", True), ("Refusal", "On", True)]),
    ])
    rx = x0 + half + 8
    end = column(rx, "SERVER RULES", [
        ("", [("Raises cost", "XP levels", False), ("Points a level", "20", False),
              ("Skills grow by", "Buying", False), ("Earning pace", "100%", False),
              ("Perk switches", "On", True), ("Enchantment locks", "On", True),
              ("Shared skills", "Off", False)]),
    ], locked=True)
    c.padlock(rx, end + 1)
    c.text("Only operators can change these.", rx + 11, end + 1, DIM, False)
    hy = y1 - 40
    c.fill(rx, hy, half, 20, BLACK)
    c.fill(rx + 1, hy + 1, half - 2, 18, INSET)
    c.text("Padlock corner", rx + 5, hy + 2, GOLD, False)
    c.text("Where the padlock sits on an item.", rx + 5, hy + 11, GRAY, False)
    c.button(rx, y1 - 16, half // 2 - 2, 16, "Defaults", PLAIN)
    c.button(rx + half // 2 + 2, y1 - 16, half - half // 2 - 2, 16, "Done", GO)
    c.save("road_options")


road()
home()
options()


FOCUS_GROUPS = [("Fighting", "minecraft:iron_sword", False, ["Melee", "Ranged", "Defense"], "+50% kill XP"),
                ("Working", "minecraft:iron_pickaxe", True, ["Mining", "Gathering", "Arcane"], "+50% ore XP"),
                ("Roaming", "minecraft:feather", False, ["Agility", "Building"], "travel XP")]


def skill_row(c, rx, y, rw, h, skill):
    name, icon, lvl, pts = skill
    hover = name == "Mining"
    mastered = lvl == 32
    c.fill(rx, y, rw, h, BLACK)
    c.fill(rx + 1, y + 1, rw - 2, h - 2, 0xFF332B1D if mastered else (CARD_HI if hover else CARD))
    c.fill(rx + 1, y + 1, 2, h - 2, ACCENT[name])
    if hover:
        c.edge(rx, y, rw, h, BRASS_LIGHT)
    ty = y + (h - 8) // 2 + 1
    c.item(icon, rx + 6, y + (h - 16) // 2)
    c.text(name, rx + 26, ty, WHITE)
    c.right(str(lvl), rx + 108, ty, GOLD if mastered else WHITE)
    c.right("Lv ", rx + 108 - FONT.width(str(lvl)), ty, DIM, False)
    mx0, mx1 = rx + 116, rx + rw - 62
    my = y + h // 2 - 1

    def mat(lv):
        return round(mx0 + (lv - 1) * (mx1 - mx0) / 31)

    c.fill(mx0 - 1, my - 1, mx1 - mx0 + 2, 4, BLACK)
    c.fill(mx0, my, mx1 - mx0, 2, 0xFF3A4152)
    c.fill(mx0, my, mat(lvl) - mx0, 2, 0xFFF2C14E)
    for lv in PERK_LEVELS:
        state = perk_state(name, lvl, lv)
        colour = {"owned": 0xFFF2C14E, "open": 0xFF67C14A, "locked": 0xFF2A2F3B}[state]
        px = mat(lv)
        c.fill(px - 3, my - 3, 7, 8, BLACK)
        c.fill(px - 2, my - 2, 5, 6, colour)
    if mastered:
        c.right("★ Mastered", rx + rw - 6, ty, GOLD)
    else:
        if pts:
            c.right(f"◆{pts}", rx + rw - 44, ty, GOLD)
        c.button(rx + rw - 40, y + (h - 13) // 2, 35, 13, str(lvl), GO, arrow=True)


def you_panel(c, x0, y0, cw, y1, tail):
    c.fill(x0, y0, cw, y1 - y0, BLACK)
    c.fill(x0 + 1, y0 + 1, cw - 2, y1 - y0 - 2, CARD)
    mx = x0 + cw // 2
    c.sprite("advancements/goal_frame_obtained", mx - 16, y0 + 6, 32, 32)
    c.art(FACE, FACE_PAL, mx - 8, y0 + 14, 2)
    c.centred("xKincott", x0, y0 + 42, cw, WHITE)
    c.centred("Total level 65", x0, y0 + 52, cw, GOLD)
    c.fill(x0 + 8, y0 + 64, cw - 16, 1, BRASS_DARK)
    c.text("GEAR", x0 + 8, y0 + 69, DIM, False)
    gx, gy = mx - 29, y0 + 80
    gear = ["minecraft:iron_helmet", "minecraft:iron_chestplate", "minecraft:diamond_pickaxe",
            "minecraft:iron_leggings", "minecraft:iron_boots", None]
    for i, item in enumerate(gear):
        c.slot(gx + (i % 3) * 20, gy + (i // 3) * 20, item, locked=item == "minecraft:diamond_pickaxe")
    c.centred("✘ Pickaxe: Mining 16", x0, gy + 45, cw, RED, False)
    tail(gy + 58)


def home_medallions():
    c = Canvas()
    chrome(c, "Home")
    x0, x1, y0, y1 = WX + 8, WX + WW - 8, WY + 9, WY + WH - 8
    cw = 104

    def focus(y):
        c.fill(x0 + 8, y, cw - 16, 1, BRASS_DARK)
        c.text("FOCUS", x0 + 8, y + 4, DIM, False)
        for i, (name, icon, on, _, _) in enumerate(FOCUS_GROUPS):
            fx = x0 + 10 + i * 30
            fy = y + 15
            c.sprite("advancements/goal_frame_obtained" if on else "advancements/goal_frame_unobtained",
                     fx, fy, 26, 26)
            c.item(icon, fx + 5, fy + 5, 16, dim=not on)
            if on:
                c.fill(fx + 17, fy + 17, 8, 8, BLACK)
                c.text("✔", fx + 18, fy + 17, GOLD, False)
        c.centred("Working", x0, y + 45, cw, GOLD, False)

    you_panel(c, x0, y0, cw, y1, focus)
    rx = x0 + cw + 6
    rw = x1 - rx
    rh = (y1 - y0 - 7 * 2) / 8
    for i, skill in enumerate(SKILLS):
        skill_row(c, rx, round(y0 + i * (rh + 2)), rw, round(rh), skill)
    c.save("focus_a_medallions")


def home_groups():
    c = Canvas()
    chrome(c, "Home")
    x0, x1, y0, y1 = WX + 8, WX + WW - 8, WY + 9, WY + WH - 8
    cw = 104
    you_panel(c, x0, y0, cw, y1, lambda y: None)
    rx = x0 + cw + 6
    rw = x1 - rx
    head_h, row_h, gap = 12, 16, 1
    y = y0
    by_name = {s[0]: s for s in SKILLS}
    for name, icon, on, skills, bonus in FOCUS_GROUPS:
        c.text(name.upper(), rx + 2, y + 2, GOLD if on else GRAY)
        tx = rx + 4 + FONT.width(name.upper())
        c.text(bonus, tx + 4, y + 2, DIM, False)
        lx = tx + 8 + FONT.width(bonus)
        c.fill(lx, y + 6, rw - (lx - rx) - 62, 1, BRASS if on else 0xFF3A4152)
        if on:
            c.right("✔ Focused", rx + rw - 4, y + 2, GOLD, False)
        else:
            c.button(rx + rw - 56, y, 56, head_h - 1, "Focus", PLAIN)
        y += head_h + 1
        for s in skills:
            skill_row(c, rx, y, rw, row_h, by_name[s])
            y += row_h + gap
        y += 3
    c.save("focus_b_groups")


home_medallions()
home_groups()


def you_doll(c, x0, y0, cw, y1):
    c.fill(x0, y0, cw, y1 - y0, BLACK)
    c.fill(x0 + 1, y0 + 1, cw - 2, y1 - y0 - 2, CARD)
    mx = x0 + cw // 2
    c.art(FACE, FACE_PAL, x0 + 7, y0 + 5, 1)
    c.edge(x0 + 6, y0 + 4, 10, 10, BRASS)
    c.text("xKincott", x0 + 20, y0 + 6, WHITE)
    c.right("65", x0 + cw - 8, y0 + 6, GOLD)
    c.right("Lv ", x0 + cw - 8 - FONT.width("65"), y0 + 6, DIM, False)
    c.fill(x0 + 8, y0 + 18, cw - 16, 1, BRASS_DARK)
    # the doll: the gear shaped like a body in a stone alcove
    ax, ay, aw, ah = x0 + 8, y0 + 23, cw - 16, 90
    c.fill(ax, ay, aw, ah, INSET)
    c.tile("gui/advancements/backgrounds/stone", ax + 1, ay + 1, aw - 2, ah - 2, 0.72)
    for i, item in enumerate(["minecraft:iron_helmet", "minecraft:iron_chestplate", "minecraft:iron_leggings",
                              "minecraft:iron_boots"]):
        c.slot(mx - 9, ay + 6 + i * 20, item)
    c.slot(mx - 31, ay + 26, "minecraft:diamond_pickaxe", locked=True)
    c.slot(mx + 13, ay + 26, None)
    c.centred("\u2718 Pickaxe: Mining 16", x0, ay + ah + 4, cw, RED, False)
    # stats
    sy = ay + ah + 18
    c.fill(x0 + 8, sy - 4, cw - 16, 1, BRASS_DARK)
    stats = [("Perks", "7 / 40", WHITE), ("Mastered", "1 / 8", GOLD), ("Points", "12", GOLD)]
    for i, (label, value, colour) in enumerate(stats):
        y = sy + i * 11
        c.text(label, x0 + 8, y, DIM, False)
        c.right(value, x0 + cw - 8, y, colour)


def home_groups_doll():
    c = Canvas()
    chrome(c, "Home")
    x0, x1, y0, y1 = WX + 8, WX + WW - 8, WY + 9, WY + WH - 8
    cw = 104
    you_doll(c, x0, y0, cw, y1)
    rx = x0 + cw + 6
    rw = x1 - rx
    head_h, row_h, gap = 12, 16, 1
    y = y0
    by_name = {s[0]: s for s in SKILLS}
    for name, icon, on, skills, bonus in FOCUS_GROUPS:
        c.text(name.upper(), rx + 2, y + 2, GOLD if on else GRAY)
        tx = rx + 4 + FONT.width(name.upper())
        c.text(bonus, tx + 4, y + 2, DIM, False)
        lx = tx + 8 + FONT.width(bonus)
        c.fill(lx, y + 6, rw - (lx - rx) - 62, 1, BRASS if on else 0xFF3A4152)
        if on:
            c.right("✔ Focused", rx + rw - 4, y + 2, GOLD, False)
        else:
            c.button(rx + rw - 56, y, 56, head_h - 1, "Focus", PLAIN)
        y += head_h + 1
        for s in skills:
            skill_row(c, rx, y, rw, row_h, by_name[s])
            y += row_h + gap
        y += 3
    c.save("home_doll")


home_groups_doll()
