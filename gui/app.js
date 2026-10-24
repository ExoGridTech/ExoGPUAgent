(() => {
  const state = {
    csrf: null,
    pollMs: 5000,
    timer: null,
    view: "dashboard",
  };

  const $ = (id) => document.getElementById(id);

  function show(view) {
    ["login", "dashboard", "settings", "diagnostics"].forEach((v) => {
      $(`view-${v}`).classList.toggle("hidden", v !== view);
    });
    $("nav").classList.toggle("hidden", view === "login");
    state.view = view;
  }

  async function api(path, opts = {}) {
    const headers = Object.assign({ "Content-Type": "application/json" }, opts.headers || {});
    if (opts.method && opts.method !== "GET" && state.csrf) {
      headers["X-CSRF-Token"] = state.csrf;
      headers["X-Requested-With"] = "ExoGPUAgentAdmin";
    }
    const res = await fetch(path, {
      credentials: "same-origin",
      ...opts,
      headers,
    });
    const text = await res.text();
    let body = null;
    try { body = text ? JSON.parse(text) : null; } catch (_) { body = { raw: text }; }
    if (!res.ok) {
      const err = new Error((body && (body.error || body.detail)) || res.statusText);
      err.status = res.status;
      err.body = body;
      throw err;
    }
    return body;
  }

  async function refreshSession() {
    try {
      const s = await api("/admin/api/session");
      if (!s.authenticated) {
        state.csrf = null;
        show("login");
        return false;
      }
      state.csrf = s.csrf_token;
      if (s.poll_interval_secs) state.pollMs = s.poll_interval_secs * 1000;
      return true;
    } catch (_) {
      show("login");
      return false;
    }
  }

  function card(title, value) {
    return `<div class="card"><h3>${escapeHtml(title)}</h3><div class="val">${escapeHtml(String(value))}</div></div>`;
  }

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  async function loadDashboard() {
    const d = await api("/admin/api/dashboard");
    $("dash-meta").innerHTML = [
      card("Agent", d.agent_status),
      card("Version", d.version),
      card("OS", d.os),
      card("Hostname", d.hostname || "—"),
      card("Uptime", d.uptime_human),
      card("NVML", d.nvml_status),
      card("GPU count", d.gpu_count),
      card("Collector", d.telemetry_http),
      card("Service", d.service),
    ].join("");
    const gpus = d.gpus || [];
    if (!gpus.length) {
      $("dash-gpus").innerHTML = `<p class="muted">${escapeHtml(d.gpus_note || "No GPUs")}</p>`;
      return;
    }
    $("dash-gpus").innerHTML = gpus.map((g) => `
      <div class="card" style="margin-bottom:0.75rem">
        <h3>#${g.index} ${escapeHtml(g.name)}</h3>
        <div class="grid">
          ${card("Temp °C", g.temperature_c ?? "—")}
          ${card("Power W", g.power_w ?? "—")}
          ${card("Util %", g.utilization_pct ?? "—")}
          ${card("Fan %", g.fan_pct ?? "—")}
          ${card("PCI", g.pci_address || "—")}
        </div>
      </div>`).join("");
  }

  async function loadSettings() {
    const c = await api("/admin/api/config");
    $("cfg-hostname").value = c.host_name || "";
    $("cfg-poll").value = c.gui.poll_interval_secs;
    $("cfg-bind").value = c.bind;
    $("cfg-port").value = c.port;
    $("cfg-gui-bind").value = c.gui.bind;
    $("cfg-gui-port").value = c.gui.port;
    $("cfg-path").value = c.config_path;
  }

  async function loadDiagnostics() {
    const d = await api("/admin/api/diagnostics");
    $("diag-out").textContent = JSON.stringify(d, null, 2);
  }

  function schedulePoll() {
    if (state.timer) clearInterval(state.timer);
    state.timer = setInterval(() => {
      if (document.visibilityState === "hidden") return;
      if (state.view === "dashboard") loadDashboard().catch(() => {});
    }, state.pollMs);
  }

  $("login-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    $("login-error").hidden = true;
    try {
      const body = await api("/admin/api/login", {
        method: "POST",
        body: JSON.stringify({ password: $("password").value }),
      });
      state.csrf = body.csrf_token;
      $("password").value = "";
      show("dashboard");
      await loadDashboard();
      schedulePoll();
    } catch (err) {
      $("login-error").hidden = false;
      $("login-error").textContent = err.message || "Login failed";
    }
  });

  $("btn-logout").addEventListener("click", async () => {
    try {
      await api("/admin/api/logout", { method: "POST", body: "{}" });
    } catch (_) {}
    state.csrf = null;
    if (state.timer) clearInterval(state.timer);
    show("login");
  });

  document.querySelectorAll("#nav button[data-view]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const view = btn.getAttribute("data-view");
      show(view);
      try {
        if (view === "dashboard") await loadDashboard();
        if (view === "settings") await loadSettings();
        if (view === "diagnostics") await loadDiagnostics();
      } catch (err) {
        if (err.status === 401) show("login");
      }
    });
  });

  $("settings-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    $("settings-msg").hidden = true;
    $("settings-error").hidden = true;
    try {
      const res = await api("/admin/api/config", {
        method: "PUT",
        body: JSON.stringify({
          host_name: $("cfg-hostname").value,
          gui_poll_interval_secs: Number($("cfg-poll").value),
        }),
      });
      $("settings-msg").hidden = false;
      $("settings-msg").textContent = res.restart_required
        ? "Saved. Service restart required for some changes."
        : "Saved.";
      if (res.poll_interval_secs) {
        state.pollMs = res.poll_interval_secs * 1000;
        schedulePoll();
      }
    } catch (err) {
      $("settings-error").hidden = false;
      $("settings-error").textContent = err.message || "Save failed";
    }
  });

  $("btn-refresh-diag").addEventListener("click", () => loadDiagnostics().catch(() => {}));
  $("btn-export-diag").addEventListener("click", async () => {
    const d = await api("/admin/api/diagnostics");
    const blob = new Blob([JSON.stringify(d, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "exogpuagent-diagnostics-redacted.json";
    a.click();
    URL.revokeObjectURL(a.href);
  });

  (async () => {
    if (await refreshSession()) {
      show("dashboard");
      await loadDashboard();
      schedulePoll();
    } else {
      show("login");
    }
  })();
})();
