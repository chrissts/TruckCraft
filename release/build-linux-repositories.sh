#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

if [[ $# -ne 5 ]]; then
    echo "Usage: $0 ASSETS_DIR OUTPUT_DIR RELEASE_TAG SIGNING_FINGERPRINT BASE_URL" >&2
    exit 2
fi
assets=$(realpath "$1")
output=$2
tag=$3
key=$4
base_url=${5%/}
if [[ ! $tag =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ||
      ! $key =~ ^[A-Fa-f0-9]{40}$ || ! $base_url =~ ^https://[A-Za-z0-9./_-]+$ ]]; then
    echo 'Expected a stable version tag, an OpenPGP fingerprint and an HTTPS base URL.' >&2
    exit 2
fi
version=${tag#v}
app_id=io.github.chrissts.TruckCraft
gpg=(gpg --batch --local-user "$key")
arch_name="truckcraft-$version-1-x86_64.pkg.tar.zst"
deb="$assets/TruckCraft.deb"
rpm="$assets/TruckCraft.rpm"
arch="$assets/TruckCraft.pkg.tar.zst"
bundle="$assets/TruckCraft.flatpak"
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
rpm --dbpath "$scratch/rpmdb" --initdb

# The feed advertises only releases whose native metadata matches the requested version.
if [[ $(dpkg-deb -f "$deb" Package Version Architecture) != $'Package: truckcraft\nVersion: '"$version"$'\nArchitecture: amd64' ||
      $(rpm --dbpath "$scratch/rpmdb" -qp --qf '%{NAME} %{VERSION} %{RELEASE} %{ARCH}' "$rpm") != "truckcraft $version 1.fc45 x86_64" ||
      $(bsdtar -xOf "$arch" .PKGINFO | sed -nE '/^(pkgname|pkgver|arch) = /p') != $'pkgname = truckcraft\npkgver = '"$version"$'-1\narch = x86_64' ]]; then
    echo "The Linux packages do not match $tag and the expected architectures." >&2
    exit 1
fi

mkdir -- "$output"
output=$(realpath "$output")
"${gpg[@]}" --armor --export "$key" > "$output/truckcraft.asc"
"${gpg[@]}" --export "$key" > "$output/truckcraft.gpg"
test -s "$output/truckcraft.gpg"

(
    mkdir -p "$output/debian"
    cd "$output/debian"
    pool=pool/main/t/truckcraft
    suite=dists/stable
    component="$suite/main/binary-amd64"
    mkdir -p "$pool" "$component"
    cp -- "$deb" "$pool/truckcraft_${version}_amd64.deb"
    apt-ftparchive packages pool > "$component/Packages"
    gzip -n -c "$component/Packages" > "$component/Packages.gz"
    apt-ftparchive \
        -o APT::FTPArchive::Release::Origin=TruckCraft \
        -o APT::FTPArchive::Release::Label=TruckCraft \
        -o APT::FTPArchive::Release::Suite=stable \
        -o APT::FTPArchive::Release::Codename=stable \
        -o APT::FTPArchive::Release::Architectures=amd64 \
        -o APT::FTPArchive::Release::Components=main \
        -o APT::FTPArchive::Release::Acquire-By-Hash=yes \
        release "$suite" > Release
    mv Release "$suite/Release"
    install -d "$component/by-hash/SHA256" "$component/by-hash/SHA512"
    for index in "$component/Packages" "$component/Packages.gz"; do
        sha256=$(sha256sum "$index")
        sha512=$(sha512sum "$index")
        cp -- "$index" "$component/by-hash/SHA256/${sha256%% *}"
        cp -- "$index" "$component/by-hash/SHA512/${sha512%% *}"
    done
    "${gpg[@]}" --clearsign --output "$suite/InRelease" "$suite/Release"
    "${gpg[@]}" --armor --detach-sign --output "$suite/Release.gpg" "$suite/Release"
)

(
    mkdir -p "$output/fedora"
    cd "$output/fedora"
    package="truckcraft-$version-1.fc45.x86_64.rpm"
    cp -- "$rpm" "$package"
    rpmsign --dbpath "$scratch/rpmdb" --key-id "$key" --addsign --rpmv4 "$package"
    rpmkeys --dbpath "$scratch/rpmdb" --import "$output/truckcraft.asc"
    rpmkeys --dbpath "$scratch/rpmdb" --checksig "$package"
    createrepo_c --quiet .
    "${gpg[@]}" --armor --detach-sign repodata/repomd.xml
)

(
    mkdir -p "$output/arch"
    cd "$output/arch"
    cp -- "$arch" "$arch_name"
    "${gpg[@]}" --detach-sign "$arch_name"
    repo-add --sign --key "$key" --include-sigs truckcraft.db.tar.gz "$arch_name"
    # Pages artifacts accept regular files. Pacman reads these short names.
    for name in truckcraft.db truckcraft.db.sig truckcraft.files truckcraft.files.sig; do
        if [[ -L $name ]]; then
            cp -L -- "$name" "$name.copy"
            mv -- "$name.copy" "$name"
        fi
    done
)

repo="$output/flatpak"
ostree --repo="$repo" init --mode=archive-z2
flatpak build-import-bundle --no-update-summary "$repo" "$bundle"
flatpak build-sign --gpg-sign="$key" "$repo" "$app_id" stable
flatpak build-update-repo --gpg-sign="$key" "$repo"
ostree --repo="$repo" fsck

cat > "$output/TruckCraft.flatpakref" <<EOF
[Flatpak Ref]
Name=$app_id
Branch=stable
Title=TruckCraft
Url=$base_url/flatpak/
SuggestRemoteName=truckcraft
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
IsRuntime=false
GPGKey=$(base64 -w0 "$output/truckcraft.gpg")
EOF

cat > "$output/truckcraft.repo" <<EOF
[truckcraft]
name=TruckCraft
baseurl=$base_url/fedora/
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$base_url/truckcraft.asc
EOF

cat > "$output/index.html" <<EOF
<!doctype html>
<meta charset="utf-8">
<title>TruckCraft package repositories</title>
<h1>TruckCraft Linux packages</h1>
<p>Current release: $version</p>
<ul>
  <li><a href="https://github.com/chrissts/TruckCraft#linux-updates">Package manager setup and signing key</a></li>
  <li><a href="TruckCraft.flatpakref">Install the experimental Flatpak</a></li>
  <li><a href="https://github.com/chrissts/TruckCraft/releases/latest">Download standalone packages</a></li>
</ul>
EOF
