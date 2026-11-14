#!/usr/bin/env python3
"""Builds dist/reskilled.hmod: one file with the content mod (horizon-mod.toml, assets/, data/) and the native bundle
(horizon-native.json, mixins/reskilled/), as Horizon's mod-files rules ask. Standard library only; the same sources
give the same bytes (sorted entries, fixed timestamps, deflate).

    python3 -I reskilled/tools/package.py            # writes dist/reskilled.hmod at the repository root
"""

import json
import pathlib
import re
import sys
import zipfile

MOD_ID = "reskilled"
GAME = "26.3"
#: The zip's timestamp for every entry: the earliest a zip can hold, so builds do not depend on the clock.
FIXED_TIME = (1980, 1, 1, 0, 0, 0)
#: Inside the bundle, Horizon takes only these.
BUNDLE_SUFFIXES = (".rs", ".toml")
SKIPPED_DIRS = {"target", ".git"}


def pin_values(root):
    """The Horizon commit, the horizon-mixins commit and the hook API number, from HORIZON_PIN.md."""
    pin = (root / "HORIZON_PIN.md").read_text()
    horizon = re.search(r"\| Horizon commit \| `([0-9a-f]{40})` \|", pin).group(1)
    mixins = re.search(r"\| horizon-mixins commit \| `([0-9a-f]{40})`", pin).group(1)
    hooks = int(re.search(r"\| `NATIVE_HOOKS_API` \| (\d+)", pin).group(1))
    return horizon, mixins, hooks


def toml_value(text, key):
    return re.search(rf'^{key} = "([^"]+)"', text, re.M).group(1)


def files_under(folder, keep):
    """Every file under `folder` that `keep` accepts, skipping build output, as paths relative to `folder`."""
    found = []
    for path in sorted(folder.rglob("*")):
        relative = path.relative_to(folder)
        if not path.is_file() or SKIPPED_DIRS.intersection(relative.parts) or not keep(path):
            continue
        found.append(relative)
    return found


def entries(root):
    """(name in the archive, bytes) for every entry, in sorted order."""
    content = root / "content"
    native = root / "native"
    manifest = (content / "horizon-mod.toml").read_text()
    bundle_manifest = (native / "horizon-mixin.toml").read_text()
    version = toml_value(manifest, "version")
    if toml_value(manifest, "id") != MOD_ID or toml_value(bundle_manifest, "id") != MOD_ID:
        raise SystemExit("the content and native manifests must both name the mod reskilled")
    if toml_value(bundle_manifest, "version") != version:
        raise SystemExit("the content and native manifests name different versions")
    horizon, mixins, hooks = pin_values(root)
    if toml_value(bundle_manifest, "horizon_revision") != horizon:
        raise SystemExit("horizon-mixin.toml's horizon_revision is not the pinned commit")
    native_json = {
        "schema": 1,
        "id": MOD_ID,
        "version": version,
        "game": GAME,
        "horizon": horizon,
        "mixins": mixins,
        "hooks": hooks,
    }
    out = {
        "horizon-mod.toml": manifest.encode(),
        "horizon-native.json": (json.dumps(native_json, indent=2) + "\n").encode(),
    }
    for top in ("assets", "data"):
        for relative in files_under(content / top, lambda _: True):
            out[f"{top}/{relative.as_posix()}"] = (content / top / relative).read_bytes()
    for relative in files_under(native, lambda p: p.suffix in BUNDLE_SUFFIXES):
        out[f"mixins/{MOD_ID}/{relative.as_posix()}"] = (native / relative).read_bytes()
    return sorted(out.items())


def build(root, out):
    """Writes the .hmod for the mod folder `root` to `out`."""
    out = pathlib.Path(out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w") as archive:
        for name, data in entries(pathlib.Path(root)):
            info = zipfile.ZipInfo(name, FIXED_TIME)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, data)
    return out


def main():
    root = pathlib.Path(__file__).resolve().parent.parent
    out = build(root, root.parent / "dist" / f"{MOD_ID}.hmod")
    print(out)


if __name__ == "__main__":
    sys.exit(main())
