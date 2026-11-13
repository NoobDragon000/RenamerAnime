#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APPDIR="${REPO_ROOT}/dist/RenamerAnime.AppDir"
OUT_APPIMAGE="${REPO_ROOT}/dist/RenamerAnime-v2.0.0-linux-x86_64.AppImage"

echo "==> [1/4] Building frontend bundle (ui/dist)..."
(cd "${REPO_ROOT}/ui" && npm ci && npm run build)

echo "==> [2/4] Building Linux release binaries..."
(cd "${REPO_ROOT}" && cargo build --release -p renamer-gui -p renamer-cli)

echo "==> [3/4] Assembling AppDir..."
rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin" \
         "${APPDIR}/usr/share/applications" \
         "${APPDIR}/usr/share/icons/hicolor/256x256/apps" \
         "${APPDIR}/usr/share/renamer-anime/rules"

install -m 755 "${REPO_ROOT}/target/release/renamer-gui" "${APPDIR}/usr/bin/renamer-gui"
install -m 755 "${REPO_ROOT}/target/release/renamer-cli" "${APPDIR}/usr/bin/renamer-cli"
install -m 644 "${REPO_ROOT}/rules/default_rules.json" "${APPDIR}/usr/share/renamer-anime/rules/default_rules.json"
install -m 644 "${REPO_ROOT}/crates/renamer-gui/icons/icon.png" "${APPDIR}/renameranime.png"
install -m 644 "${REPO_ROOT}/crates/renamer-gui/icons/icon.png" "${APPDIR}/.DirIcon"
install -m 644 "${REPO_ROOT}/crates/renamer-gui/icons/icon.png" \
  "${APPDIR}/usr/share/icons/hicolor/256x256/apps/renameranime.png"

cat > "${APPDIR}/io.github.noobdragon000.renameranime.desktop" <<'EOF'
[Desktop Entry]
Name=RenamerAnime
Comment=Batch rename anime folders and episodes using AniList English titles
Exec=renamer-gui
Icon=renameranime
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
StartupNotify=true
X-AppImage-Version=2.0.0
EOF

cp "${APPDIR}/io.github.noobdragon000.renameranime.desktop" \
   "${APPDIR}/usr/share/applications/io.github.noobdragon000.renameranime.desktop"

cat > "${APPDIR}/AppRun" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PATH="${HERE}/usr/bin:${PATH}"
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"
exec "${HERE}/usr/bin/renamer-gui" "$@"
EOF
chmod +x "${APPDIR}/AppRun"

echo "==> [4/4] Building AppImage with appimagetool..."
TOOL_DIR="${REPO_ROOT}/target/appimage-tools"
mkdir -p "${TOOL_DIR}"
APPIMAGETOOL="${TOOL_DIR}/appimagetool-x86_64.AppImage"
if [[ ! -x "${APPIMAGETOOL}" ]]; then
  curl -fsSL -o "${APPIMAGETOOL}" \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
  chmod +x "${APPIMAGETOOL}"
fi

rm -f "${OUT_APPIMAGE}"
ARCH=x86_64 "${APPIMAGETOOL}" --appimage-extract-and-run "${APPDIR}" "${OUT_APPIMAGE}"
chmod +x "${OUT_APPIMAGE}"

echo "==> Standalone Linux AppImage ready: ${OUT_APPIMAGE}"
ls -lh "${OUT_APPIMAGE}"
