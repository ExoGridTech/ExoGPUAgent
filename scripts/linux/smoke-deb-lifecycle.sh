#!/usr/bin/env bash
# Disposable DEB lifecycle smoke test (install → upgrade → remove → purge).
# Safe for CI runners / throwaway VMs only — not for production hosts.
#
# Usage:
#   sudo ./scripts/linux/smoke-deb-lifecycle.sh /path/to/exogpuagent_0.3.0_amd64.deb
set -euo pipefail

DEB="${1:-}"
if [[ -z "${DEB}" || ! -f "${DEB}" ]]; then
  echo "usage: $0 /path/to/exogpuagent_VERSION_amd64.deb" >&2
  exit 2
fi
DEB="$(cd "$(dirname "${DEB}")" && pwd)/$(basename "${DEB}")"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "error: must run as root (CI disposable host / container)" >&2
  exit 1
fi

if ! command -v apt-get >/dev/null 2>&1; then
  echo "error: apt-get required" >&2
  exit 1
fi

CFG=/etc/exogpuagent/config.toml
MARKER_HOST="smoke-preserve-$$"

cleanup_pkg() {
  apt-get purge -y exogpuagent >/dev/null 2>&1 || true
  rm -rf /etc/exogpuagent /var/lib/exogpuagent
}

echo "==> fresh install"
cleanup_pkg
apt-get install -y "${DEB}"

id exogpuagent >/dev/null
getent group exogpuagent >/dev/null
test -f "${CFG}"
test "$(stat -c '%a' /etc/exogpuagent)" = "750"
test "$(stat -c '%a' "${CFG}")" = "640"
# owner group should be exogpuagent
stat -c '%U:%G' "${CFG}" | grep -q 'root:exogpuagent'

systemctl is-enabled exogpuagent.service | grep -Eq 'enabled|enabled-runtime'
# Service may be active even without NVIDIA (HTTP up, /gpu 503).
systemctl start exogpuagent.service || true
sleep 1
systemctl is-active exogpuagent.service | grep -Eq 'active|activating' || {
  echo "warning: service not active — dumping journal" >&2
  journalctl -u exogpuagent -n 50 --no-pager || true
  # Still fail the smoke if unit cannot start at all on a clean host.
  systemctl is-active --quiet exogpuagent.service
}

# Localhost-only listen check (best-effort).
if command -v ss >/dev/null 2>&1; then
  if ss -ltnp | grep -E ':12001\b' | grep -v '127.0.0.1' | grep -q .; then
    echo "error: port 12001 appears bound outside 127.0.0.1" >&2
    ss -ltnp | grep 12001 || true
    exit 1
  fi
fi

curl -fsS --max-time 3 http://127.0.0.1:12001/health >/dev/null \
  || curl -sS --max-time 3 -o /dev/null -w '%{http_code}' http://127.0.0.1:12001/health | grep -Eq '200|503'

# Mark config for upgrade preservation.
exogpuagent config set --machine "host_name=${MARKER_HOST}"

echo "==> upgrade (reinstall same deb) — config must survive; service should not be left disabled"
apt-get install -y --reinstall "${DEB}"
grep -q "host_name = \"${MARKER_HOST}\"" "${CFG}"
systemctl is-enabled exogpuagent.service | grep -Eq 'enabled|enabled-runtime'

echo "==> remove — keep config"
apt-get remove -y exogpuagent
test -f "${CFG}"
grep -q "host_name = \"${MARKER_HOST}\"" "${CFG}"
# Package binary removed
test ! -x /usr/bin/exogpuagent

echo "==> reinstall after remove — restore service, keep marker"
apt-get install -y "${DEB}"
grep -q "host_name = \"${MARKER_HOST}\"" "${CFG}"

echo "==> purge — delete only ExoGPUAgent dirs"
apt-get purge -y exogpuagent
test ! -e /etc/exogpuagent
test ! -e /var/lib/exogpuagent
# User may remain by design
id exogpuagent >/dev/null 2>&1 || true

echo "OK: DEB lifecycle smoke passed"
