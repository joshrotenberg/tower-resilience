#!/usr/bin/env python3
"""Build and inspect every publishable workspace crate without publishing it."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tarfile
import tomllib
from typing import Any, Iterable


class PreflightError(RuntimeError):
    """A release preflight check failed."""


def run(command: list[str], root: Path) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    environment["CARGO_TERM_COLOR"] = "never"
    result = subprocess.run(
        command,
        cwd=root,
        env=environment,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        output = "\n".join(part for part in (result.stdout, result.stderr) if part)
        raise PreflightError(f"command failed: {' '.join(command)}\n{output}".rstrip())
    return result


def load_metadata(root: Path) -> dict[str, Any]:
    result = run(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], root)
    return json.loads(result.stdout)


def workspace_packages(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    member_ids = set(metadata["workspace_members"])
    return [package for package in metadata["packages"] if package["id"] in member_ids]


def publishable_packages(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    # Cargo represents `publish = false` as an empty registry allow-list.
    return [package for package in workspace_packages(metadata) if package.get("publish") != []]


def skipped_packages(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    return [package for package in workspace_packages(metadata) if package.get("publish") == []]


def _path_from_manifest(package: dict[str, Any], value: str) -> Path:
    return (Path(package["manifest_path"]).parent / value).resolve()


def validate_package_metadata(packages: list[dict[str, Any]]) -> list[str]:
    errors: list[str] = []
    for package in packages:
        name = package["name"]
        manifest_directory = Path(package["manifest_path"]).parent

        readme = package.get("readme")
        if not readme:
            errors.append(f"{name}: package.readme is missing")
        else:
            readme_path = _path_from_manifest(package, readme)
            if not readme_path.is_file() or readme_path.stat().st_size == 0:
                errors.append(f"{name}: README is missing or empty: {readme_path}")

        license_expression = package.get("license")
        license_file = package.get("license_file")
        if not license_expression and not license_file:
            errors.append(f"{name}: package license metadata is missing")
        elif license_file:
            license_path = _path_from_manifest(package, license_file)
            if not license_path.is_file() or license_path.stat().st_size == 0:
                errors.append(f"{name}: license file is missing or empty: {license_path}")

        changelog = manifest_directory / "CHANGELOG.md"
        if not changelog.is_file() or changelog.stat().st_size == 0:
            errors.append(f"{name}: CHANGELOG.md is missing or empty")

        targets = package.get("targets", [])
        if not targets:
            errors.append(f"{name}: package has no Cargo targets")
        for target in targets:
            source = Path(target["src_path"])
            if not source.is_file():
                errors.append(f"{name}: target source is missing: {source}")
    return errors


def _is_exact_internal_requirement(requirement: str, version: str) -> bool:
    return requirement.strip() in {version, f"^{version}", f"={version}"}


def validate_dependencies(packages: list[dict[str, Any]]) -> list[str]:
    errors: list[str] = []
    internal_versions = {package["name"]: package["version"] for package in packages}

    for package in packages:
        for dependency in package.get("dependencies", []):
            dependency_name = dependency["name"]
            source = dependency.get("source") or ""
            path = dependency.get("path")
            requirement = dependency.get("req") or ""
            kind = dependency.get("kind") or "normal"
            label = f"{package['name']}: {kind} dependency {dependency_name}"

            if source.startswith("git+"):
                errors.append(f"{label} uses a forbidden git source")

            if path and (not requirement or requirement.strip() == "*"):
                errors.append(f"{label} is a forbidden path-only dependency")

            if dependency_name in internal_versions:
                expected = internal_versions[dependency_name]
                if not path:
                    errors.append(f"{label} must retain its workspace path for local validation")
                if not _is_exact_internal_requirement(requirement, expected):
                    errors.append(
                        f"{label} requires {requirement!r}; expected the "
                        f"synchronized version {expected}"
                    )
    return errors


def publication_order(packages: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_name = {package["name"]: package for package in packages}
    dependencies: dict[str, set[str]] = {name: set() for name in by_name}

    for package in packages:
        for dependency in package.get("dependencies", []):
            if (dependency.get("kind") or "normal") == "dev":
                continue
            if dependency["name"] in by_name:
                dependencies[package["name"]].add(dependency["name"])

    order: list[dict[str, Any]] = []
    remaining = {name: set(required) for name, required in dependencies.items()}
    while remaining:
        ready = sorted(name for name, required in remaining.items() if not required)
        if not ready:
            cycle = ", ".join(sorted(remaining))
            raise PreflightError(f"publishable workspace dependency cycle: {cycle}")
        for name in ready:
            order.append(by_name[name])
            del remaining[name]
        for required in remaining.values():
            required.difference_update(ready)

    facade = next((package for package in order if package["name"] == "tower-resilience"), None)
    if facade and order[-1] is not facade:
        raise PreflightError("tower-resilience facade is not last in publication order")
    return order


def package_file_lists(root: Path, packages: list[dict[str, Any]]) -> dict[str, set[str]]:
    required_files = {"Cargo.toml", "Cargo.toml.orig", "Cargo.lock", "CHANGELOG.md", "README.md"}
    file_lists: dict[str, set[str]] = {}

    for package in packages:
        result = run(
            [
                "cargo",
                "package",
                "--locked",
                "--allow-dirty",
                "--list",
                "-p",
                package["name"],
            ],
            root,
        )
        files = {line.strip() for line in result.stdout.splitlines() if line.strip()}
        missing = sorted(required_files - files)
        if missing:
            raise PreflightError(
                f"{package['name']}: package file list is missing {', '.join(missing)}"
            )
        if not any(path.startswith("src/") for path in files):
            raise PreflightError(f"{package['name']}: package file list has no src/ content")
        file_lists[package["name"]] = files
    return file_lists


def _file_hash(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot_manifests(root: Path, packages: list[dict[str, Any]]) -> dict[Path, str]:
    paths = {root / "Cargo.toml", root / "Cargo.lock"}
    paths.update(Path(package["manifest_path"]) for package in packages)
    return {path: _file_hash(path) for path in sorted(paths)}


def verify_snapshot(snapshot: dict[Path, str]) -> None:
    changed = [
        str(path)
        for path, digest in snapshot.items()
        if not path.is_file() or _file_hash(path) != digest
    ]
    if changed:
        raise PreflightError(
            "packaging changed source manifests or lockfiles: " + ", ".join(changed)
        )


def _dependency_specs(document: dict[str, Any]) -> Iterable[tuple[str, dict[str, Any]]]:
    dependency_sections = {"dependencies", "dev-dependencies", "build-dependencies"}
    for key, value in document.items():
        if key in dependency_sections and isinstance(value, dict):
            for name, specification in value.items():
                if isinstance(specification, dict):
                    yield name, specification
        elif isinstance(value, dict):
            yield from _dependency_specs(value)


def inspect_archives(
    metadata: dict[str, Any],
    packages: list[dict[str, Any]],
    expected_files: dict[str, set[str]],
) -> None:
    target_directory = Path(metadata["target_directory"])
    for package in packages:
        name = package["name"]
        version = package["version"]
        archive = target_directory / "package" / f"{name}-{version}.crate"
        if not archive.is_file() or archive.stat().st_size == 0:
            raise PreflightError(f"{name}: expected archive was not produced: {archive}")

        prefix = PurePosixPath(f"{name}-{version}")
        with tarfile.open(archive, mode="r:gz") as crate:
            archive_files: set[str] = set()
            normalized_manifest: bytes | None = None
            for member in crate.getmembers():
                path = PurePosixPath(member.name)
                if not member.isfile() or not path.is_relative_to(prefix):
                    continue
                relative = str(path.relative_to(prefix))
                archive_files.add(relative)
                if relative == "Cargo.toml":
                    extracted = crate.extractfile(member)
                    normalized_manifest = extracted.read() if extracted else None

        if archive_files != expected_files[name]:
            missing = sorted(expected_files[name] - archive_files)
            extra = sorted(archive_files - expected_files[name])
            raise PreflightError(
                f"{name}: archive differs from cargo package --list; "
                f"missing={missing}, extra={extra}"
            )
        if normalized_manifest is None:
            raise PreflightError(f"{name}: archive has no normalized Cargo.toml")

        document = tomllib.loads(normalized_manifest.decode("utf-8"))
        if document.get("package", {}).get("name") != name:
            raise PreflightError(f"{name}: normalized manifest has the wrong package name")
        if document.get("package", {}).get("version") != version:
            raise PreflightError(f"{name}: normalized manifest has the wrong package version")
        for dependency_name, specification in _dependency_specs(document):
            if "path" in specification or "git" in specification:
                raise PreflightError(
                    f"{name}: normalized dependency {dependency_name} retained a path or git source"
                )


def package_archives(root: Path, packages: list[dict[str, Any]]) -> None:
    command = ["cargo", "package", "--locked", "--allow-dirty", "--no-verify"]
    for package in packages:
        command.extend(["-p", package["name"]])
    result = run(command, root)
    if result.stderr:
        print(result.stderr.rstrip())


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    try:
        metadata = load_metadata(root)
        packages = publishable_packages(metadata)
        skipped = skipped_packages(metadata)
        if not packages:
            raise PreflightError("workspace has no publishable packages")

        errors = validate_package_metadata(packages) + validate_dependencies(packages)
        if errors:
            raise PreflightError("metadata validation failed:\n- " + "\n- ".join(errors))

        order = publication_order(packages)
        print(f"Publishable packages ({len(order)}), in dependency order:")
        for position, package in enumerate(order, start=1):
            print(f"  {position:2}. {package['name']} {package['version']}")
        skipped_names = sorted(package["name"] for package in skipped)
        print("Skipped publish = false packages: " + ", ".join(skipped_names))

        snapshot = snapshot_manifests(root, packages)
        file_lists = package_file_lists(root, order)
        package_archives(root, order)
        inspect_archives(metadata, order, file_lists)
        verify_snapshot(snapshot)
    except (OSError, json.JSONDecodeError, tomllib.TOMLDecodeError, PreflightError) as error:
        print(f"publish preflight failed: {error}", file=sys.stderr)
        return 1

    print(
        f"Publish preflight passed: inspected {len(order)} package file lists "
        "and archives; nothing was published."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
