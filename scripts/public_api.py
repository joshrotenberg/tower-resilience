#!/usr/bin/env python3
"""Review and snapshot the public API of every publishable workspace library."""

from __future__ import annotations

import argparse
import difflib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[1]
SNAPSHOT_DIR = ROOT / "docs" / "public-api"
NIGHTLY = "nightly-2026-08-01"
PUBLIC_API_VERSION = "0.52.0"


def publishable_libraries(metadata: dict[str, Any]) -> list[str]:
    """Return sorted names of publishable workspace packages with lib targets."""
    workspace_members = set(metadata["workspace_members"])
    packages = []
    for package in metadata["packages"]:
        if package["id"] not in workspace_members or package.get("publish") == []:
            continue
        if any("lib" in target["kind"] for target in package["targets"]):
            packages.append(package["name"])
    return sorted(packages)


def normalized_api(output: str) -> str:
    """Normalize tool output for stable text snapshots."""
    return output.rstrip() + "\n"


def snapshot_diff(package: str, expected: str, actual: str) -> str:
    """Return a unified diff for one package snapshot."""
    return "".join(
        difflib.unified_diff(
            expected.splitlines(keepends=True),
            actual.splitlines(keepends=True),
            fromfile=f"docs/public-api/{package}.txt",
            tofile=f"generated:{package}",
        )
    )


def run(command: list[str], *, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        if result.stdout:
            print(result.stdout, file=sys.stderr, end="")
        if result.stderr:
            print(result.stderr, file=sys.stderr, end="")
        raise RuntimeError(f"command failed ({result.returncode}): {' '.join(command)}")
    return result.stdout


def metadata() -> dict[str, Any]:
    return json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"]))


def tool_environment() -> dict[str, str]:
    version = run(["cargo", "public-api", "--version"]).strip()
    if version != f"cargo-public-api {PUBLIC_API_VERSION}":
        raise RuntimeError(
            f"cargo-public-api {PUBLIC_API_VERSION} is required; found {version!r}. "
            f"Install it with: cargo install cargo-public-api --locked --version {PUBLIC_API_VERSION}"
        )

    rustc = run(["rustup", "which", "--toolchain", NIGHTLY, "rustc"]).strip()
    rustdoc = run(["rustup", "which", "--toolchain", NIGHTLY, "rustdoc"]).strip()
    env = os.environ.copy()
    env.update({"RUSTC": rustc, "RUSTDOC": rustdoc})
    return env


def current_api(package: str, env: dict[str, str]) -> str:
    api = normalized_api(
        run(
            [
                "cargo",
                "public-api",
                "--package",
                package,
                "--all-features",
                "-s",
                "--color",
                "never",
            ],
            env=env,
        )
    )
    if not api.strip():
        raise RuntimeError(f"cargo-public-api returned an empty API for {package}")
    return api


def update(packages: Iterable[str], env: dict[str, str]) -> int:
    SNAPSHOT_DIR.mkdir(parents=True, exist_ok=True)
    for package in packages:
        print(f"snapshotting {package}", flush=True)
        (SNAPSHOT_DIR / f"{package}.txt").write_text(
            current_api(package, env), encoding="utf-8"
        )
    return 0


def check(packages: Iterable[str], env: dict[str, str]) -> int:
    failures = 0
    for package in packages:
        print(f"checking {package}", flush=True)
        path = SNAPSHOT_DIR / f"{package}.txt"
        actual = current_api(package, env)
        if not path.exists():
            print(f"missing snapshot: {path.relative_to(ROOT)}", file=sys.stderr)
            failures += 1
            continue
        expected = path.read_text(encoding="utf-8")
        if expected != actual:
            print(snapshot_diff(package, expected, actual), file=sys.stderr, end="")
            failures += 1

    expected_names = {f"{package}.txt" for package in packages}
    if SNAPSHOT_DIR.exists():
        for stale in sorted(SNAPSHOT_DIR.glob("*.txt")):
            if stale.name not in expected_names:
                print(f"stale snapshot: {stale.relative_to(ROOT)}", file=sys.stderr)
                failures += 1

    if failures:
        print(
            "Public API snapshots differ. Review the changes, update migration/release notes, "
            "then run `python3 scripts/public_api.py update` to acknowledge them.",
            file=sys.stderr,
        )
        return 1
    print("Public API snapshots are current.")
    return 0


def published_diff(packages: Iterable[str], baseline: str, env: dict[str, str]) -> int:
    for package in packages:
        print(f"\n## {package} ({baseline} -> working tree)", flush=True)
        output = run(
            [
                "cargo",
                "public-api",
                "--package",
                package,
                "--all-features",
                "-s",
                "--color",
                "never",
                "diff",
                baseline,
            ],
            env=env,
        )
        print(output.rstrip() or "(no public API changes)")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("check", help="fail if checked-in snapshots differ")
    subparsers.add_parser("update", help="regenerate checked-in snapshots")
    diff_parser = subparsers.add_parser("diff", help="compare every crate to a published version")
    diff_parser.add_argument("baseline", help="published version, for example 0.12.0")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        packages = publishable_libraries(metadata())
        if not packages:
            raise ValueError("no publishable workspace libraries found")
        env = tool_environment()
        if args.command == "check":
            return check(packages, env)
        if args.command == "update":
            return update(packages, env)
        return published_diff(packages, args.baseline, env)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"public API review failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
