#!/usr/bin/env bash
# Validate-LinuxInstall.sh — local technical report for ExoGPUAgent Linux installs.
# Does not upload data. Redacts hostnames/IPs when --redact is passed.
# Usage: sudo ./Validate-LinuxInstall.sh [--redact] [--out REPORT.md]
set -euo pipefail

REDACT=0
OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --redact) REDACT=1; shift ;;
    --out) OUT="${2:-}"; shift 2 ;;
    -h|--help)
      echo "Usage: $0 [--redact] [--out REPORT.md]"
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

ts="$(date -u +%Y%m%dT%H%M%SZ)"
if [[ -z "${OUT}" ]]; then
  OUT="${PWD}/exogpuagent-linux-validate-${ts}.md"
fi

redact() {
  local s="$1"
  if [[ "${REDACT}" -eq 1 ]]; then
    s="$(printf '%s' "$s" | sed -E \
      -e 's/[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/<ip>/g' \
      -e 's/([A-Za-z0-9-]+\.)+[A-Za-z]{2,}/<host>/g')"
  fi
  printf '%s' "$s"
}

section() {
  printf '\n## %s\n\n' "$1" >> "${OUT}"
}

codeblock() {
  printf '```\n%s\n```\n' "$1" >> "${OUT}"
}

run_capture() {
  local title="$1"
  shift
  section "${title}"
  local out
  if out="$("$@" 2>&1)"; then
    codeblock "$(redact "${out}")"
  else
    codeblock "$(redact "${out}")"$'\n'"[exit $?]"
  fi
}

{
  echo "# ExoGPUAgent Linux validation report"
  echo
  echo "- Generated (UTC): ${ts}"
  echo "- Redact: ${REDACT}"
  echo "- Note: keep this file local; do not commit private host details."
} > "${OUT}"

section "Binary / package"
{
  echo "which: $(command -v exogpuagent || echo 'not in PATH')"
  if command -v exogpuagent >/dev/null 2>&1; then
    exogpuagent version || true
    echo "ldd:"
    ldd "$(command -v exogpuagent)" 2>&1 | head -n 40 || true
    echo "GLIBC symbols:"
    objdump -T "$(command -v exogpuagent)" 2>/dev/null | grep -oE 'GLIBC_[0-9.]+' | sort -u || true
  fi
  if command -v dpkg >/dev/null 2>&1; then
    dpkg -l 'exogpuagent' 2>&1 || true
    dpkg -L exogpuagent 2>&1 | head -n 50 || true
  fi
  echo "glibc (ldd --version):"
  ldd --version 2>&1 | head -n 1 || true
  echo "os-release:"
  if [[ "${REDACT}" -eq 1 ]]; then
    grep -E '^(ID|VERSION_ID|PRETTY_NAME)=' /etc/os-release 2>/dev/null || true
  else
    cat /etc/os-release 2>/dev/null || true
  fi
} | {
  body="$(cat)"
  codeblock "$(redact "${body}")"
}

section "systemd"
{
  systemctl cat exogpuagent.service 2>&1 | head -n 80 || true
  echo "---"
  systemctl status exogpuagent.service --no-pager -l 2>&1 | head -n 40 || true
  echo "is-active: $(systemctl is-active exogpuagent.service 2>&1 || true)"
  echo "is-enabled: $(systemctl is-enabled exogpuagent.service 2>&1 || true)"
} | {
  body="$(cat)"
  codeblock "$(redact "${body}")"
}

section "config (safe)"
{
  if command -v exogpuagent >/dev/null 2>&1; then
    exogpuagent config path --machine 2>&1 || true
    exogpuagent config show --machine 2>&1 || true
  fi
  ls -la /etc/exogpuagent 2>&1 || true
} | {
  body="$(cat)"
  codeblock "$(redact "${body}")"
}

if command -v exogpuagent >/dev/null 2>&1; then
  run_capture "doctor" exogpuagent doctor --machine || true
  run_capture "status" exogpuagent status --machine || true
fi

section "HTTP localhost"
{
  port=12001
  if [[ -f /etc/exogpuagent/config.toml ]]; then
    p="$(sed -n 's/^port *= *\([0-9][0-9]*\).*/\1/p' /etc/exogpuagent/config.toml | head -n1)"
    [[ -n "${p}" ]] && port="${p}"
  fi
  echo "probing 127.0.0.1:${port}"
  curl -sS -m 3 "http://127.0.0.1:${port}/health" || echo "health failed"
  echo
  curl -sS -m 3 "http://127.0.0.1:${port}/gpu" | head -c 2000 || echo "gpu failed"
  echo
} | {
  body="$(cat)"
  # Strip GPU UUIDs if present in future payloads; redact pci-ish strings lightly.
  body="$(printf '%s' "$body" | sed -E 's/GPU-[0-9a-fA-F-]+/<gpu-uuid>/g')"
  codeblock "$(redact "${body}")"
}

section "Listen sockets (expect 127.0.0.1 only)"
{
  if command -v ss >/dev/null 2>&1; then
    ss -ltnp 2>/dev/null | grep -E '12001|12002|exogpuagent' || echo "(no matching listeners)"
  elif command -v netstat >/dev/null 2>&1; then
    netstat -ltnp 2>/dev/null | grep -E '12001|12002|exogpuagent' || echo "(no matching listeners)"
  else
    echo "ss/netstat not available"
  fi
} | {
  body="$(cat)"
  codeblock "$(redact "${body}")"
}

section "Graphical dependency check (core binary)"
{
  if command -v exogpuagent >/dev/null 2>&1; then
    if ldd "$(command -v exogpuagent)" 2>/dev/null | grep -Ei 'libX11|libwayland|libgtk|libQt'; then
      echo "FAIL: unexpected graphical libraries linked"
      ldd "$(command -v exogpuagent)" | grep -Ei 'libX11|libwayland|libgtk|libQt' || true
    else
      echo "OK: no X11/Wayland/GTK/Qt libs in ldd"
    fi
  fi
} >> "${OUT}"

section "Recent journal (last 40 lines)"
{
  journalctl -u exogpuagent -n 40 --no-pager 2>&1 || true
} | {
  body="$(cat)"
  codeblock "$(redact "${body}")"
}

section "Checklist (manual)"
cat >> "${OUT}" <<'EOF'
- [ ] Clean install via apt install ./exogpuagent_*.deb
- [ ] systemctl start/stop/restart
- [ ] Enabled after reboot
- [ ] Config preserved across upgrade
- [ ] apt remove keeps /etc/exogpuagent
- [ ] apt purge removes /etc/exogpuagent and /var/lib/exogpuagent
- [ ] NVML absent → service up, /gpu 503, doctor notes clearly
- [ ] API JSON contract unchanged
- [ ] No listen outside 127.0.0.1
- [ ] Coexists with CUDA/mining workloads (operator verified)

EOF

echo "Wrote ${OUT}"
