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

The Linux packages are checked on Fedora 45, Ubuntu 26.04, Arch Linux, and CachyOS. Other distributions need compatible system libraries.

- **Bazzite** is Fedora-based. Use `TruckCraft.rpm` inside a Fedora 45 Distrobox container. TruckCraft has yet to be tested on Bazzite itself. [Bazzite’s Distrobox guide](https://docs.bazzite.gg/Installing_and_Managing_Software/Distrobox/)
- **SteamOS (experimental).** In Desktop Mode, use the Flatpak download and run the install command in Konsole. The bundle passed automated Linux sandbox tests. Testing on SteamOS and Steam Deck hardware is still pending. [Valve’s SteamOS desktop FAQ](https://help.steampowered.com/en/faqs/view/671A-4453-E8D2-323C)
- **macOS** has an opt-in universal CI test build with automated smoke tests on Apple Silicon and Intel. Hands-on testing on a Mac is still pending. [macOS test builds]($repository/actions/workflows/package-check.yml)

For help with save discovery, backups, and recovery, see the [TruckCraft README]($repository#start-using-truckcraft).
