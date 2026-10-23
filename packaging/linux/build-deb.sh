#!/usr/bin/env bash
# Build exogpuagent_VERSION_amd64.deb from a prebuilt release binary.
# Intended for Ubuntu 22.04 CI (glibc 2.35 baseline). Does not install NVIDIA drivers.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "${ROOT}/Cargo.toml" | head -n1)}"
BIN="${BIN:-${ROOT}/target/release/exogpuagent}"
OUT_DIR="${OUT_DIR:-${ROOT}/dist/deb}"
ARCH="${ARCH:-amd64}"
PKG_NAME="exogpuagent"
DEB_NAME="${PKG_NAME}_${VERSION}_${ARCH}.deb"

if [[ ! -x "${BIN}" ]]; then
  echo "error: missing release binary: ${BIN}" >&2
  exit 1
fi

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "error: dpkg-deb is required" >&2
  exit 1
fi

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/exogpuagent-deb.XXXXXX")"
cleanup() { rm -rf "${STAGE}"; }
trap cleanup EXIT

DEST="${STAGE}/${PKG_NAME}"
mkdir -p \
  "${DEST}/DEBIAN" \
  "${DEST}/usr/bin" \
  "${DEST}/lib/systemd/system" \
  "${DEST}/usr/share/exogpuagent" \
  "${DEST}/usr/share/doc/${PKG_NAME}" \
  "${DEST}/etc/exogpuagent"

install -m 0755 "${BIN}" "${DEST}/usr/bin/exogpuagent"
install -m 0644 "${ROOT}/packaging/linux/exogpuagent.service" \
  "${DEST}/lib/systemd/system/exogpuagent.service"
install -m 0644 "${ROOT}/packaging/linux/config.toml.default" \
  "${DEST}/usr/share/exogpuagent/config.toml.default"

# Do not ship /etc/exogpuagent/config.toml in the package payload so upgrades
# never clobber operator settings (postinst copies default if absent).

install -m 0644 "${ROOT}/packaging/linux/debian/copyright" \
  "${DEST}/usr/share/doc/${PKG_NAME}/copyright"
install -m 0644 "${ROOT}/packaging/linux/debian/changelog" \
  "${DEST}/usr/share/doc/${PKG_NAME}/changelog.Debian"
gzip -9nf "${DEST}/usr/share/doc/${PKG_NAME}/changelog.Debian"
install -m 0644 "${ROOT}/README.md" "${DEST}/usr/share/doc/${PKG_NAME}/README"
if [[ -f "${ROOT}/docs/phase-2c-linux.md" ]]; then
  install -m 0644 "${ROOT}/docs/phase-2c-linux.md" \
    "${DEST}/usr/share/doc/${PKG_NAME}/phase-2c-linux.md"
fi

install -m 0755 "${ROOT}/packaging/linux/debian/postinst" "${DEST}/DEBIAN/postinst"
install -m 0755 "${ROOT}/packaging/linux/debian/prerm" "${DEST}/DEBIAN/prerm"
install -m 0755 "${ROOT}/packaging/linux/debian/postrm" "${DEST}/DEBIAN/postrm"

# Rough installed size in KiB.
INSTALLED_SIZE="$(du -sk "${DEST}" | awk '{print $1}')"

# Detect libc Depends from versioned symbols (objdump -T), baseline Ubuntu 22.04 / glibc 2.35.
LIBC_DEP="libc6 (>= 2.35)"
if command -v objdump >/dev/null 2>&1; then
  SYMS="$(objdump -T "${BIN}" 2>/dev/null | grep -oE 'GLIBC_[0-9.]+' | sort -u || true)"
  if printf '%s\n' "${SYMS}" | grep -qx 'GLIBC_2.39'; then
    LIBC_DEP="libc6 (>= 2.39)"
  elif printf '%s\n' "${SYMS}" | grep -qx 'GLIBC_2.38'; then
    LIBC_DEP="libc6 (>= 2.38)"
  elif printf '%s\n' "${SYMS}" | grep -qx 'GLIBC_2.36'; then
    LIBC_DEP="libc6 (>= 2.36)"
  fi
  echo "Detected GLIBC symbols:"
  printf '%s\n' "${SYMS}"
  echo "Depends: ${LIBC_DEP}"
fi

cat > "${DEST}/DEBIAN/control" <<EOF
Package: ${PKG_NAME}
Version: ${VERSION}
Architecture: ${ARCH}
Maintainer: ExoGridTech <noreply@exogrid.tech>
Installed-Size: ${INSTALLED_SIZE}
Depends: ${LIBC_DEP}
Section: utils
Priority: optional
Homepage: https://github.com/ExoGridTech/ExoGPUAgent
Description: Local-first NVIDIA GPU telemetry agent
 Lightweight localhost HTTP agent exposing /health and /gpu via NVML.
 Runs as a systemd service under a dedicated user. Does not install NVIDIA
 drivers, CUDA, Tailscale, or graphical desktop components.
EOF

mkdir -p "${OUT_DIR}"
echo "Running: dpkg-deb --root-owner-group --build ${DEST} ${OUT_DIR}/${DEB_NAME}"
if ! dpkg-deb --root-owner-group --build "${DEST}" "${OUT_DIR}/${DEB_NAME}"; then
  echo "error: dpkg-deb failed to build ${DEB_NAME}" >&2
  exit 1
fi

if [[ ! -f "${OUT_DIR}/${DEB_NAME}" ]]; then
  echo "error: dpkg-deb reported success but ${OUT_DIR}/${DEB_NAME} is missing" >&2
  ls -la "${OUT_DIR}" >&2 || true
  exit 1
fi
if [[ ! -s "${OUT_DIR}/${DEB_NAME}" ]]; then
  echo "error: built package is empty: ${OUT_DIR}/${DEB_NAME}" >&2
  exit 1
fi

(
  cd "${OUT_DIR}"
  sha256sum "${DEB_NAME}" > SHA256SUMS.txt
  sha256sum -c SHA256SUMS.txt
)

echo "Built ${OUT_DIR}/${DEB_NAME}"
dpkg-deb -I "${OUT_DIR}/${DEB_NAME}"
dpkg-deb -c "${OUT_DIR}/${DEB_NAME}" | head -n 40

# Machine-readable marker for CI assertions
{
  echo "deb_path=${OUT_DIR}/${DEB_NAME}"
  echo "deb_name=${DEB_NAME}"
  echo "version=${VERSION}"
  sha256sum "${OUT_DIR}/${DEB_NAME}"
} > "${OUT_DIR}/build-meta.txt"
cat "${OUT_DIR}/build-meta.txt"

# Record glibc baseline hint for docs/CI logs.
if command -v ldd >/dev/null 2>&1; then
  echo "--- ldd (host) ---"
  ldd --version | head -n1 || true
  ldd "${BIN}" || true
fi
if command -v objdump >/dev/null 2>&1; then
  echo "--- GLIBC symbols (unique majors) ---"
  objdump -T "${BIN}" 2>/dev/null | grep -oE 'GLIBC_[0-9.]+' | sort -u || true
fi
