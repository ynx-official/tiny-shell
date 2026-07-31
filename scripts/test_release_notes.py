from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.release_notes import ReleaseNotesError, generate_release_notes


class ReleaseNotesTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        (self.root / "docs" / "upgrade" / "v1.2.3").mkdir(parents=True)
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "tiny-shell"\nversion = "1.2.3"\n',
            encoding="utf-8",
        )
        (self.root / "CHANGELOG.md").write_text(
            "# Changelog\n\n"
            "## [1.2.3] - 2026-07-30\n\n"
            "- 发布摘要。\n\n"
            "[1.2.3]: docs/upgrade/v1.2.3/README.md\n",
            encoding="utf-8",
        )
        (self.root / "docs" / "upgrade" / "README.md").write_text(
            "当前代码版本为 [`v1.2.3`](v1.2.3/README.md)。\n\n"
            "| [v1.2.3](v1.2.3/README.md) | 2026-07-30 | 发布摘要。 |\n",
            encoding="utf-8",
        )
        self.detail_path = (
            self.root / "docs" / "upgrade" / "v1.2.3" / "README.md"
        )
        self.detail_path.write_text(
            "# TinyShell v1.2.3\n\n"
            "> 发布日期：2026-07-30\n\n"
            "## 版本概述\n\n发布概述。\n\n"
            "## 新功能\n\n- 新功能。\n\n"
            "## 验证结果\n\n- 测试通过。\n\n"
            "## 变更依据\n\n- 比较链接。\n\n"
            "[返回版本总览](../README.md)\n",
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_generate_release_notes_keeps_user_facing_sections(self) -> None:
        notes = generate_release_notes(self.root, "v1.2.3")

        self.assertTrue(notes.startswith("# TinyShell v1.2.3"))
        self.assertIn("> 发布日期：2026-07-30", notes)
        self.assertIn("## 版本概述", notes)
        self.assertIn("## 新功能", notes)
        self.assertNotIn("验证结果", notes)
        self.assertNotIn("变更依据", notes)
        self.assertNotIn("返回版本总览", notes)

    def test_rejects_tag_that_does_not_match_cargo_version(self) -> None:
        with self.assertRaisesRegex(ReleaseNotesError, "与 Cargo.toml 版本 1.2.3 不一致"):
            generate_release_notes(self.root, "v1.2.4")

    def test_rejects_missing_changelog_entry(self) -> None:
        (self.root / "CHANGELOG.md").write_text("# Changelog\n", encoding="utf-8")

        with self.assertRaisesRegex(ReleaseNotesError, "缺少必要内容"):
            generate_release_notes(self.root, "v1.2.3")


if __name__ == "__main__":
    unittest.main()