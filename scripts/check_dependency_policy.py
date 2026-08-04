#!/usr/bin/env python3
"""Enforce the M0 normal/build dependency direction between Spherra crates."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from collections.abc import Iterable


def cargo_executable() -> str:
    cargo_on_path = shutil.which("cargo")
    if cargo_on_path is not None:
        return cargo_on_path

    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    cargo_from_home = cargo_home / "bin" / "cargo"
    if cargo_from_home.is_file() and os.access(cargo_from_home, os.X_OK):
        return str(cargo_from_home)

    raise SystemExit(
        "cargo executable not found on PATH or at "
        f"{cargo_from_home}; install Rust or set CARGO_HOME"
    )


def normal_or_build_dependencies(package: dict[str, object]) -> Iterable[str]:
    dependencies = package["dependencies"]
    assert isinstance(dependencies, list)
    for dependency in dependencies:
        assert isinstance(dependency, dict)
        kind = dependency["kind"]
        if kind not in (None, "normal", "build"):
            continue
        name = dependency["name"]
        assert isinstance(name, str)
        yield name


def main() -> int:
    completed = subprocess.run(
        [cargo_executable(), "metadata", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(completed.stdout)

    workspace_ids = set(metadata["workspace_members"])
    spherra_packages = {
        package["name"]
        for package in metadata["packages"]
        if package["id"] in workspace_ids and package["name"].startswith("spherra-")
    }
    ALLOWED = {
        (package, "spherra-domain")
        for package in spherra_packages
        if package != "spherra-domain"
    }
    ALLOWED |= {
        ("spherra-codec", "spherra-simd"),
        ("spherra-testkit", "spherra-codec"),
        ("spherra-testkit", "spherra-format"),
        ("spherra-bench", "spherra-codec"),
        ("spherra-bench", "spherra-format"),
        ("spherra-bench", "spherra-testkit"),
    }

    invalid_edges = []
    for package in metadata["packages"]:
        package_id = package["id"]
        package_name = package["name"]
        if package_id not in workspace_ids or package_name not in spherra_packages:
            continue
        for dependency_name in normal_or_build_dependencies(package):
            if dependency_name not in spherra_packages:
                continue
            if (package_name, dependency_name) not in ALLOWED:
                invalid_edges.append((package_name, dependency_name))

    if invalid_edges:
        for package_name, dependency_name in sorted(invalid_edges):
            print(f"invalid dependency edge: {package_name} -> {dependency_name}")
        return 1

    print("dependency policy passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
