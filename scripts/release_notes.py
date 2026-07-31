#!/usr/bin/env python3
"""校验 TinyShell 发布元数据，并从版本详情生成 GitHub Release Notes。"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

TAG_PATTERN = re.compile(r"^v(?P<version>\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$")
DATE_PATTERN = re.compile(r"^> 发布日期：(?P<date>\d{4}-\d{2}-\d{2})$", re.MULTILINE)
EXCLUDED_HEADINGS = {"验证结果", "变更依据"}


class ReleaseNotesError(ValueError):
    """发布资料不完整或互相矛盾。"""


@dataclass(frozen=True)
class ReleaseMetadata:
    tag: str
    version: str
    release_date: str
    detail_path: Path


def read_package_version(cargo_toml: Path) -> str:
    content = cargo_toml.read_text(encoding="utf-8")
    package_match = re.search(r"(?ms)^\[package\]\s*$.*?(?=^\[|\Z)", content)
    if package_match is None:
        raise ReleaseNotesError("Cargo.toml 缺少 [package] 配置")

    version_match = re.search(
        r'(?m)^version\s*=\s*"(?P<version>[^"]+)"\s*$', package_match.group(0)
    )
    if version_match is None:
        raise ReleaseNotesError("Cargo.toml 的 [package] 缺少 version")
    return version_match.group("version")


def parse_tag(tag: str) -> str:
    match = TAG_PATTERN.fullmatch(tag)
    if match is None:
        raise ReleaseNotesError(
            f"无效发布标签 {tag!r}，必须使用 vMAJOR.MINOR.PATCH 格式"
        )
    return match.group("version")


def require_contains(content: str, expected: str, source: Path) -> None:
    if expected not in content:
        raise ReleaseNotesError(f"{source.as_posix()} 缺少必要内容：{expected}")


def load_metadata(root: Path, tag: str) -> tuple[ReleaseMetadata, str]:
    version = parse_tag(tag)
    cargo_version = read_package_version(root / "Cargo.toml")
    if version != cargo_version:
        raise ReleaseNotesError(
            f"发布标签 {tag} 与 Cargo.toml 版本 {cargo_version} 不一致"
        )

    detail_path = root / "docs" / "upgrade" / tag / "README.md"
    if not detail_path.is_file():
        raise ReleaseNotesError(f"缺少版本详情文档：{detail_path.as_posix()}")

    detail = detail_path.read_text(encoding="utf-8")
    require_contains(detail, f"# TinyShell {tag}", detail_path)

    date_match = DATE_PATTERN.search(detail)
    if date_match is None:
        raise ReleaseNotesError(
            f"{detail_path.as_posix()} 缺少“> 发布日期：YYYY-MM-DD”"
        )

    metadata = ReleaseMetadata(
        tag=tag,
        version=version,
        release_date=date_match.group("date"),
        detail_path=detail_path,
    )
    return metadata, detail


def validate_indexes(root: Path, metadata: ReleaseMetadata) -> None:
    changelog_path = root / "CHANGELOG.md"
    if not changelog_path.is_file():
        raise ReleaseNotesError("缺少 CHANGELOG.md")
    changelog = changelog_path.read_text(encoding="utf-8")
    require_contains(
        changelog,
        f"## [{metadata.version}] - {metadata.release_date}",
        changelog_path,
    )
    require_contains(
        changelog,
        f"[{metadata.version}]: docs/upgrade/{metadata.tag}/README.md",
        changelog_path,
    )

    overview_path = root / "docs" / "upgrade" / "README.md"
    overview = overview_path.read_text(encoding="utf-8")
    require_contains(
        overview,
        f"当前代码版本为 [`{metadata.tag}`]({metadata.tag}/README.md)。",
        overview_path,
    )
    require_contains(
        overview,
        f"| [{metadata.tag}]({metadata.tag}/README.md) | {metadata.release_date} |",
        overview_path,
    )


def extract_release_notes(detail: str) -> str:
    lines = detail.splitlines()
    if not any(line == "## 版本概述" for line in lines):
        raise ReleaseNotesError("版本详情文档缺少“## 版本概述”")

    selected: list[str] = []
    skipping = False
    for line in lines:
        if line.startswith("## "):
            heading = line.removeprefix("## ").strip()
            skipping = heading in EXCLUDED_HEADINGS
        if not skipping and not line.startswith("[返回版本总览]"):
            selected.append(line)

    notes = "\n".join(selected).strip()
    if not notes or notes == "## 版本概述":
        raise ReleaseNotesError("版本详情文档没有可用于 GitHub Release 的正文")
    return f"{notes}\n"


def generate_release_notes(root: Path, tag: str) -> str:
    metadata, detail = load_metadata(root, tag)
    validate_indexes(root, metadata)
    return extract_release_notes(detail)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="发布标签，例如 v1.1.8")
    parser.add_argument(
        "--check-current",
        action="store_true",
        help="使用 Cargo.toml 当前版本执行校验",
    )
    parser.add_argument("--output", type=Path, help="将 Release Notes 写入指定文件")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="仓库根目录，默认自动识别",
    )
    args = parser.parse_args()
    if bool(args.tag) == bool(args.check_current):
        parser.error("必须且只能指定 --tag 或 --check-current")
    if args.check_current and args.output:
        parser.error("--check-current 不生成文件，不能与 --output 一起使用")
    return args


def main() -> int:
    args = parse_args()
    root = args.root.resolve()
    try:
        if args.check_current:
            tag = f"v{read_package_version(root / 'Cargo.toml')}"
        elif args.tag is not None:
            tag = args.tag
        else:
            raise ReleaseNotesError("缺少发布标签")
        notes = generate_release_notes(root, tag)
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(notes, encoding="utf-8", newline="\n")
            print(f"Release Notes 已生成：{args.output}")
        else:
            print(f"发布资料校验通过：{tag}")
        return 0
    except (OSError, ReleaseNotesError) as error:
        print(f"发布资料校验失败：{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())