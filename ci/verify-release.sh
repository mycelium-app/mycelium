#!/bin/bash
# Checks a published release from S3: checksums, versions, the APK's signer and the latest marker.
# Usage: ci/verify-release.sh vX.Y.Z   (AAPT2 and APKSIGNER may point at the Android build tools)
set -u
tag=$1
version=${tag#v}
base=https://github.com/mycelium-app/mycelium/releases/download
aapt2=${AAPT2:-$(command -v aapt2)}
apksigner=${APKSIGNER:-$(command -v apksigner)}
work=$(mktemp -d)
cd "$work" || exit 1

for asset in mycelium-linux-x86_64 mycelium-desktop-linux-x86_64.deb mycelium-desktop-linux-x86_64.tar.gz mycelium-web.tar.gz mycelium-android.apk; do
  curl -sfS -o "$asset" "$base/$tag/$asset" || { echo "MISSING $asset"; continue; }
  want=$(curl -sfS "$base/$tag/$asset.sha256" | cut -d' ' -f1)
  got=$(sha256sum "$asset" | cut -d' ' -f1)
  [ "$want" = "$got" ] && echo "sha ok  $asset" || echo "SHA BAD $asset"
done

chmod +x mycelium-linux-x86_64
echo "binary: $(./mycelium-linux-x86_64 version 2>&1 | head -1)"
echo "deb: $(dpkg-deb -f mycelium-desktop-linux-x86_64.deb Version)"
[ -n "$aapt2" ] && "$aapt2" dump badging mycelium-android.apk 2>/dev/null | head -1 | grep -o "versionCode='[0-9]*' versionName='[^']*'"
[ -n "$apksigner" ] && "$apksigner" verify --print-certs mycelium-android.apk 2>/dev/null | grep -o "SHA-256 digest: [0-9a-f]\{8\}"
echo "expected versionCode $(( ${version%%.*} * 10000 + $(echo "$version" | cut -d. -f2) * 100 + $(echo "$version" | cut -d. -f3) )), signer 6ca54d2f"
curl -sfS -o flake.tgz "$base/$tag/mycelium-flake.tar.gz" && tar xzf flake.tgz && grep -rho 'version = "[^"]*"' . 2>/dev/null | sort -u | head -2
echo "latest: $(curl -sfS "$base/latest")"

cd / && rm -rf "$work"
