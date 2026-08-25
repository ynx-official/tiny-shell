from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts.update_manifest import (
    UpdateManifestError,
    build_manifest,
    expected_asset_names,
    write_manifest,
)


class UpdateManifestTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        self.dist = self.root / "dist"
        self.dist.mkdir()
        self.tag = "v1.2.3"
        for index, name in enumerate(sorted(expected_asset_names(self.tag)), start=1):
            platform_dir = self.dist / f"artifact-{index}"
            platform_dir.mkdir()
            (platform_dir / name).write_bytes(f"asset-{index}".encode())
        (self.dist / "release-notes.md").write_text(
            "## 版本概述\n\n本次更新。\n", encoding="utf-8"
        )

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_builds_deterministic_manifest_from_actual_assets(self) -> None:
        manifest = build_manifest(self.dist, self.tag, "ynx-official/tiny-shell")

        self.assertEqual(manifest["schema_version"], 1)
        self.assertEqual(manifest["version"], self.tag)
        self.assertEqual(
            manifest["notes_url"],
            "https://github.com/ynx-official/tiny-shell/releases/download/"
            f"{self.tag}/release-notes.md",
        )
        self.assertEqual(len(manifest["assets"]), 12)
        names = [asset["name"] for asset in manifest["assets"]]
        self.assertEqual(names, sorted(names))
        self.assertIn(
            f"tiny-shell-{self.tag}-linux-x86_64.AppImage",
            names,
        )

        first = manifest["assets"][0]
        first_path = next(self.dist.rglob(str(first["name"])))
        self.assertEqual(first["size"], first_path.stat().st_size)
        self.assertEqual(
            first["digest"],
            f"sha256:{hashlib.sha256(first_path.read_bytes()).hexdigest()}",
        )
        self.assertEqual(
            first["url"],
            f"https://github.com/ynx-official/tiny-shell/releases/download/"
            f"{self.tag}/{first['name']}",
        )

    def test_writes_stable_json_with_trailing_newline(self) -> None:
        output = self.root / "update-manifest.json"
        expected = write_manifest(
            self.dist, self.tag, "ynx-official/tiny-shell", output
        )

        content = output.read_text(encoding="utf-8")
        self.assertTrue(content.endswith("\n"))
        self.assertEqual(json.loads(content), expected)

    def test_rejects_missing_asset(self) -> None:
        next(self.dist.rglob("*-setup.exe")).unlink()

        with self.assertRaisesRegex(UpdateManifestError, "缺少"):
            build_manifest(self.dist, self.tag, "ynx-official/tiny-shell")

    def test_rejects_unexpected_asset(self) -> None:
        (self.dist / "debug.log").write_text("debug", encoding="utf-8")

        with self.assertRaisesRegex(UpdateManifestError, "多余"):
            build_manifest(self.dist, self.tag, "ynx-official/tiny-shell")

    def test_rejects_duplicate_asset_names(self) -> None:
        source = next(self.dist.rglob("*-portable.zip"))
        duplicate_dir = self.dist / "duplicate"
        duplicate_dir.mkdir()
        (duplicate_dir / source.name).write_bytes(source.read_bytes())

        with self.assertRaisesRegex(UpdateManifestError, "重名"):
            build_manifest(self.dist, self.tag, "ynx-official/tiny-shell")


if __name__ == "__main__":
    unittest.main()
