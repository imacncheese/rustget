/* RustGet web UI. A thin, dependency-free client over the local API. */
(() => {
  "use strict";

  const ROW_H = 52;
  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => Array.from(document.querySelectorAll(sel));

  const state = {
    token: "",
    downloads: [],
    live: new Map(),
    filter: "all",
    category: "",
    categories: [],
    search: "",
    sortKey: "created_at_ms",
    sortDir: -1,
    selected: null,
    lastRange: "",
    ws: null,
    reconnectDelay: 500,
    toastTimer: null,
    notify: localStorage.getItem("rustget.notify") !== "off",
  };

  // ---------- formatting ----------
  const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];
  function bytes(n) {
    if (n === null || n === undefined || Number.isNaN(n)) return "—";
    let value = Number(n), i = 0;
    while (value >= 1024 && i < UNITS.length - 1) { value /= 1024; i++; }
    return `${value >= 100 || i === 0 ? value.toFixed(0) : value.toFixed(1)} ${UNITS[i]}`;
  }
  function speed(n) {
    if (!n) return "—";
    return `${bytes(n)}/s`;
  }
  function eta(record) {
    if (!record.total_size || !record.speed_bps || record.status === "completed") return "—";
    const left = Math.max(0, record.total_size - record.downloaded);
    const seconds = Math.round(left / record.speed_bps);
    if (!Number.isFinite(seconds) || seconds <= 0) return "—";
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  }
  function date(ms) {
    if (!ms) return "—";
    const d = new Date(Number(ms));
    const now = Date.now();
    const diff = Math.max(0, now - d.getTime());
    if (diff < 60_000) return "just now";
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }
  function progressRatio(record) {
    if (!record || !record.total_size) return 0;
    return Math.max(0, Math.min(1, (record.downloaded || 0) / record.total_size));
  }
  function statusLabel(s) {
    return ({ finalizing: "finishing", downloading: "downloading" })[s] || s;
  }
  function esc(value) {
    return String(value ?? "").replace(/[&<>"']/g, (ch) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[ch]);
  }
  function iconFor(name) {
    const ext = (name.split(".").pop() || "").toLowerCase();
    if (["mp4", "mkv", "webm", "avi", "mov"].includes(ext)) return "▶";
    if (["mp3", "flac", "wav", "m4a", "ogg"].includes(ext)) return "♪";
    if (["zip", "7z", "rar", "tar", "gz", "xz"].includes(ext)) return "▣";
    if (["pdf", "doc", "docx", "txt", "md"].includes(ext)) return "≡";
    if (["exe", "msi", "dmg", "deb", "rpm", "appimage"].includes(ext)) return "⚙";
    if (["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext)) return "▧";
    return "⇩";
  }

  // ---------- API ----------
  function apiBase() { return localStorage.getItem("rustget.api") || ""; }
  async function api(path, options = {}) {
    const headers = Object.assign({}, options.headers || {});
    if (state.token) headers.Authorization = `Bearer ${state.token}`;
    if (options.body) headers["Content-Type"] = "application/json";
    const response = await fetch(apiBase() + path, { ...options, headers });
    if (response.status === 401) {
      openSettings("The API token is missing or invalid.");
      throw new Error("Unauthorized");
    }
    const text = await response.text();
    let body = null;
    try { body = text ? JSON.parse(text) : null; } catch { body = text; }
    if (!response.ok) {
      throw new Error((body && (body.error || body.message)) || `HTTP ${response.status}`);
    }
    return body;
  }

  async function load() {
    if (!state.token) {
      renderAll();
      return;
    }
    try {
      const data = await api("/api/v1/downloads");
      state.downloads = Array.isArray(data) ? data : (data.downloads || []);
      try {
        const categories = await api("/api/v1/categories");
        state.categories = categories.categories || [];
      } catch { state.categories = []; }
      renderAll();
      connectWs();
    } catch (error) {
      if (String(error.message) !== "Unauthorized") toast(error.message, true);
      renderAll();
    }
  }

  // ---------- websocket ----------
  function connectWs() {
    if (state.ws) { state.ws.onclose = null; state.ws.close(); state.ws = null; }
    if (!state.token) return;
    const base = apiBase() || location.origin;
    const url = new URL("/api/v1/ws", base);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    url.searchParams.set("token", state.token);
    try {
      const socket = new WebSocket(url);
      state.ws = socket;
      socket.onopen = () => { state.reconnectDelay = 500; };
      socket.onmessage = (event) => {
        let message;
        try { message = JSON.parse(event.data); } catch { return; }
        handleEvent(message);
      };
      socket.onclose = () => {
        state.ws = null;
        setTimeout(connectWs, state.reconnectDelay);
        state.reconnectDelay = Math.min(15000, state.reconnectDelay * 1.7);
      };
    } catch {
      setTimeout(connectWs, state.reconnectDelay);
    }
  }

  function handleEvent(message) {
    if (!message || !message.type) return;
    if (message.type === "updated") {
      const incoming = message;
      const index = state.downloads.findIndex((item) => item.id === incoming.id);
      if (index >= 0) state.downloads[index] = incoming;
      else state.downloads.unshift(incoming);
      renderAll();
      if (state.selected === incoming.id) renderDetails();
    } else if (message.type === "removed") {
      state.downloads = state.downloads.filter((item) => item.id !== message.id);
      state.live.delete(message.id);
      if (state.selected === message.id) { state.selected = null; renderDetails(); }
      renderAll();
    } else if (message.type === "progress") {
      state.live.set(message.id, message.progress || message);
      if (state.selected === message.id) renderDetails();
    } else if (message.type === "finished") {
      const record = state.downloads.find((item) => item.id === message.id);
      if (message.status === "completed" && record) {
        toast(`Download complete: ${record.filename}`);
        notify(record);
      } else if (message.status === "failed" && record) {
        toast(`Download failed: ${record.filename}`, true);
      }
      load();
    }
  }

  function notify(record) {
    if (!state.notify || !("Notification" in window)) return;
    if (Notification.permission === "granted") {
      new Notification("RustGet — download complete", { body: record.filename });
    }
  }

  // ---------- filtering/rendering ----------
  function active(record) {
    return ["probing", "downloading", "finalizing"].includes(record.status);
  }
  function matchesFilter(record) {
    if (state.category && record.category !== state.category) return false;
    switch (state.filter) {
      case "downloading": return active(record);
      case "queued": return record.status === "queued";
      case "paused": return record.status === "paused";
      case "completed": return record.status === "completed";
      case "failed": return record.status === "failed";
      default: return true;
    }
  }
  function filtered() {
    const needle = state.search.trim().toLowerCase();
    let items = state.downloads.filter((record) => matchesFilter(record));
    if (needle) {
      items = items.filter((record) =>
        `${record.filename} ${record.url} ${record.category}`.toLowerCase().includes(needle));
    }
    const key = state.sortKey;
    items.sort((a, b) => {
      let left, right;
      if (key === "progress") { left = progressRatio(a); right = progressRatio(b); }
      else if (key === "filename" || key === "status" || key === "category") {
        left = String(a[key] || "").toLowerCase(); right = String(b[key] || "").toLowerCase();
      } else { left = Number(a[key] || 0); right = Number(b[key] || 0); }
      if (left < right) return -1 * state.sortDir;
      if (left > right) return 1 * state.sortDir;
      return 0;
    });
    return items;
  }

  function renderAll() {
    renderStats();
    renderCounts();
    renderCategories();
    renderList();
    renderDetails();
  }

  function renderCategories() {
    const nav = $("#categoryNav");
    if (!nav) return;
    const names = state.categories.map((category) => category.name);
    for (const record of state.downloads) {
      if (record.category && !names.includes(record.category)) names.push(record.category);
    }
    names.sort((a, b) => a.localeCompare(b));
    nav.innerHTML = "";
    const all = document.createElement("button");
    all.className = `nav-item${state.category ? "" : " active"}`;
    all.innerHTML = `<span class="dot"></span>All categories`;
    all.onclick = () => { state.category = ""; state.lastRange = ""; renderAll(); };
    nav.appendChild(all);
    for (const name of names) {
      const count = state.downloads.filter((record) => record.category === name).length;
      const button = document.createElement("button");
      button.className = `nav-item${state.category === name ? " active" : ""}`;
      button.innerHTML = `<span class="dot"></span><span class="category-name"></span><span class="count">${count}</span>`;
      button.querySelector(".category-name").textContent = name;
      button.onclick = () => {
        state.category = state.category === name ? "" : name;
        state.filter = "all";
        state.lastRange = "";
        renderAll();
      };
      nav.appendChild(button);
    }
  }

  function renderStats() {
    const running = state.downloads.filter(active);
    const done = state.downloads.filter((item) => item.status === "completed");
    $("#statActive").textContent = running.length;
    $("#statSpeed").textContent = speed(running.reduce((sum, item) => sum + (item.speed_bps || 0), 0));
    $("#statDone").textContent = done.length;
    $("#statBytes").textContent = bytes(done.reduce((sum, item) => sum + (item.total_size || 0), 0));
  }

  function renderCounts() {
    const counts = { all: state.downloads.length, downloading: 0, queued: 0, paused: 0, completed: 0, failed: 0 };
    for (const item of state.downloads) {
      if (active(item)) counts.downloading++;
      else if (counts[item.status] !== undefined) counts[item.status]++;
    }
    for (const [key, value] of Object.entries(counts)) {
      const node = document.querySelector(`[data-count="${key}"]`);
      if (node) node.textContent = value;
    }
    $$(".nav-item").forEach((button) => button.classList.toggle("active", button.dataset.filter === state.filter));
    $$(".col").forEach((button) => {
      const marker = button.querySelector(".sort");
      if (button.dataset.sort === state.sortKey) marker.textContent = state.sortDir > 0 ? "↑" : "↓";
      else marker.textContent = "";
    });
  }

  function rowHtml(record) {
    const ratio = progressRatio(record);
    const percent = Math.round(ratio * 100);
    const done = record.status === "completed";
    const failed = record.status === "failed";
    return `
      <div class="name-cell">
        <div class="type-icon">${iconFor(record.filename)}</div>
        <div class="name-text"><b title="${esc(record.filename)}">${esc(record.filename)}</b><small>${esc(record.category || "General")} · ${date(record.created_at_ms)}</small></div>
      </div>
      <div class="size-cell">${record.total_size ? bytes(record.total_size) : bytes(record.downloaded)}</div>
      <div class="progress-cell">
        <div class="track ${done ? "done" : ""} ${failed ? "failed" : ""}"><i style="width:${percent}%"></i></div>
        <span>${done ? "100%" : `${percent}%`}</span>
      </div>
      <div class="speed-cell">${active(record) ? speed(record.speed_bps) : "—"}</div>
      <div><span class="badge ${esc(record.status)}">${statusLabel(record.status)}</span></div>`;
  }

  function renderList() {
    const items = filtered();
    const viewport = $("#listViewport");
    const spacer = $("#listSpacer");
    const rows = $("#listRows");
    const empty = $("#empty");
    empty.hidden = items.length > 0 || state.downloads.length > 0 ? items.length > 0 : false;
    if (items.length === 0) {
      rows.innerHTML = "";
      spacer.style.height = "0px";
      empty.hidden = false;
      const title = empty.querySelector("h2");
      const copy = empty.querySelector("p");
      if (state.downloads.length === 0) {
        title.textContent = "No downloads yet";
        copy.textContent = "Paste a link or add one to get started.";
      } else {
        title.textContent = "No matching downloads";
        copy.textContent = "Try clearing the search or choosing another filter.";
      }
      return;
    }
    empty.hidden = true;
    spacer.style.height = `${items.length * ROW_H}px`;
    const top = viewport.scrollTop;
    const height = viewport.clientHeight || 600;
    const start = Math.max(0, Math.floor(top / ROW_H) - 4);
    const end = Math.min(items.length, Math.ceil((top + height) / ROW_H) + 4);
    const range = `${start}:${end}:${items.length}:${state.sortKey}:${state.sortDir}:${state.filter}:${state.search}`;
    if (range === state.lastRange) {
      rows.querySelectorAll(".row").forEach((row) => {
        row.classList.toggle("selected", row.dataset.id === state.selected);
      });
      return;
    }
    state.lastRange = range;
    const fragment = document.createDocumentFragment();
    for (let index = start; index < end; index++) {
      const record = items[index];
      const row = document.createElement("div");
      row.className = `row${record.id === state.selected ? " selected" : ""}`;
      row.dataset.id = record.id;
      row.style.position = "absolute";
      row.style.top = `${index * ROW_H}px`;
      row.style.left = "0";
      row.style.right = "0";
      row.setAttribute("role", "row");
      row.tabIndex = -1;
      row.innerHTML = rowHtml(record);
      fragment.appendChild(row);
    }
    rows.innerHTML = "";
    rows.appendChild(fragment);
  }

  function renderDetails() {
    const record = state.downloads.find((item) => item.id === state.selected);
    $("#detailsEmpty").hidden = !!record;
    $("#detailsBody").hidden = !record;
    if (!record) return;
    const live = state.live.get(record.id);
    $("#detailName").textContent = record.filename;
    $("#detailName").title = record.filename;
    const badge = $("#detailStatus");
    badge.textContent = statusLabel(record.status);
    badge.className = `badge ${record.status}`;
    const ratio = progressRatio(record);
    $("#detailBar").style.width = `${Math.round(ratio * 100)}%`;
    $("#detailBar").style.background = record.status === "failed" ? "var(--danger)" : "";
    $("#detailSize").textContent = `${bytes(record.downloaded)} / ${record.total_size ? bytes(record.total_size) : "unknown"}`;
    $("#detailSpeed").textContent = active(record) ? speed(record.speed_bps) : "—";
    $("#detailEta").textContent = eta(record);
    $("#detailAdded").textContent = date(record.created_at_ms);
    $("#detailPath").textContent = record.destination || "—";
    $("#detailPath").title = record.destination || "";
    $("#detailUrl").textContent = record.final_url || record.url || "—";
    $("#detailUrl").title = record.final_url || record.url || "";
    const openConnections = live?.active_connections ?? (active(record) ? record.connections : 0);
    $("#detailConns").textContent = `${openConnections} active`;
    const errorRow = $("#detailErrorRow");
    errorRow.hidden = !record.error;
    $("#detailError").textContent = record.error || "";
    renderSegments(record, live);
    const pauseable = active(record) || record.status === "queued";
    const resumable = ["paused", "queued", "failed"].includes(record.status);
    $("#pauseBtn").disabled = !pauseable;
    $("#resumeBtn").disabled = !resumable && record.status !== "paused";
    $("#retryBtn").hidden = record.status !== "failed";
    $(".actions").style.opacity = "1";
  }

  function renderSegments(record, live) {
    const container = $("#detailSegments");
    const segments = live?.segments || [];
    container.innerHTML = "";
    if (!segments.length) {
      const segment = document.createElement("div");
      segment.className = "seg";
      segment.innerHTML = `<i style="width:${Math.round(progressRatio(record) * 100)}%"></i>`;
      container.appendChild(segment);
      return;
    }
    for (const item of segments) {
      const length = Math.max(1, item.end - item.start + 1);
      const written = Math.max(0, Math.min(length, item.written - item.start));
      const segment = document.createElement("div");
      segment.className = `seg ${item.state === "complete" ? "done" : ""} ${item.state === "pending" ? "idle" : ""}`;
      segment.style.flexGrow = String(length);
      segment.title = `#${item.id} · ${bytes(written)} / ${bytes(length)}`;
      segment.innerHTML = `<i style="width:${Math.round((written / length) * 100)}%"></i>`;
      container.appendChild(segment);
    }
  }

  // ---------- actions ----------
  async function action(path, method = "POST") {
    try {
      const record = await api(path, { method });
      if (record && record.id) {
        const index = state.downloads.findIndex((item) => item.id === record.id);
        if (index >= 0) state.downloads[index] = record;
        renderAll();
      }
      return record;
    } catch (error) {
      toast(error.message, true);
      return null;
    }
  }

  async function pauseSelected() {
    if (!state.selected) return;
    await action(`/api/v1/downloads/${state.selected}/pause`);
  }
  async function resumeSelected() {
    if (!state.selected) return;
    await action(`/api/v1/downloads/${state.selected}/resume`);
  }
  async function retrySelected() {
    if (!state.selected) return;
    await action(`/api/v1/downloads/${state.selected}/retry`);
  }
  async function removeSelected() {
    if (!state.selected) return;
    const record = state.downloads.find((item) => item.id === state.selected);
    if (!record) return;
    const deleteFiles = confirm(`Remove "${record.filename}"?\n\nOK also deletes the downloaded file and partial data.`);
    try {
      await api(`/api/v1/downloads/${state.selected}?delete_files=${deleteFiles}`, { method: "DELETE" });
      state.downloads = state.downloads.filter((item) => item.id !== state.selected);
      state.live.delete(state.selected);
      state.selected = null;
      renderAll();
      toast("Download removed");
    } catch (error) {
      toast(error.message, true);
    }
  }
  async function openSelected() {
    if (!state.selected) return;
    try { await api(`/api/v1/downloads/${state.selected}/open`, { method: "POST" }); }
    catch (error) { toast(error.message, true); }
  }

  function select(id) {
    state.selected = id;
    renderList();
    renderDetails();
  }

  // ---------- dialogs ----------
  function openAdd() {
    $("#addForm").reset();
    $("#addConns").value = "8";
    $("#addLimit").value = "0";
    $("#addCategory").value = "General";
    $("#addStart").checked = true;
    $("#addDialog").showModal();
    setTimeout(() => $("#addUrl").focus(), 30);
  }
  function openSettings(message) {
    $("#tokenInput").value = state.token;
    $("#themeSelect").value = localStorage.getItem("rustget.theme") || "system";
    $("#notifySelect").value = state.notify ? "on" : "off";
    $("#settingsDialog").showModal();
    if (message) toast(message, true);
  }

  async function submitAdd(event) {
    event.preventDefault();
    const url = $("#addUrl").value.trim();
    if (!url) return;
    const limitMb = parseFloat($("#addLimit").value || "0");
    const body = {
      url,
      connections: Math.max(1, Math.min(32, parseInt($("#addConns").value || "8", 10))),
      speed_limit: limitMb > 0 ? Math.round(limitMb * 1024 * 1024) : null,
      category: $("#addCategory").value.trim() || "General",
      start: $("#addStart").checked,
    };
    const directory = $("#addDir").value.trim();
    if (directory) body.directory = directory;
    const submit = $("#addSubmit");
    submit.disabled = true;
    submit.textContent = "Adding…";
    try {
      const record = await api("/api/v1/downloads", { method: "POST", body: JSON.stringify(body) });
      const index = state.downloads.findIndex((item) => item.id === record.id);
      if (index >= 0) state.downloads[index] = record; else state.downloads.unshift(record);
      state.selected = record.id;
      $("#addDialog").close();
      renderAll();
      toast(`Added ${record.filename}`);
    } catch (error) {
      toast(error.message, true);
    } finally {
      submit.disabled = false;
      submit.textContent = "Add download";
    }
  }

  function applyTheme() {
    const choice = localStorage.getItem("rustget.theme") || "system";
    const dark = choice === "dark" || (choice === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    $("#themeBtn span").textContent = choice.charAt(0).toUpperCase() + choice.slice(1);
  }
  function cycleTheme() {
    const current = localStorage.getItem("rustget.theme") || "system";
    const next = current === "system" ? "dark" : current === "dark" ? "light" : "system";
    localStorage.setItem("rustget.theme", next);
    applyTheme();
  }

  function toast(message, isError = false) {
    const node = $("#toast");
    node.textContent = message;
    node.className = `toast show${isError ? " error" : ""}`;
    clearTimeout(state.toastTimer);
    state.toastTimer = setTimeout(() => { node.className = "toast"; }, 3800);
  }

  // ---------- events ----------
  function bind() {
    $("#addBtn").addEventListener("click", openAdd);
    $("#emptyAdd").addEventListener("click", openAdd);
    $("#settingsBtn").addEventListener("click", () => openSettings());
    $("#themeBtn").addEventListener("click", cycleTheme);
    $("#addForm").addEventListener("submit", submitAdd);
    $("#pauseBtn").addEventListener("click", pauseSelected);
    $("#resumeBtn").addEventListener("click", resumeSelected);
    $("#retryBtn").addEventListener("click", retrySelected);
    $("#removeBtn").addEventListener("click", removeSelected);
    $("#openBtn").addEventListener("click", openSelected);

    $("#pasteBtn").addEventListener("click", async () => {
      try {
        const text = await navigator.clipboard.readText();
        if (text) $("#addUrl").value = text.trim();
      } catch { toast("Clipboard access was blocked by the browser", true); }
    });

    $$("#nav .nav-item").forEach((button) => button.addEventListener("click", () => {
      state.filter = button.dataset.filter;
      state.category = "";
      state.lastRange = "";
      renderAll();
    }));
    $$(".col").forEach((button) => button.addEventListener("click", () => {
      const key = button.dataset.sort;
      if (state.sortKey === key) state.sortDir *= -1;
      else { state.sortKey = key; state.sortDir = key === "filename" || key === "status" ? 1 : -1; }
      state.lastRange = "";
      renderList();
      renderCounts();
    }));

    let searchTimer;
    $("#search").addEventListener("input", (event) => {
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        state.search = event.target.value;
        state.lastRange = "";
        renderList();
      }, 90);
    });

    $("#listViewport").addEventListener("scroll", () => {
      if (!state.scrollQueued) {
        state.scrollQueued = true;
        requestAnimationFrame(() => { state.scrollQueued = false; renderList(); });
      }
    }, { passive: true });

    $("#listRows").addEventListener("click", (event) => {
      const row = event.target.closest(".row");
      if (row) select(row.dataset.id);
    });
    $("#listRows").addEventListener("dblclick", (event) => {
      const row = event.target.closest(".row");
      if (row) { select(row.dataset.id); openSelected(); }
    });

    $("#settingsForm").addEventListener("submit", (event) => {
      event.preventDefault();
      state.token = $("#tokenInput").value.trim();
      localStorage.setItem("rustget.token", state.token);
      const theme = $("#themeSelect").value;
      localStorage.setItem("rustget.theme", theme);
      state.notify = $("#notifySelect").value === "on";
      localStorage.setItem("rustget.notify", state.notify ? "on" : "off");
      if (state.notify && "Notification" in window && Notification.permission === "default") {
        Notification.requestPermission();
      }
      applyTheme();
      $("#settingsDialog").close();
      load();
      toast("Settings saved");
    });

    document.addEventListener("keydown", (event) => {
      const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "");
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n") { event.preventDefault(); openAdd(); }
      else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") { event.preventDefault(); $("#search").focus(); }
      else if (event.key === "Escape" && !document.querySelector("dialog[open]")) { select(null); }
      else if (!typing && event.key === "Delete" && state.selected) { event.preventDefault(); removeSelected(); }
      else if (!typing && event.code === "Space" && state.selected) {
        event.preventDefault();
        const record = state.downloads.find((item) => item.id === state.selected);
        if (!record) return;
        if (active(record) || record.status === "queued") pauseSelected();
        else resumeSelected();
      } else if (!typing && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
        const items = filtered();
        if (!items.length) return;
        event.preventDefault();
        const index = items.findIndex((item) => item.id === state.selected);
        const next = event.key === "ArrowDown" ? Math.min(items.length - 1, index + 1) : Math.max(0, index - 1);
        select(items[next].id);
        const viewport = $("#listViewport");
        const top = next * ROW_H;
        if (top < viewport.scrollTop) viewport.scrollTop = top;
        if (top + ROW_H > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = top + ROW_H - viewport.clientHeight;
      }
    });

    matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyTheme);
    $("#listViewport").addEventListener("keydown", (event) => {
      if (event.key === "Enter" && state.selected) openSelected();
    });
  }

  // ---------- boot ----------
  function readToken() {
    const hash = new URLSearchParams(location.hash.replace(/^#/, ""));
    const fromHash = hash.get("token");
    if (fromHash) {
      localStorage.setItem("rustget.token", fromHash);
      history.replaceState(null, "", location.pathname + location.search);
    }
    state.token = localStorage.getItem("rustget.token") || "";
  }

  function boot() {
    readToken();
    applyTheme();
    bind();
    renderAll();
    if (!state.token) {
      openSettings("Enter the API token printed by `rustget serve`.");
    } else {
      load();
    }
  }

  boot();
})();
