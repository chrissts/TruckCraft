#!/usr/bin/env python3
"""Package a locked TruckCraft build without bundled SCS art."""

import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import platform
import plistlib
import re
import shutil
import struct
import shlex
import subprocess
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]
VERSION = PACKAGE["version"]
DISPLAY_VERSION = VERSION.removesuffix(".0")
APP_ID = "io.github.chrissts.TruckCraft"
PROJECT_URL = PACKAGE["repository"]
DEVELOPER = "TruckCraft contributors"
WORKSPACE_CRATES = {"truckcraft", "ts-save-core", "scs-codec"}
LICENSE_PREFIXES = ("license", "licence", "copying", "copyright", "notice", "unlicense", "third-party", "third_party", "ofl")


def run(*command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def copy_notices(destination, registry, tree, mingw=False):
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "LICENSE", destination)
    shutil.copy2(ROOT / "NOTICE", destination)
    shutil.copy2(ROOT / "README.md", destination)
    docs = destination / "docs"
    docs.mkdir()
    shutil.copy2(ROOT / "docs/company-editor.png", docs)
    licenses = destination / "licenses"
    shutil.copytree(ROOT / "crates/scs-codec/licenses", licenses / "scs-codec", dirs_exist_ok=True)
    shutil.copy2(ROOT / "crates/scs-codec/LICENSE", licenses / "scs-codec/LICENSE")
    shutil.copy2(ROOT / "crates/scs-codec/UPSTREAM-NOTICES.md", licenses / "scs-codec")
    shutil.copy2(ROOT / "crates/desktop/assets/ARTWORK.md", licenses)
    bundled_notices = ROOT / "release/notices"
    runtime = licenses / "toolchain-runtime"
    runtime.mkdir()
    shutil.copy2(bundled_notices / "toolchain-runtime/COPYRIGHT-library.html", runtime)
    if mingw:
        shutil.copy2(bundled_notices / "toolchain-runtime/COPYING.MinGW-w64-runtime.txt", runtime)

    entries = re.findall(r"^([\w-]+) v([^\s]+)", tree.read_text(encoding="utf-8-sig"), re.M)
    dependencies = sorted(set(entries) - {entry for entry in entries if entry[0] in WORKSPACE_CRATES})
    if not dependencies:
        raise ValueError("The Cargo dependency list is empty.")
    summary = []
    for name, version in dependencies:
        crate = f"{name}-{version}"
        matches = list(registry.glob(f"*/{crate}"))
        if len(matches) != 1:
            raise ValueError(f"Expected one Cargo registry source for {crate}, found {len(matches)}.")
        crate_source = matches[0]
        manifest = tomllib.loads((crate_source / "Cargo.toml").read_text(encoding="utf-8"))["package"]
        declared = manifest.get("license", manifest.get("license-file", "See bundled notices"))
        summary.append(f"{crate}\t{declared}")
        fallback = bundled_notices / crate
        if fallback.is_dir():
            shutil.copytree(fallback, licenses / crate, dirs_exist_ok=True)
        if name == "epaint_default_fonts":
            fonts = licenses / crate / "fonts"
            fonts.mkdir(parents=True, exist_ok=True)
            for path in sorted((crate_source / "fonts").glob("*.txt")):
                shutil.copy2(path, fonts)
        for path in sorted(crate_source.iterdir()):
            if path.name.lower().startswith(LICENSE_PREFIXES):
                if path.is_dir():
                    shutil.copytree(path, licenses / crate / path.name)
                elif path.is_file():
                    (licenses / crate).mkdir(exist_ok=True)
                    shutil.copy2(path, licenses / crate)
        if license_file := manifest.get("license-file"):
            path = crate_source / license_file
            if path.is_file() and path.resolve().is_relative_to(crate_source.resolve()):
                (licenses / crate).mkdir(exist_ok=True)
                shutil.copy2(path, licenses / crate / path.name)
    (licenses / "DEPENDENCIES.txt").write_text("\n".join(summary) + "\n", encoding="utf-8")
    for path in destination.rglob("*"):
        if path.is_file():
            path.chmod(0o644)


def source_files():
    known_release_files = ["release/package.py", "release/build-unix.sh", "release/README.md"]
    tracked = run("git", "ls-files", "--cached", "-z", cwd=ROOT, capture_output=True).stdout
    paths = set(os.fsdecode(tracked).split("\0")) - {""}
    paths.update(known_release_files)
    paths.update(("release/build-windows.ps1", ".github/workflows/package-check.yml",
                  "crates/desktop/src/app/review.rs", "crates/desktop/src/app/updates.rs",
                  "crates/desktop/src/app/package_check.rs"))
    paths.update(["docs/company-editor.png", ".github/workflows/desktop-tests.yml"])
    paths.update(
        file.relative_to(ROOT).as_posix()
        for file in (ROOT / "release/notices").rglob("*")
        if file.is_file()
    )
    for entry in sorted(paths):
        relative = PurePosixPath(entry)
        if relative.parts[0] in {".git", "target", "dist", "work"}:
            raise ValueError(f"Local files cannot be packaged: {relative}")
        if relative.is_relative_to("crates/desktop/assets/game") or entry in {
            "crates/desktop/assets/ats-logo.png",
            "crates/desktop/assets/ets2-logo.png",
        }:
            continue
        file = ROOT / relative
        if file.is_file() and not file.is_symlink() and file.resolve().is_relative_to(ROOT):
            yield file, entry


def zip_files(destination, files):
    with zipfile.ZipFile(
        destination, "w", zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False
    ) as archive:
        for path, name in files:
            entry = zipfile.ZipInfo(name)
            entry.create_system = 3
            mode = 0o755 if name.endswith((".exe", ".sh")) else 0o644
            entry.external_attr = (0o100000 | mode) << 16
            archive.writestr(entry, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    with zipfile.ZipFile(destination) as archive:
        if bad := archive.testzip():
            raise ValueError(f"Invalid ZIP entry: {bad}")


def install_linux_files(root, binary, registry, tree):
    paths = {
        "usr/bin/truckcraft": binary,
        f"usr/share/icons/hicolor/512x512/apps/{APP_ID}.png": ROOT / "crates/desktop/assets/app-mark-v2.png",
    }
    for name, source in paths.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, path)
        path.chmod(0o755 if name.endswith("/truckcraft") else 0o644)
    desktop = root / f"usr/share/applications/{APP_ID}.desktop"
    desktop.parent.mkdir(parents=True, exist_ok=True)
    desktop.write_text(
        f"[Desktop Entry]\nType=Application\nName=TruckCraft\n"
        f"Comment=Edit Euro Truck Simulator 2 and American Truck Simulator saves\n"
        f"Exec=truckcraft\nIcon={APP_ID}\nTerminal=false\nCategories=Game;\n",
        encoding="utf-8",
    )
    run("desktop-file-validate", str(desktop))
    metainfo = root / f"usr/share/metainfo/{APP_ID}.metainfo.xml"
    metainfo.parent.mkdir(parents=True, exist_ok=True)
    metainfo.write_text(
        f"""<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{APP_ID}</id>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>Apache-2.0</project_license>
  <name>TruckCraft</name>
  <summary>Save editor for ETS2 and ATS</summary>
  <description>
    <p>Edit Euro Truck Simulator 2 and American Truck Simulator profiles and
    saves with undo, redo and recovery copies.</p>
  </description>
  <developer id="io.github.chrissts">
    <name>{DEVELOPER}</name>
  </developer>
  <url type="homepage">{PROJECT_URL}</url>
  <launchable type="desktop-id">{APP_ID}.desktop</launchable>
  <content_rating type="oars-1.1"/>
</component>
""",
        encoding="utf-8",
    )
    copy_notices(root / "usr/share/doc/truckcraft", registry, tree)


def ubuntu_package(work, output):
    root = work / "root"
    control = work / "debian/control"
    control.parent.mkdir()
    control.write_text("Source: truckcraft\n\nPackage: truckcraft\nArchitecture: any\n", encoding="ascii")
    dependencies = run(
        "dpkg-shlibdeps", "-O", f"-e{root / 'usr/bin/truckcraft'}",
        cwd=work, capture_output=True, text=True,
    ).stdout.strip().removeprefix("shlibs:Depends=")
    if not dependencies:
        raise ValueError("dpkg-shlibdeps did not return runtime dependencies.")
    architecture = run("dpkg", "--print-architecture", capture_output=True, text=True).stdout.strip()
    control = root / "DEBIAN/control"
    control.parent.mkdir()
    control.write_text(
        f"Package: truckcraft\nVersion: {VERSION}\nArchitecture: {architecture}\n"
        f"Maintainer: {DEVELOPER}\nSection: games\nPriority: optional\n"
        f"Depends: {dependencies}, curl, libegl1, libgl1, libx11-6, libxkbcommon0, "
        f"libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0\n"
        f"Homepage: {PROJECT_URL}\n"
        f"Description: Save editor for ETS2 and ATS\n"
        f" Edit truck simulator profiles and saves with undo, redo and recovery copies.\n",
        encoding="ascii",
    )
    ubuntu = platform.freedesktop_os_release()["VERSION_ID"]
    artifact = work / f"TruckCraft-Ubuntu{ubuntu}-{architecture}.deb"
    run("dpkg-deb", "--root-owner-group", "--build", str(root), str(artifact))
    run("dpkg-deb", "--info", str(artifact), stdout=subprocess.DEVNULL)
    shutil.copyfile(artifact, output / artifact.name)


def fedora_package(work, output):
    spec = work / "truckcraft.spec"
    spec.write_text(
        f"""Name: truckcraft
Version: {VERSION}
Release: 1%{{?dist}}
Summary: Save editor for ETS2 and ATS
License: Apache-2.0
URL: {PROJECT_URL}
Vendor: {DEVELOPER}
Packager: {DEVELOPER} ({PROJECT_URL})
Requires: curl, gtk3, libX11, libxkbcommon, libxkbcommon-x11, libwayland-client, libwayland-cursor, libglvnd-egl, libglvnd-glx

%global debug_package %{{nil}}
%description
Edit truck simulator profiles and saves with undo, redo and recovery copies.

%install
mkdir -p "%{{buildroot}}"
cp -a "{work / 'root'}/." "%{{buildroot}}/"

%files
%{{_bindir}}/truckcraft
%{{_datadir}}/applications/{APP_ID}.desktop
%{{_datadir}}/metainfo/{APP_ID}.metainfo.xml
%{{_datadir}}/icons/hicolor/512x512/apps/{APP_ID}.png
%dir %{{_docdir}}/truckcraft
%doc %{{_docdir}}/truckcraft/README.md
%doc %{{_docdir}}/truckcraft/docs
%license %{{_docdir}}/truckcraft/LICENSE
%license %{{_docdir}}/truckcraft/NOTICE
%license %{{_docdir}}/truckcraft/licenses
""",
        encoding="utf-8",
    )
    run("rpmbuild", "-bb", "--define", f"_topdir {work}",
        "--define", "source_date_epoch_from_changelog 0", str(spec))
    (artifact,) = (work / "RPMS").glob("*/*.rpm")
    run("rpm", "-qp", str(artifact), stdout=subprocess.DEVNULL)
    architecture = artifact.parent.name
    fedora = platform.freedesktop_os_release()["VERSION_ID"]
    shutil.copyfile(artifact, output / f"TruckCraft-Fedora{fedora}-{architecture}.rpm")


def arch_package(work, output):
    pkgbuild = work / "PKGBUILD"
    pkgbuild.write_text(
        f"""pkgname=truckcraft
pkgver={VERSION}
pkgrel=1
pkgdesc='Native save editor for Euro Truck Simulator 2 and American Truck Simulator'
arch=('x86_64')
url={shlex.quote(PROJECT_URL)}
license=('Apache-2.0')
depends=('glibc' 'gcc-libs' 'gtk3' 'libglvnd' 'libx11' 'libxkbcommon' 'libxkbcommon-x11' 'wayland' 'curl')
options=('!debug' '!strip')

package() {{
    cp -a "$startdir/root/." "$pkgdir/"
    install -Dm644 "$pkgdir/usr/share/doc/truckcraft/LICENSE" "$pkgdir/usr/share/licenses/truckcraft/LICENSE"
}}
""",
        encoding="ascii",
    )
    env = {**os.environ, "PKGDEST": str(work), "PKGEXT": ".pkg.tar.zst"}
    run("makepkg", "--noconfirm", cwd=work, env=env)
    (artifact,) = work.glob("truckcraft-*.pkg.tar.zst")
    run("pacman", "-Qip", str(artifact), stdout=subprocess.DEVNULL)
    shutil.copyfile(artifact, output / "TruckCraft.pkg.tar.zst")


def macos_package(work, output, binary, registry, tree):
    app = work / "TruckCraft.app"
    contents = app / "Contents"
    executable = contents / "MacOS/truckcraft"
    executable.parent.mkdir(parents=True)
    shutil.copyfile(binary, executable)
    executable.chmod(0o755)
    resources = contents / "Resources"
    copy_notices(resources, registry, tree)
    icon = work / "icon.png"
    run("sips", "-z", "256", "256", str(ROOT / "docs/brand/truckcraft-icon.png"), "--out", str(icon))
    png = icon.read_bytes()
    (resources / "TruckCraft.icns").write_bytes(
        struct.pack(">4sI4sI", b"icns", len(png) + 16, b"ic08", len(png) + 8) + png
    )
    plist = {
        "CFBundleIdentifier": APP_ID,
        "CFBundleName": "TruckCraft",
        "CFBundleDisplayName": "TruckCraft",
        "CFBundleExecutable": "truckcraft",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": DISPLAY_VERSION,
        "CFBundleVersion": VERSION,
        "CFBundleIconFile": "TruckCraft.icns",
        "LSMinimumSystemVersion": "12.0",
        "NSHighResolutionCapable": True,
    }
    (contents / "Info.plist").write_bytes(plistlib.dumps(plist))
    run("plutil", "-lint", str(contents / "Info.plist"))
    run("lipo", str(executable), "-verify_arch", "arm64", "x86_64")
    run("codesign", "--force", "--sign", "-", str(app))
    run("codesign", "--verify", "--strict", str(app))
    artifact = work / f"TruckCraft-{DISPLAY_VERSION}-macOS-universal.zip"
    run("ditto", "-c", "-k", "--keepParent", str(app), str(artifact))
    shutil.copyfile(artifact, output / artifact.name)


def check_zip(archive, expected_binary=None):
    private_names = {"game.sii", "profile.sii", "info.sii", "settings.json", "controls.sii", "config.cfg"}
    machine_path = re.compile(rb"(?i)(?:[a-z]:[/\\]users[/\\][^/\\\r\n]{1,80}[/\\]|/mnt/[a-z]/Users/[^/\r\n]+/)")
    with zipfile.ZipFile(archive) as payload:
        for member in payload.infolist():
            path = PurePosixPath(member.filename)
            if (path.name.lower() in private_names or path.suffix.lower() in {".pdb", ".log"}
                or path.is_relative_to("crates/desktop/assets/game")
                or path.name in {"ets2-logo.png", "ats-logo.png"}):
                raise ValueError(f"Unexpected private file in {archive.name}: {path}")
            data = payload.read(member)
            if machine_path.search(data.replace(b"\0", b"")):
                raise ValueError(f"Machine path in {archive.name}: {path}")
            if path.name == "truckcraft.exe" and expected_binary != hashlib.sha256(data).digest():
                raise ValueError("Windows ZIP executable differs from the build output")


def check_target_path(binary):
    target = os.environ.get("CARGO_TARGET_DIR")
    if target:
        path = Path(target).resolve().as_posix().rstrip("/") + "/"
        if path == "/target/":
            return
        data = binary.read_bytes().replace(b"\0", b"").replace(b"\\", b"/")
        if os.fsencode(path).lower() in data.lower():
            raise ValueError("The packaged executable contains the local Cargo build path.")


def windows_package(work, args):
    if any([args.binary, args.registry, args.dependency_tree]):
        raise ValueError("The Windows packager builds the executable and dependency list itself.")
    registry = (args.toolchain_directory / "cargo" if args.toolchain_directory else
                Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))) / "registry/src"
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    tree = work / "dependencies.txt"
    binary = work / "truckcraft.exe"
    command = [shutil.which("pwsh") or "powershell.exe", "-NoProfile", "-File", str(ROOT / "release/build-windows.ps1"),
               "-DependencyTree", str(tree), "-BinaryOutput", str(binary)]
    if args.toolchain_directory:
        command.extend(["-ToolchainDirectory", str(args.toolchain_directory.resolve())])
    run(*command, cwd=ROOT, env={**os.environ, "CARGO_TARGET_DIR": str(target), "TRUCKCRAFT_BUNDLE_LOCAL_ART": "0"})
    check_target_path(binary)
    root = work / "app"
    copy_notices(root, registry, tree, mingw=True)
    shutil.copyfile(binary, root / "truckcraft.exe")
    digest = hashlib.sha256((root / "truckcraft.exe").read_bytes()).digest()
    artifact = work / f"TruckCraft-{DISPLAY_VERSION}-Windows-x64.zip"
    zip_files(artifact, ((p, p.relative_to(root).as_posix()) for p in sorted(root.rglob("*")) if p.is_file()))
    check_zip(artifact, digest)
    artifacts = [artifact]
    if args.source:
        source = work / f"TruckCraft-{DISPLAY_VERSION}-source.zip"
        zip_files(source, source_files())
        check_zip(source)
        artifacts.append(source)
    for path in artifacts:
        shutil.copyfile(path, args.output / path.name)
        print(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", choices=("windows", "ubuntu", "fedora", "arch", "macos"))
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--toolchain-directory", type=Path, help="Installed Windows GNU toolchain root")
    parser.add_argument("--registry", type=Path, help="Cargo registry/src directory")
    parser.add_argument("--dependency-tree", type=Path, help="Output of cargo tree --locked -p truckcraft -e normal --prefix none")
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--source", action="store_true", help="Also write an artwork-free Git source ZIP")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if args.platform != "windows" and args.binary:
        check_target_path(args.binary)
    with tempfile.TemporaryDirectory(prefix="truckcraft-release-") as temporary:
        work = Path(temporary)
        if args.platform == "windows":
            windows_package(work, args)
        elif not all([args.binary, args.registry, args.dependency_tree]):
            parser.error("Unix packaging requires --binary, --registry and --dependency-tree")
        elif args.platform == "macos":
            macos_package(work, args.output, args.binary, args.registry, args.dependency_tree)
        else:
            install_linux_files(work / "root", args.binary, args.registry, args.dependency_tree)
            packages = {"ubuntu": ubuntu_package, "fedora": fedora_package, "arch": arch_package}
            packages[args.platform](work, args.output)


if __name__ == "__main__":
    main()
