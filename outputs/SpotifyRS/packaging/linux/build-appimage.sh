#!/usr/bin/env bash
# Arguments: release executable, output AppImage. Run on x86_64 Linux.
set -euo pipefail
binary=$(realpath "$1")
output=$(realpath -m "$2")
project=$(cd "$(dirname "$0")/../.." && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cd "$stage"
app=SpotifyRS.AppDir
install -Dm755 "$binary" "$app/usr/bin/spotifyrs"
install -Dm644 "$project/ui/assets/spotifyrs.svg" "$app/spotifyrs.svg"
install -m644 "$project/packaging/linux/spotifyrs.desktop" "$app/spotifyrs.desktop"
ln -s spotifyrs.svg "$app/.DirIcon"
mkdir -p "$app/usr/lib" "$app/usr/share/doc/spotifyrs"
cp "$project/../../LICENSE" "$project/THIRD_PARTY_NOTICES.md" "$app/usr/share/doc/spotifyrs/"
cp -r "$project/licenses" "$app/usr/share/doc/spotifyrs/"
# Keep the desktop's glibc, display drivers and font configuration.
for lib in libssl.so.3 libcrypto.so.3; do
    path=$(ldd "$binary" | awk -v name="$lib" '$1 == name {print $3}')
    install -m644 "$path" "$app/usr/lib/$lib"
done
cp /usr/share/doc/libssl3*/copyright "$app/usr/share/doc/spotifyrs/OpenSSL-copyright"
cat > "$app/AppRun" <<'RUN'
#!/bin/sh
appdir=$(dirname "$(readlink -f "$0")")
export LD_LIBRARY_PATH="$appdir/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$appdir/usr/bin/spotifyrs" "$@"
RUN
chmod +x "$app/AppRun"
curl --fail --location --retry 3 -o appimagetool https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
echo 'ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0  appimagetool' | sha256sum -c -
chmod +x appimagetool
# Reuse the verified tool's own x86_64 type-2 runtime.
head -c "$(./appimagetool --appimage-offset)" appimagetool > runtime
./appimagetool --appimage-extract >/dev/null
ARCH=x86_64 squashfs-root/AppRun --runtime-file runtime "$app" "$output"
