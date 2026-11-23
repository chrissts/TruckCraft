#!/usr/bin/env python3
"""Render release notes for the current Cargo package version."""

from pathlib import Path
from string import Template
import sys
import tomllib


release_dir = Path(__file__).resolve().parent
with (release_dir.parent / "Cargo.toml").open("rb") as manifest:
    package = tomllib.load(manifest)["workspace"]["package"]
version = package["version"]

changes_file = release_dir / "notes" / f"{version}.md"
if not changes_file.is_file():
    sys.exit(f"Add release/notes/{version}.md before drafting this release.")
changes = changes_file.read_text(encoding="utf-8").strip()
if not changes:
    sys.exit(f"Add the user-facing changes to release/notes/{version}.md.")

template = Template((release_dir / "notes-template.md").read_text(encoding="utf-8"))
sys.stdout.write(
    template.substitute(version=version.removesuffix(".0"), changes=changes, repository=package["repository"])
)
