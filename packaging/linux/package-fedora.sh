#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="2.0.0"
PKG_NAME="renamer-anime-${VERSION}-linux-x64"
STAGE_DIR="${REPO_ROOT}/dist/${PKG_NAME}"
TAR_PATH="${REPO_ROOT}/dist/${PKG_NAME}.tar.gz"

echo "Building frontend (ui/dist)..."
(cd "${REPO_ROOT}/ui" && npm ci && npm run build)

echo "Building Linux release binaries..."
(cd "${REPO_ROOT}" && cargo build --release --workspace)

rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/bin" "${STAGE_DIR}/rules" "${STAGE_DIR}/share/applications" "${STAGE_DIR}/share/icons/hicolor/256x256/apps"

install -m 755 "${REPO_ROOT}/target/release/renamer-gui" "${STAGE_DIR}/bin/renamer-gui"
install -m 755 "${REPO_ROOT}/target/release/renamer-cli" "${STAGE_DIR}/bin/renamer-cli"
install -m 755 "${REPO_ROOT}/anime.sh" "${STAGE_DIR}/bin/anime.sh"
install -m 644 "${REPO_ROOT}/rules/default_rules.json" "${STAGE_DIR}/rules/default_rules.json"
install -m 644 "${REPO_ROOT}/README.md" "${STAGE_DIR}/README.md"
install -m 644 "${REPO_ROOT}/packaging/linux/io.github.noobdragon000.renameranime.desktop" \
  "${STAGE_DIR}/share/applications/io.github.noobdragon000.renameranime.desktop"
install -m 644 "${REPO_ROOT}/crates/renamer-gui/icons/icon.png" \
  "${STAGE_DIR}/share/icons/hicolor/256x256/apps/renameranime.png"

cat > "${STAGE_DIR}/run-renamer-gui.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${DIR}/bin/renamer-gui" "$@"
EOF
chmod +x "${STAGE_DIR}/run-renamer-gui.sh"

tar -C "${REPO_ROOT}/dist" -czf "${TAR_PATH}" "${PKG_NAME}"
echo "Packaged Fedora/Linux archive: ${TAR_PATH}"

# Optional RPM packaging when rpmbuild is available on Fedora
if command -v rpmbuild >/dev/null 2>&1; then
  RPM_TOP="${REPO_ROOT}/dist/rpmbuild"
  rm -rf "${RPM_TOP}"
  mkdir -p "${RPM_TOP}"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
  cp "${TAR_PATH}" "${RPM_TOP}/SOURCES/${PKG_NAME}.tar.gz"

  cat > "${RPM_TOP}/SPECS/renamer-anime.spec" <<SPEC
Name:           renamer-anime
Version:        ${VERSION}
Release:        1%{?dist}
Summary:        Cross-platform anime library renamer powered by AniList GraphQL API
License:        MIT
Source0:        ${PKG_NAME}.tar.gz

%description
Batch rename anime folders and episodes using AniList English titles with tag cleanup, rate-limit protection, and one-click undo.

%prep
%setup -q -n ${PKG_NAME}

%install
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}/usr/share/applications
mkdir -p %{buildroot}/usr/share/icons/hicolor/256x256/apps
mkdir -p %{buildroot}/usr/share/renamer-anime/rules
install -m 755 bin/renamer-gui %{buildroot}/usr/bin/renamer-gui
install -m 755 bin/renamer-cli %{buildroot}/usr/bin/renamer-cli
install -m 644 share/applications/io.github.noobdragon000.renameranime.desktop %{buildroot}/usr/share/applications/
install -m 644 share/icons/hicolor/256x256/apps/renameranime.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/renameranime.png
install -m 644 rules/default_rules.json %{buildroot}/usr/share/renamer-anime/rules/default_rules.json

%files
/usr/bin/renamer-gui
/usr/bin/renamer-cli
/usr/share/applications/io.github.noobdragon000.renameranime.desktop
/usr/share/icons/hicolor/256x256/apps/renameranime.png
/usr/share/renamer-anime/rules/default_rules.json
SPEC

  rpmbuild --define "_topdir ${RPM_TOP}" -bb "${RPM_TOP}/SPECS/renamer-anime.spec"
  find "${RPM_TOP}/RPMS" -name "*.rpm" -exec cp {} "${REPO_ROOT}/dist/" \;
fi
