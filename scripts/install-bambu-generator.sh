#!/bin/sh
# Install exact external bytes and their reviewed neutral deployment document.
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
descriptor_hash=a93a499aa55a5096b2c0c264dd7db537e305148f664a9ed2253062cd7358d8c1
handoff_commit=7acb58a4d631c1afdd3d7ffc450d431f184c0160
handoff_root=https://raw.githubusercontent.com/altendky/slicer-project-generators/$handoff_commit/handoffs/bambu-studio/v0.1.0-v1
handoff_hash=8547eae6e1d9378040922972ab9c9bc4561d54afb9c0ba9e3e7305e320eeb40f
review_hash=38733e4aa030f9a470c960fd0464635dde8b32b743fca5ed02cadfc5bf53f8c3
metadata_hash=1874abee0bdc31d52812b47729e84e42ffee5b28ef26d1f88cfd6b527f205c72
handoff_notice_hash=9449ef5bf42aee9b245c1a639025ed2afec4afbcb9f6fd9949fceb97aeb21f91
handoff_readme_hash=98d0a4277b09fcfb6e960a280edeb79b0705e2f85f2371447f66c23d91737b2f

curl --fail --location --proto '=https' --proto-redir '=https' "$release/$package" -o "$scratch/$package"
curl --fail --location --proto '=https' --proto-redir '=https' "$release/$source" -o "$scratch/$source"
printf '%s  %s\n' "$package_hash" "$scratch/$package" "$source_hash" "$scratch/$source" | sha256sum --check

mkdir "$scratch/component"
tar --extract --gzip --file "$scratch/$package" --directory "$scratch/component" --no-same-owner --no-same-permissions
printf '%s  %s\n' "$binary_hash" "$scratch/component/slicer-project-generator-bambu-studio" "$descriptor_hash" "$scratch/component/package.json" | sha256sum --check
mkdir "$scratch/component/handoff"
for name in handoff.json review.json release-metadata.json README.md LICENSE-MIT-service; do
    curl --fail --location --proto '=https' --proto-redir '=https' "$handoff_root/$name" -o "$scratch/component/handoff/$name"
done
printf '%s  %s\n' "$handoff_hash" "$scratch/component/handoff/handoff.json" "$review_hash" "$scratch/component/handoff/review.json" "$metadata_hash" "$scratch/component/handoff/release-metadata.json" | sha256sum --check
printf '%s  %s\n' "$handoff_notice_hash" "$scratch/component/handoff/LICENSE-MIT-service" "$handoff_readme_hash" "$scratch/component/handoff/README.md" | sha256sum --check
# The packet is external AGPL metadata, not service implementation. Preserve its
# license and provenance outside the repository; add only our operational path.
jq --arg executablePath "$install_root/slicer-project-generator-bambu-studio" \
    '.immutableDeployment + {executablePath: $executablePath}' \
    "$scratch/component/handoff/handoff.json" > "$scratch/component/deployed-generator.json"
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
printf '%s\n' 'Exact Bambu component and reviewed neutral deployment document installed.'
