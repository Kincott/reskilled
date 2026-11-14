"""Checks package.py's .hmod: one file holding the content mod and the native bundle, as Horizon's mod-files rules
want it. Run with `python3 -I -m unittest discover -s reskilled/tools -p "test_*.py"`."""

import json
import pathlib
import re
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import package  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTENT_PATH = re.compile(r"^(assets|data)(/[a-z0-9_.-]+){2,}$")


def build(out_dir):
    out = pathlib.Path(out_dir) / "reskilled.hmod"
    package.build(ROOT, out)
    return out


class PackageTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.hmod = build(self.tmp.name)
        self.zip = zipfile.ZipFile(self.hmod)
        self.names = self.zip.namelist()

    def tearDown(self):
        self.zip.close()
        self.tmp.cleanup()

    def test_archive_has_only_allowed_entries(self):
        # Given the built file
        # When each entry is looked at
        # Then it is a manifest, content under assets/ or data/, or a .rs or .toml file under mixins/reskilled/
        for name in self.names:
            allowed = (
                name in ("horizon-mod.toml", "horizon-native.json")
                or CONTENT_PATH.match(name)
                or (name.startswith("mixins/reskilled/") and name.endswith((".rs", ".toml")))
            )
            self.assertTrue(allowed, name)
        self.assertIn("mixins/reskilled/horizon-mixin.toml", self.names)
        self.assertIn("mixins/reskilled/api/src/lib.rs", self.names)
        self.assertIn("data/reskilled/reskilled/locks/tiers.json", self.names)
        self.assertIn("assets/reskilled/lang/en_us.json", self.names)

    def test_manifests_name_the_same_id_and_version(self):
        # Given the three manifests inside
        native = json.loads(self.zip.read("horizon-native.json"))
        content = self.zip.read("horizon-mod.toml").decode()
        mixin = self.zip.read("mixins/reskilled/horizon-mixin.toml").decode()

        # When their ids and versions are read
        content_id = re.search(r'^id = "([^"]+)"', content, re.M).group(1)
        content_version = re.search(r'^version = "([^"]+)"', content, re.M).group(1)
        mixin_id = re.search(r'^id = "([^"]+)"', mixin, re.M).group(1)
        mixin_version = re.search(r'^version = "([^"]+)"', mixin, re.M).group(1)

        # Then they all agree
        self.assertEqual({native["id"], content_id, mixin_id}, {"reskilled"})
        self.assertEqual({native["version"], content_version, mixin_version}, {content_version})

    def test_native_json_fields_match_the_pin(self):
        # Given the native manifest and the pin file
        native = json.loads(self.zip.read("horizon-native.json"))
        pin = (ROOT / "HORIZON_PIN.md").read_text()
        mixin = self.zip.read("mixins/reskilled/horizon-mixin.toml").decode()

        # When they are compared
        # Then the manifest carries exactly the allowed fields, with the pin's commits, game and hook version
        self.assertEqual(set(native), {"schema", "id", "version", "game", "horizon", "mixins", "hooks"})
        self.assertEqual(native["schema"], 1)
        self.assertEqual(native["game"], "26.3")
        self.assertIn(native["horizon"], pin)
        self.assertIn(native["horizon"], mixin)
        self.assertIn(native["mixins"], pin)
        self.assertRegex(native["horizon"], r"^[0-9a-f]{40}$")
        self.assertRegex(native["mixins"], r"^[0-9a-f]{40}$")
        self.assertEqual(native["hooks"], 415)

    def test_no_cargo_lock_or_target_inside(self):
        # Given the built file
        # When its paths are looked at
        # Then no build output, lock file or editor junk went in
        for name in self.names:
            self.assertNotIn("/target/", "/" + name)
            self.assertFalse(name.endswith("Cargo.lock"), name)
            self.assertNotIn(".DS_Store", name)

    def test_build_is_reproducible(self):
        # Given a second build into another place
        with tempfile.TemporaryDirectory() as other:
            again = build(other)

            # When the two files are compared
            # Then they are the same bytes, with deflated entries in sorted order
            self.assertEqual(self.hmod.read_bytes(), again.read_bytes())
        self.assertEqual(self.names, sorted(self.names))
        self.assertTrue(all(info.compress_type == zipfile.ZIP_DEFLATED for info in self.zip.infolist()))


if __name__ == "__main__":
    unittest.main()
