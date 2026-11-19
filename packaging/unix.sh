#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
bin=$(cd "${1:-target/release}" && pwd)
name=lamina
title=Lamina
version=$(awk '/^version = "/ {gsub(/["[:space:]]/,"",$3); print $3; exit}' Cargo.toml)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p target/installers
case "$(uname)" in
Darwin)
  app="$stage/$title.app/Contents"
  mkdir -p "$app/MacOS" "$app/Resources"
  cp "$bin/$name" "$bin/$name-viewer" "$app/MacOS/"
  cp assets/icon.icns "$app/Resources/icon.icns"
  cat > "$app/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>app.$name.viewer</string>
<key>CFBundleName</key><string>$title</string>
<key>CFBundleExecutable</key><string>$name-viewer</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>CFBundleVersion</key><string>$version</string>
<key>CFBundleShortVersionString</key><string>$version</string>
</dict></plist>
EOF

  pkgbuild --component "$stage/$title.app" --install-location /Applications --identifier "app.$name.viewer" --version "$version" "target/installers/$name-$version-$(lipo -archs "$bin/$name-viewer").pkg"
  ;;
Linux)
  pkg="$stage/pkg"
  mkdir -p "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" "$pkg/usr/share/icons/hicolor/scalable/apps" "$stage/debian"
  install -m755 "$bin/$name" "$bin/$name-viewer" "$pkg/usr/bin/"
  install -m644 assets/icon.svg "$pkg/usr/share/icons/hicolor/scalable/apps/$name.svg"
  cat > "$pkg/usr/share/applications/$name.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$title
Exec=$name-viewer
Icon=$name
Terminal=false
Categories=Graphics;Engineering;
EOF
  touch "$stage/debian/control"
  deps=$(cd "$stage" && dpkg-shlibdeps -O "$pkg/usr/bin/$name" "$pkg/usr/bin/$name-viewer")
  arch=$(dpkg --print-architecture)
  cat > "$pkg/DEBIAN/control" <<EOF
Package: $name
Version: $version
Architecture: $arch
Maintainer: $title project
Depends: ${deps#shlibs:Depends=}
Description: $title design application
EOF
  dpkg-deb --root-owner-group --build "$pkg" "target/installers/$name-$version-$arch.deb"
  ;;
*) exit 1 ;;
esac
