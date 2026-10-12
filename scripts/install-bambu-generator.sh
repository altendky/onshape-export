#!/bin/sh
# Install the exact external component; do not rebuild or enable it.
set -eu

if [ "$(id -u)" != 0 ]; then
    printf '%s\n' 'Bambu generator installation requires root ownership.' >&2
    exit 1
fi
if [ "$(uname -m)" != x86_64 ]; then
    printf '%s\n' 'The reviewed Bambu release supports Linux x86_64 only.' >&2
    exit 1
fi

install_root=${1:-/opt/onshape-export/generators/bambu-studio-v0.1.0}
case "$install_root" in
    /*) ;;
    *) printf '%s\n' 'Installation root must be absolute.' >&2; exit 1 ;;
esac
if [ -e "$install_root" ]; then
    printf '%s\n' 'Refusing to replace an existing generator installation.' >&2
    exit 1
fi

mkdir -p "${TMPDIR:-/tmp}/agents"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/agents/XXXXXXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
release=https://github.com/altendky/slicer-project-generators/releases/download/bambu-studio-v0.1.0
package=bambu-studio-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
source=bambu-studio-v0.1.0-corresponding-source.tar.gz
package_hash=a9c859962d23de9812a28871fa6a80334b7543ab695b52db73ca2d91387c3d9c
source_hash=bcc3c15220f739b9df9c6b5a6b2422c05ac4c126a37e2e8c5a8640b3184535ae
binary_hash=734aa2340c954b7d06f1cea0d6c307b33279ffdd74b10cfe1639cdefc7e5e44c

curl --fail --location --proto '=https' --proto-redir '=https' "$release/$package" -o "$scratch/$package"
curl --fail --location --proto '=https' --proto-redir '=https' "$release/$source" -o "$scratch/$source"
printf '%s  %s\n' "$package_hash" "$scratch/$package" "$source_hash" "$scratch/$source" | sha256sum --check

mkdir "$scratch/component"
tar --extract --gzip --file "$scratch/$package" --directory "$scratch/component" --no-same-owner --no-same-permissions
printf '%s  %s\n' "$binary_hash" "$scratch/component/slicer-project-generator-bambu-studio" | sha256sum --check
find "$scratch/component" -type d -exec chmod 0755 {} +
find "$scratch/component" -type f -exec chmod 0644 {} +
chmod 0755 "$scratch/component/slicer-project-generator-bambu-studio"
mkdir "$scratch/component/distribution"
# Retain corresponding source as opaque external distribution bytes, never
# extract it or introduce implementation source into the service repository.
cp "$scratch/$package" "$scratch/$source" "$scratch/component/distribution/"
chmod 0644 "$scratch/component/distribution/$package" "$scratch/component/distribution/$source"
chown -R root:root "$scratch/component"
mkdir -p "$(dirname "$install_root")"
mv "$scratch/component" "$install_root"

find "$install_root" \( ! -user root -o -perm /022 \) -print > "$scratch/invalid-permissions"
if [ -s "$scratch/invalid-permissions" ]; then
    printf '%s\n' 'Installed component ownership or permissions are invalid.' >&2
    exit 1
fi
printf '%s\n' 'Exact Bambu component installed; deployment configuration remains disabled pending reviewed identity handoff.'
