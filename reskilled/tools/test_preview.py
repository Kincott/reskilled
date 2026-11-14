"""Checks preview.py's drawing of guide scenes and plain sprites. Run with
`python3 -I -m unittest discover -s reskilled/tools -p "test_*.py"`. Needs Pillow."""

import pathlib
import sys
import tempfile
import unittest

from PIL import Image

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import preview  # noqa: E402

VIEW = {"center": [0.5, 0.5, 0.5], "yaw": 0.0, "pitch": 0.0, "scale": 20.0}


class ProjectTest(unittest.TestCase):
    def test_the_center_lands_in_the_middle_and_east_on_the_left(self):
        # Given an unturned, untilted view of a 100 by 60 box
        rect = [0.0, 0.0, 100.0, 60.0]

        # When the center, a point up and a point east are projected
        center = preview.project([0.5, 0.5, 0.5], VIEW, rect)
        up = preview.project([0.5, 1.5, 0.5], VIEW, rect)
        east = preview.project([1.5, 0.5, 0.5], VIEW, rect)

        # Then the center is the box's middle, up is up the screen and east is on the left, as the game draws it
        self.assertEqual([round(v, 3) for v in center[:2]], [50.0, 30.0])
        self.assertLess(up[1], 30.0)
        self.assertLess(east[0], 50.0)


class SceneTest(unittest.TestCase):
    def test_a_scene_draws_its_blocks_inside_its_box_only(self):
        # Given an empty canvas and a scene of two blocks in a box
        canvas = Image.new("RGBA", (120, 80), (0, 0, 0, 255))
        paint = {"rect": [20.0, 10.0, 100.0, 70.0], "blocks": [[[0, 0, 0], 1000], [[0, 1, 0], 1001]],
                 "center": [0.5, 1.0, 0.5], "yaw": 35.0, "pitch": 30.0, "scale": 16.0}
        states = {"1000": "minecraft:stone", "1001": "minecraft:wheat[age=7]"}

        # When the scene is drawn
        preview.scene(canvas, paint, states)

        # Then pixels changed, and none outside the box
        changed = [(x, y) for x in range(120) for y in range(80) if canvas.getpixel((x, y)) != (0, 0, 0, 255)]
        self.assertTrue(changed)
        self.assertTrue(all(20 <= x < 100 and 10 <= y < 70 for x, y in changed))


class SpriteTest(unittest.TestCase):
    def test_effect_icons_come_from_the_mob_effect_folder(self):
        # Given a resource folder with an effect icon and a heart sprite
        with tempfile.TemporaryDirectory() as tmp:
            assets = pathlib.Path(tmp)
            for name in ("mob_effect/strength", "gui/sprites/hud/heart/full"):
                file = assets / "textures" / f"{name}.png"
                file.parent.mkdir(parents=True)
                Image.new("RGBA", (9, 9), (200, 0, 0, 255)).save(file)

            # When each is looked up as the game names it
            effect = preview.sprite_file(assets, "mob_effect/strength")
            heart = preview.sprite_file(assets, "gui/sprites/hud/heart/full")
            missing = preview.sprite_file(assets, "gui/sprites/nothing")

            # Then both are found where the game keeps them, and an unknown one is not
            self.assertEqual(effect, assets / "textures/mob_effect/strength.png")
            self.assertEqual(heart, assets / "textures/gui/sprites/hud/heart/full.png")
            self.assertIsNone(missing)


if __name__ == "__main__":
    unittest.main()
