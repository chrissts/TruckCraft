## What's new in TruckCraft $version

$changes

## Compatibility

TruckCraft edits local Euro Truck Simulator 2 and American Truck Simulator saves. Close the game and back up your profile before editing.

## Downloads

| Platform | File | How to open it |
| --- | --- | --- |
| Windows x64 | `TruckCraft.exe` | Run the executable. |
| Fedora and Fedora-based distributions (x86-64) | `TruckCraft.rpm` | Install with `sudo dnf install ./TruckCraft.rpm`. |
| Ubuntu, Debian, and their derivatives (x86-64) | `TruckCraft.deb` | Install with `sudo apt install ./TruckCraft.deb`. |
| Arch Linux and CachyOS (x86-64) | `TruckCraft.pkg.tar.zst` | Install with `sudo pacman -U ./TruckCraft.pkg.tar.zst`. |
| Linux with Flatpak (x86-64, experimental) | `TruckCraft.flatpak` | Install with `flatpak install --user ./TruckCraft.flatpak`. |
| Portable Linux (x86-64, experimental) | `TruckCraft.AppImage` | Make it executable with `chmod +x TruckCraft.AppImage`, then run `./TruckCraft.AppImage`. |

The Linux packages are checked on Fedora 45, Ubuntu 26.04, Arch Linux, and CachyOS. Other distributions need compatible system libraries.

The AppImage targets Ubuntu 26.04 and compatible x86-64 distributions. It uses
the host’s GTK 3 and graphics libraries. If FUSE is unavailable, run
`./TruckCraft.AppImage --appimage-extract-and-run`. [AppImage FUSE help](https://docs.appimage.org/user-guide/troubleshooting/fuse.html)

- **Bazzite.** Try the experimental `TruckCraft.flatpak` first. For `TruckCraft.rpm`, Bazzite recommends a Fedora Distrobox. Use Fedora 45 to match TruckCraft’s RPM tests. You can also install the RPM on the host with `rpm-ostree` if its dependencies resolve. Host installation requires a reboot. TruckCraft testing on Bazzite is pending. [Bazzite’s RPM guide](https://docs.bazzite.gg/Installing_and_Managing_Software/rpm-ostree/)
- **SteamOS (experimental).** In Desktop Mode, use the Flatpak download and run the install command in Konsole. The bundle passed automated Linux sandbox tests. Testing on SteamOS and Steam Deck hardware is still pending. [Valve’s SteamOS desktop FAQ](https://help.steampowered.com/en/faqs/view/671A-4453-E8D2-323C)
- **macOS** has an opt-in universal CI test build with automated smoke tests on Apple Silicon and Intel. Hands-on testing on a Mac is still pending. [macOS test builds]($repository/actions/workflows/package-check.yml)

For help with save discovery, backups, and recovery, see the [TruckCraft README]($repository#start-using-truckcraft).
