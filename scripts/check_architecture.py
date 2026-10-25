#!/usr/bin/env python3
"""Enforce workspace layers from Cargo metadata, including optional/target edges."""

import json
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]

# Production and build dependencies. Integration tests may cross these layers.
# A new crate or dependency direction requires an explicit policy decision here.
LAYERS = {
    "cam-geo": set(),
    "cam-cldata": set(),
    "cam-model": {"cam-geo", "cam-cldata"},
    "cam-kernel": {"cam-model"},
    "cam-import": {"cam-geo", "cam-kernel"},
    "cam-geometry3d": {"cam-model", "cam-geo"},
    "cam-toolpath": {"cam-geo", "cam-cldata", "cam-model", "cam-geometry3d"},
    "cam-post": {"cam-cldata", "cam-model"},
    "cam-sim": {"cam-cldata", "cam-geo"},
    "cam-render": {"cam-model", "cam-geo", "cam-cldata"},
    "cam-app": {
        "cam-geo", "cam-cldata", "cam-model", "cam-toolpath", "cam-post",
        "cam-import", "cam-kernel", "cam-geometry3d", "cam-render", "cam-sim",
    },
}
UI_DEPENDENCIES = {
    "iced": {"cam-app"},
    "rfd": {"cam-app"},
    "wgpu": {"cam-app", "cam-render"},
    "bytemuck": {"cam-render"},
    "ab_glyph": {"cam-render"},
}
HEADLESS_DEFAULTS = {"cam-app", "cam-render", "cam-import", "cam-kernel"}


def violations(metadata):
    members = set(metadata["workspace_members"])
    packages = [p for p in metadata["packages"] if p["id"] in members]
    names = {p["name"] for p in packages}
    errors = []
    for package in packages:
        name = package["name"]
        if name not in LAYERS:
            errors.append(f"{name}: workspace crate has no architecture policy")
            continue
        if name in HEADLESS_DEFAULTS and package["features"].get("default", []):
            errors.append(f"{name}: default features must stay empty for headless builds")
        for dependency in package["dependencies"]:
            if dependency["kind"] == "dev":
                continue
            # Cargo's name is the package name even when the dependency is renamed.
            target = dependency["name"]
            if target in names and target not in LAYERS[name]:
                errors.append(f"{name} -> {target}: forbidden workspace dependency")
            if target in UI_DEPENDENCIES:
                if name not in UI_DEPENDENCIES[target]:
                    errors.append(f"{name} -> {target}: UI dependency outside the shell/renderer")
                elif not dependency["optional"]:
                    errors.append(f"{name} -> {target}: UI dependency must be optional")
    return errors


def main():
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked", "--offline"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        )
    except subprocess.CalledProcessError as error:
        print(error.stderr, file=sys.stderr)
        return 1
    errors = violations(json.loads(result.stdout))
    if errors:
        print("Architecture check failed:\n" + "\n".join(f"- {e}" for e in errors), file=sys.stderr)
        return 1
    print("Architecture check passed (workspace layers and optional UI dependencies).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
