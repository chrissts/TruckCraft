# TruckCraft release packages

TruckCraft stays on app version 1.0 and targets ETS2 and ATS 1.62.x. The
packaging commands create local artifacts with TruckCraft artwork, the original
attribution, and third-party license notices. They exclude bundled SCS game art.
The packaging script requires Python 3.11 or later.

| Platform | Package | Build environment |
| --- | --- | --- |
| Windows x64 | Application ZIP, optional source ZIP | Windows with Rust 1.100.0 and the GNU linker |
| Ubuntu x86-64 | `.deb` | Ubuntu 26.04 LTS |
| Fedora x86-64 | `.rpm` | Fedora 45 |
| Arch Linux and CachyOS x86-64 | `.pkg.tar.zst` | Arch Linux or CachyOS with `makepkg` |
| Flatpak x86-64 | `TruckCraft.flatpak` | Linux with Flatpak and Flatpak Builder |
| macOS | Universal `.app` in a ZIP | macOS with Xcode Command Line Tools and both Apple Rust targets |

## Ubuntu 26.04 LTS

```sh
sudo apt install build-essential pkg-config libgtk-3-dev libgl-dev libegl-dev \
  libxkbcommon-dev libwayland-dev libfontconfig1-dev libdbus-1-dev \
  python3 dpkg-dev desktop-file-utils
bash release/build-unix.sh ubuntu
sudo apt install ./dist/TruckCraft-Ubuntu26.04-amd64.deb
```

## Fedora 45

```sh
sudo dnf install gcc pkgconf-pkg-config gtk3-devel mesa-libGL-devel \
  mesa-libEGL-devel libxkbcommon-devel wayland-devel fontconfig-devel dbus-devel \
  python3 rpm-build desktop-file-utils
bash release/build-unix.sh fedora
sudo dnf install ./dist/TruckCraft-Fedora45-x86_64.rpm
```

Run `TRUCKCRAFT_BUNDLE_LOCAL_ART=0 cargo test --locked --workspace` on each
build system. Use
`TRUCKCRAFT_DIST_DIR` to place generated packages outside the checkout.

## Arch Linux and CachyOS

```sh
sudo pacman -Syu --needed base-devel python rustup desktop-file-utils gtk3 \
  libglvnd libx11 libxkbcommon libxkbcommon-x11 wayland curl
rustup toolchain install 1.100.0 --profile minimal
RUSTUP_TOOLCHAIN=1.100.0 bash release/build-unix.sh arch
sudo pacman -U ./dist/TruckCraft.pkg.tar.zst
```

Run the build as a regular user so `makepkg` can create the package.

## Flatpak

Build from a committed source tree with Flatpak, Flatpak Builder, and the
workspace Rust toolchain installed. The script uses Flathub for the GNOME SDK
and Rust extension.

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
bash release/build-flatpak.sh
flatpak install --user ./dist/TruckCraft.flatpak
flatpak run io.github.chrissts.TruckCraft
```

Standard game and Steam folders are visible in the sandbox. For a custom save
directory or another Steam library, grant access to that folder and then add
it in TruckCraft. Replace the example with an absolute path.

```sh
flatpak override --user --filesystem=/absolute/path/to/save-folder io.github.chrissts.TruckCraft
```

## macOS

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
TRUCKCRAFT_BUNDLE_LOCAL_ART=0 cargo test --locked --workspace
bash release/build-unix.sh macos
```

The script builds both architectures, checks that the executable contains both
slices, assembles `TruckCraft.app`, and creates an ad hoc signature for local
testing. The configured minimum is macOS 12. Verify on real Apple Silicon and
Intel Macs before release. Developer ID signing and Apple notarization are
required for normal Gatekeeper distribution. Apply the Developer ID signature
with the hardened runtime to the app before notarizing and stapling it, then
re-create the ZIP with `ditto`. [Apple Developer ID guidance](https://developer.apple.com/developer-id/)

## Windows x64

Run these commands from PowerShell with Python 3.11 or later. The wrapper uses
the Rust target and registry installed at the supplied toolchain path.

```powershell
python release/package.py windows --toolchain-directory 'C:\path\to\rust-toolchain'
```

The Windows command builds the GNU target without game artwork, inspects the ZIP contents and prints the archive’s SHA-256 hash. Use `--output` for a separate output directory. Run the extracted executable with `--package-check` for a native disposable-save check.

Add `--source` to create a local source ZIP from the current contents of tracked files. It includes the
release scripts and excludes installed game art. Build the source ZIP with
`TRUCKCRAFT_BUNDLE_LOCAL_ART=0`.

## Verification

Inspect the decompressed packages for local paths, fixture data, unexpected
files, and license notices. Scan the exact executable that will ship, then
record SHA-256 hashes. Launch each package on a clean system and check profile
discovery, browse dialogs, Save, Undo, Redo and recovery with a disposable save.
Load edited copies in both simulators and record those checks separately.

The manual [native package workflow](../.github/workflows/package-check.yml) builds each package and tests it on a fresh runner. The Flatpak check repeats the save and recovery test in native Linux, Proton, and explicitly granted custom directories. Both Mac runners use the same universal ZIP. The Arch Linux and CachyOS containers install the same pacman package. If enabled, the draft release attaches the verified Windows, RPM, Debian, pacman, and Flatpak downloads. The workflow retains private test artifacts for one day.

### Sign a Windows update

Before publishing a draft, download its final `TruckCraft.exe` and run the signer on the Windows release machine with PowerShell 7 and OpenSSL 3. Use the private key corresponding to [the public key bundled in TruckCraft](windows-update-public-key.txt). Keep the private key and a secure backup outside the repository.

```powershell
./release/sign-windows-update.ps1 -Tag v1.0.1 -Executable 'C:\releases\TruckCraft.exe' -SigningKey 'C:\keys\windows-updates-ed25519.pem'
```

Append the output as its own line at the end of the draft description. GitHub hides this HTML comment on the release page. It signs the exact tag, size, and SHA-256 of the executable, so the file must be final before signing. Publish the draft with the signed description. Releases without signed metadata still appear in the update dialog with a manual download link.

Test Windows updates in a writable directory using isolated instances and two different app versions. Check pending edits, a successful download and restart, rejection of altered payloads, and recovery if the replacement cannot start.

### Publish Linux update feeds

Publishing a stable GitHub release automatically starts **Publish Linux update
feeds** on `main`. Drafts and prereleases leave the feeds alone. The workflow
checks the latest stable release asset digests and builds signed APT, DNF,
pacman and Flatpak repositories from those packages. GitHub Pages deploys the
feeds together. The workflow can also be run manually from `main` to redeploy
the latest stable release.

The workflow uses the `LINUX_REPOSITORY_SIGNING_KEY` secret in the
`linux-repository` environment, which allows deployments from `main`. The
exported OpenPGP key must match the fingerprint in
[`linux-repository-fingerprint.txt`](linux-repository-fingerprint.txt). Keep a
secure offline backup and rotate the key before it expires.

## Release notes

For each version, add a Markdown summary of user-facing changes to
`release/notes/<Cargo version>.md`. Preview the full notes with
`python3 release/render-notes.py`. The draft job requires that version’s notes
and renders them with [the shared template](notes-template.md).
