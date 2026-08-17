#!/usr/bin/env python3
"""从发布构建产物生成 TinyShell 在线更新清单。"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from urllib.parse import quote

TAG_PATTERN = re.compile(r"^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")
SCHEMA_VERSION = 1
DEFAULT_REPOSITORY = "ynx-official/tiny-shell"
RELEASE_NOTES_ASSET = "release-notes.md"


class UpdateManifestError(ValueError):
    """发布产物无法形成安全且完整的更新清单。"""


def expected_asset_names(tag: str) -> set[str]:
    return {
        f"tiny-shell-{tag}-linux-x86_64.AppImage",
        f"tiny-shell-{tag}-linux-x86_64.tar.gz",
        f"tiny-shell-{tag}-macos-aarch64-portable.zip",
        f"tiny-shell-{tag}-macos-aarch64-setup.pkg",
        f"tiny-shell-{tag}-macos-x86_64-portable.zip",
        f"tiny-shell-{tag}-macos-x86_64-setup.pkg",
        f"tiny-shell-{tag}-windows-x86_64-portable.zip",
        f"tiny-shell-{tag}-windows-x86_64-setup.exe",
    }


def sha256_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as asset:
        for chunk in iter(lambda: asset.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def collect_assets(dist: Path, tag: str, repository: str) -> list[dict[str, object]]:
    if not TAG_PATTERN.fullmatch(tag):
        raise UpdateManifestError(f"无效发布标签 {tag!r}")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise UpdateManifestError(f"无效 GitHub 仓库标识 {repository!r}")
    if not dist.is_dir():
        raise UpdateManifestError(f"发布产物目录不存在：{dist.as_posix()}")

    expected = expected_asset_names(tag)
    files_by_name: dict[str, Path] = {}
    for path in sorted(candidate for candidate in dist.rglob("*") if candidate.is_file()):
        if path.name in {"update-manifest.json", RELEASE_NOTES_ASSET}:
            continue
        if path.name in files_by_name:
            raise UpdateManifestError(f"存在重名发布产物：{path.name}")
        files_by_name[path.name] = path

    actual = set(files_by_name)
    missing = sorted(expected - actual)
    unexpected = sorted(actual - expected)
    if missing or unexpected:
        details = []
        if missing:
            details.append(f"缺少：{', '.join(missing)}")
        if unexpected:
            details.append(f"多余：{', '.join(unexpected)}")
        raise UpdateManifestError("发布产物集合不完整（" + "；".join(details) + "）")

    return [
        {
            "name": name,
            "url": (
                f"https://github.com/{repository}/releases/download/"
                f"{quote(tag, safe='')}/{quote(name, safe='')}"
            ),
            "size": files_by_name[name].stat().st_size,
            "digest": sha256_digest(files_by_name[name]),
        }
        for name in sorted(expected)
    ]


def build_manifest(dist: Path, tag: str, repository: str) -> dict[str, object]:
    assets = collect_assets(dist, tag, repository)
    return {
        "schema_version": SCHEMA_VERSION,
        "version": tag,
        "notes_url": (
            f"https://github.com/{repository}/releases/download/"
            f"{quote(tag, safe='')}/{RELEASE_NOTES_ASSET}"
        ),
        "assets": assets,
    }


def write_manifest(
    dist: Path, tag: str, repository: str, output: Path
) -> dict[str, object]:
    manifest = build_manifest(dist, tag, repository)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    return manifest


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, required=True, help="构建产物根目录")
    parser.add_argument("--tag", required=True, help="发布标签，例如 v1.1.9")
    parser.add_argument(
        "--repository",
        default=DEFAULT_REPOSITORY,
        help="GitHub 仓库 owner/name",
    )
    parser.add_argument("--output", type=Path, required=True, help="清单输出路径")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        manifest = write_manifest(
            args.dist.resolve(), args.tag, args.repository, args.output.resolve()
        )
        print(f"更新清单已生成：{args.output}（{len(manifest['assets'])} 个产物）")
        return 0
    except (OSError, UpdateManifestError) as error:
        print(f"更新清单生成失败：{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
