#!/usr/bin/env python3
"""Build npm tarballs from Cargo's version and the shared target list."""

import argparse
import json
import re
import shutil
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGETS = json.loads((ROOT / "npm/targets.json").read_text())


def release_version(tag):
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    if not re.fullmatch(
        r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?", version
    ):
        raise ValueError(f"Unsupported npm release version: {version}")
    if tag and tag != f"v{version}":
        raise ValueError(
            f"Release tag {tag} does not match Cargo.toml version v{version}"
        )
    return version


def pack(directory, manifest, destination):
    (directory / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
    shutil.copy2(ROOT / "LICENSE", directory / "LICENSE")
    subprocess.run(
        ["npm", "pack", "--ignore-scripts", "--pack-destination", str(destination)],
        cwd=directory,
        check=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tag", default="", help="Release tag, which must equal v<Cargo version>"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser(
        "matrix", help="Print the release version and GitHub Actions build matrix"
    )
    package = subparsers.add_parser(
        "pack", help="Pack a native binary and its npm launcher"
    )
    package.add_argument("platform", choices=[target["platform"] for target in TARGETS])
    package.add_argument("binary", type=Path)
    args = parser.parse_args()
    version = release_version(args.tag)
    if args.command == "matrix":
        print(json.dumps({"version": version, "matrix": {"include": TARGETS}}))
        return

    binary = args.binary.resolve(strict=True)
    actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
    if actual != f"scripts {version}":
        raise ValueError(f"Binary version {actual!r} does not match scripts {version}")

    target = next(target for target in TARGETS if target["platform"] == args.platform)
    base = ROOT / "target/npm"
    destination = base / "dist"
    destination.mkdir(parents=True, exist_ok=True)
    common = {
        "version": version,
        "license": "MIT",
        "repository": {
            "type": "git",
            "url": "git+https://github.com/mbullington/scripts.git",
        },
        "homepage": "https://mbullington.github.io/scripts/",
        "publishConfig": {"access": "public"},
    }

    platform_dir = base / "packages" / args.platform
    shutil.rmtree(platform_dir, ignore_errors=True)
    (platform_dir / "bin").mkdir(parents=True)
    shutil.copy2(binary, platform_dir / "bin/scripts")
    (platform_dir / "bin/scripts").chmod(0o755)
    pack(
        platform_dir,
        {
            **common,
            "name": f"@mbullington/scripts-{args.platform}",
            "description": f"Native scripts binary for {args.platform}.",
            "os": [target["os"]],
            "cpu": [target["cpu"]],
            "files": ["bin/scripts"],
            "preferUnplugged": True,
        },
        destination,
    )

    launcher_dir = base / "packages/scripts"
    shutil.rmtree(launcher_dir, ignore_errors=True)
    launcher_dir.mkdir(parents=True)
    shutil.copy2(ROOT / "npm/scripts.cjs", launcher_dir / "scripts.cjs")
    (launcher_dir / "scripts.cjs").chmod(0o755)
    shutil.copy2(ROOT / "npm/targets.json", launcher_dir / "targets.json")
    shutil.copy2(ROOT / "README.md", launcher_dir / "README.md")
    pack(
        launcher_dir,
        {
            **common,
            "name": "@mbullington/scripts",
            "description": "Parallel monorepo task runner with dependency graphs and content-aware caching.",
            "bin": {"scripts": "scripts.cjs"},
            "engines": {"node": "^22.15.0 || >=23.11.0"},
            "files": ["scripts.cjs", "targets.json"],
            "optionalDependencies": {
                f"@mbullington/scripts-{item['platform']}": version for item in TARGETS
            },
        },
        destination,
    )


if __name__ == "__main__":
    main()
