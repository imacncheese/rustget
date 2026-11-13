/* RustGet web UI. A thin, dependency-free client over the local API. */
(() => {
  "use strict";

  const ROW_H = 52;
  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => Array.from(document.querySelectorAll(sel));

  const I18N = window.RUSTGET_I18N || { en: {} };
  let language = localStorage.getItem("rustget.lang");
  if (!language) language = (navigator.language || "").toLowerCase().startsWith("de") ? "de" : "en";

  function t(key) {
    return I18N[language]?.[key] ?? I18N.en?.[key] ?? key;
  }

  function applyLanguage(next = language) {
    language = I18N[next] ? next : "en";
    localStorage.setItem("rustget.lang", language);
    document.documentElement.lang = language;
    for (const node of document.querySelectorAll("[data-i18n]")) {
      node.textContent = t(node.dataset.i18n);
    }
    for (const node of document.querySelectorAll("[data-i18n-placeholder]")) {
      node.placeholder = t(node.dataset.i18nPlaceholder);
    }
    const select = $("#languageSelect");
    if (select) select.value = language;
  }

  const speedHistory = new Map();
  const state = {
    token: "",
    downloads: [],
    live: new Map(),
    filter: "all",
    category: "",
    categories: [],
    schedules: [],
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
  function displayRatio(record) {
    const live = state.live.get(record.id);
    if (live && live.unit_total) {
      return Math.max(0, Math.min(1, (live.unit_completed || 0) / live.unit_total));
    }
    return progressRatio(record);
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
      openWizard();
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
      const progress = message.progress || message;
      state.live.set(message.id, progress);
      const history = speedHistory.get(message.id) || [];
      history.push(Number(progress.speed_bps) || 0);
      if (history.length > 60) history.shift();
      speedHistory.set(message.id, history);
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

  // ---------- scheduler ----------
  const DAY_NAMES = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

  let logTimer = null;
  async function refreshLogs() {
    try {
      const data = await api("/api/v1/logs?limit=300");
      const list = $("#logList");
      list.innerHTML = "";
      if (!(data.entries || []).length) {
        list.textContent = "No log entries yet.";
        return;
      }
      for (const entry of data.entries) {
        const line = document.createElement("span");
        line.className = `log-line ${String(entry.level).toLowerCase()}`;
        const time = new Date(entry.timestamp_ms).toLocaleTimeString();
        line.innerHTML = `<span class="lvl">${esc(entry.level)}</span><span class="src">${esc(entry.target)}</span> `;
        line.append(`${time} ${entry.message}`);
        list.appendChild(line);
      }
      list.scrollTop = list.scrollHeight;
    } catch (error) {
      $("#logList").textContent = error.message;
    }
  }
  async function openLogs() {
    $("#logsDialog").showModal();
    await refreshLogs();
    clearInterval(logTimer);
    logTimer = setInterval(refreshLogs, 2000);
  }
  function closeLogs() {
    clearInterval(logTimer);
    logTimer = null;
    $("#logsDialog").close();
  }

  async function openSchedules() {
    $("#schedulesDialog").showModal();
    try {
      const data = await api("/api/v1/schedules");
      state.schedules = data.schedules || [];
      renderSchedules();
    } catch (error) {
      toast(error.message, true);
    }
  }

  function clock(minutes) {
    if (minutes === null || minutes === undefined) return "—";
    return `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
  }

  function dayList(mask) {
    return DAY_NAMES.filter((_, index) => mask & (1 << index)).join(", ");
  }

  function renderSchedules() {
    const week = $("#schedWeek");
    week.innerHTML = "";
    for (let day = 0; day < 7; day++) {
      const row = document.createElement("div");
      row.className = "sched-day";
      const label = document.createElement("span");
      label.textContent = DAY_NAMES[day];
      const track = document.createElement("div");
      track.className = "sched-track";
      for (const schedule of state.schedules) {
        if (!(schedule.days & (1 << day))) continue;
        const start = schedule.start_min ?? 0;
        const stop = schedule.stop_min ?? 1440;
        if (start < stop) addBlock(track, start, stop, schedule.name);
        else if (start > stop) {
          addBlock(track, start, 1440, schedule.name);
          addBlock(track, 0, stop, schedule.name);
        } else {
          addBlock(track, 0, 1440, schedule.name);
        }
      }
      row.append(label, track);
      week.appendChild(row);
    }
    const list = $("#schedList");
    list.innerHTML = "";
    if (!state.schedules.length) {
      const empty = document.createElement("p");
      empty.className = "hint";
      empty.textContent = "No schedules. The daemon always allows downloads.";
      list.appendChild(empty);
      return;
    }
    for (const schedule of state.schedules) {
      const row = document.createElement("div");
      row.className = "sched-row";
      const name = document.createElement("strong");
      name.textContent = schedule.name;
      const detail = document.createElement("span");
      detail.textContent = `${schedule.start_min === null ? "always" : clock(schedule.start_min)}–${schedule.stop_min === null ? "always" : clock(schedule.stop_min)} · ${dayList(schedule.days)} · ${schedule.action}`;
      const remove = document.createElement("button");
      remove.className = "danger";
      remove.textContent = "Delete";
      remove.onclick = async () => {
        try {
          await api(`/api/v1/schedules/${schedule.id}`, { method: "DELETE" });
          state.schedules = state.schedules.filter((entry) => entry.id !== schedule.id);
          renderSchedules();
        } catch (error) { toast(error.message, true); }
      };
      row.append(name, detail, remove);
      list.appendChild(row);
    }
  }

  function addBlock(track, start, stop, label) {
    const block = document.createElement("div");
    block.className = "sched-block";
    block.title = `${label}: ${clock(start)}–${clock(stop)}`;
    block.style.left = `${(start / 1440) * 100}%`;
    block.style.width = `${Math.max(0.4, ((stop - start) / 1440) * 100)}%`;
    track.appendChild(block);
  }

  async function submitSchedule(event) {
    event.preventDefault();
    const start = $("#schedStart").value.trim() || null;
    const stop = $("#schedStop").value.trim() || null;
    if (!start && !stop) { toast("Set a start or stop time", true); return; }
    const days = $$("#schedDays input:checked").reduce((mask, input) => mask | (1 << Number(input.value)), 0);
    if (!days) { toast("Choose at least one day", true); return; }
    try {
      await api("/api/v1/schedules", {
        method: "POST",
        body: JSON.stringify({
          name: $("#schedName").value.trim() || "Schedule",
          start, stop, days,
          action: $("#schedAction").value,
        }),
      });
      $("#scheduleForm").reset();
      $$("#schedDays input").forEach((input, index) => { input.checked = index < 5; });
      const data = await api("/api/v1/schedules");
      state.schedules = data.schedules || [];
      renderSchedules();
      toast("Schedule added");
    } catch (error) { toast(error.message, true); }
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
        `${record.filename} ${record.url} ${record.category} ${(record.tags || []).join(" ")}`.toLowerCase().includes(needle));
    }
    const key = state.sortKey;
    items.sort((a, b) => {
      let left, right;
      if (key === "progress") { left = displayRatio(a); right = displayRatio(b); }
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
    if (!state.selected && state.downloads.length && state.filter === "all" && !state.search) {
      state.selected = state.downloads[0].id;
    }
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
    all.innerHTML = `<span class="dot"></span>${t("nav.allCategories")}`;
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
    const ratio = displayRatio(record);
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
        title.textContent = t("empty.title");
        copy.textContent = t("empty.body");
      } else {
        title.textContent = t("empty.none");
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
    const ratio = displayRatio(record);
    $("#detailBar").style.width = `${Math.round(ratio * 100)}%`;
    $("#detailBar").style.background = record.status === "failed" ? "var(--danger)" : "";
    $("#detailSize").textContent = `${bytes(record.downloaded)} / ${record.total_size ? bytes(record.total_size) : "unknown"}`;
    $("#detailSpeed").textContent = active(record) ? speed(record.speed_bps) : "—";
    renderSpeedChart(record);
    renderTags(record);
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

  function renderTags(record) {
    const container = $("#detailTags");
    container.innerHTML = "";
    if (!(record.tags || []).length) {
      container.textContent = "No tags yet";
      container.style.color = "var(--muted)";
      return;
    }
    container.style.color = "";
    for (const tag of record.tags) {
      const chip = document.createElement("span");
      chip.className = "tag";
      const label = document.createElement("span");
      label.textContent = tag;
      const remove = document.createElement("button");
      remove.type = "button";
      remove.textContent = "×";
      remove.title = `Remove ${tag}`;
      remove.addEventListener("click", () => saveTags(record, record.tags.filter((item) => item !== tag)));
      chip.append(label, remove);
      container.appendChild(chip);
    }
  }

  async function saveTags(record, tags) {
    try {
      const updated = await api(`/api/v1/downloads/${record.id}`, {
        method: "PATCH",
        body: JSON.stringify({ tags }),
      });
      const index = state.downloads.findIndex((item) => item.id === record.id);
      if (index >= 0) state.downloads[index] = updated;
      renderAll();
    } catch (error) { toast(error.message, true); }
  }

  async function addTagFromInput() {
    const input = $("#tagInput");
    const tag = input.value.trim();
    if (!tag || !state.selected) return;
    const record = state.downloads.find((item) => item.id === state.selected);
    if (!record) return;
    await saveTags(record, [...(record.tags || []), tag]);
    input.value = "";
  }

  function renderSpeedChart(record) {
    const canvas = $("#speedChart");
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const width = Math.max(1, Math.floor(rect.width));
    const height = Math.max(1, Math.floor(rect.height));
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    if (canvas.width !== Math.floor(width * dpr) || canvas.height !== Math.floor(height * dpr)) {
      canvas.width = Math.floor(width * dpr);
      canvas.height = Math.floor(height * dpr);
    }
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const history = (speedHistory.get(record.id) || []).slice(-60);
    if (!history.length) history.push(Number(record.speed_bps) || 0);
    while (history.length < 2) history.unshift(history[0]);
    const sorted = [...history].sort((a, b) => a - b);
    const max = Math.max(1, sorted[Math.floor((sorted.length - 1) * 0.95)] || 1);
    const step = width / (history.length - 1);
    ctx.beginPath();
    history.forEach((value, index) => {
      const x = index * step;
      const y = height - 4 - (value / max) * (height - 10);
      if (index === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.lineTo(width, height);
    ctx.lineTo(0, height);
    ctx.closePath();
    const fill = ctx.createLinearGradient(0, 0, 0, height);
    fill.addColorStop(0, "rgba(255,122,69,.34)");
    fill.addColorStop(1, "rgba(255,122,69,0)");
    ctx.fillStyle = fill;
    ctx.fill();
    ctx.beginPath();
    history.forEach((value, index) => {
      const x = index * step;
      const y = height - 4 - (value / max) * (height - 10);
      if (index === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.strokeStyle = "#ff7a45";
    ctx.lineWidth = 1.7;
    ctx.lineJoin = "round";
    ctx.stroke();
    ctx.fillStyle = "rgba(140,149,168,.75)";
    ctx.font = "10px ui-monospace, monospace";
    ctx.fillText(`${speed(max)} peak`, 2, 11);
  }

  function renderSegments(record, live) {
    const container = $("#detailSegments");
    const segments = live?.segments || [];
    container.innerHTML = "";
    if (!segments.length) {
      const segment = document.createElement("div");
      segment.className = "seg";
      segment.innerHTML = `<i style="width:${Math.round(displayRatio(record) * 100)}%"></i>`;
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
  async function changePriority(delta) {
    const record = state.downloads.find((item) => item.id === state.selected);
    if (!record) return;
    try {
      const updated = await api(`/api/v1/downloads/${record.id}`, {
        method: "PATCH",
        body: JSON.stringify({ priority: (record.priority || 0) + delta }),
      });
      const index = state.downloads.findIndex((item) => item.id === updated.id);
      if (index >= 0) state.downloads[index] = updated;
      renderAll();
      toast(`Priority ${updated.priority > 0 ? "+" : ""}${updated.priority}`);
    } catch (error) { toast(error.message, true); }
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
    $("#variantField").hidden = true;
    $("#addVariant").innerHTML = '<option value="">Auto (best)</option>';
    $("#addConns").value = "8";
    $("#addLimit").value = "0";
    $("#addCategory").value = "General";
    $("#addStart").checked = true;
    $("#addDialog").showModal();
    setTimeout(() => $("#addUrl").focus(), 30);
  }
  function wizardStep(step) {
    $$("#wizardDialog .wizard-step").forEach((panel) => {
      panel.hidden = Number(panel.dataset.step) !== step;
    });
    if (step === 2) setTimeout(() => $("#wizardToken").focus(), 30);
  }
  function openWizard() {
    $("#wizardError").hidden = true;
    $("#wizardToken").value = "";
    wizardStep(1);
    $("#wizardDialog").showModal();
  }
  async function finishWizard() {
    const token = $("#wizardToken").value.trim();
    const error = $("#wizardError");
    if (token.length < 16) {
      error.textContent = "That does not look like a RustGet token.";
      error.hidden = false;
      return;
    }
    try {
      const response = await fetch("/api/v1/stats", {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!response.ok) throw new Error(`token rejected (${response.status})`);
      state.token = token;
      localStorage.setItem("rustget.token", token);
      $("#wizardDialog").close();
      await load();
      toast("RustGet connected");
    } catch (cause) {
      error.textContent = cause.message;
      error.hidden = false;
    }
  }
  function openSettings(message) {
    $("#tokenInput").value = state.token;
    $("#themeSelect").value = localStorage.getItem("rustget.theme") || "system";
    $("#languageSelect").value = language;
    $("#settingsSearch").value = "";
    for (const node of document.querySelectorAll(
      "#settingsForm .field, #settingsForm details, #settingsForm .two",
    )) {
      node.hidden = false;
    }
    $("#notifySelect").value = state.notify ? "on" : "off";
    $("#settingsDialog").showModal();
    if (message) toast(message, true);
  }

  async function exportConfig() {
    try {
      const response = await fetch("/api/v1/export", {
        headers: { Authorization: `Bearer ${state.token}` },
      });
      if (!response.ok) throw new Error(`export failed (${response.status})`);
      const blob = await response.blob();
      const link = document.createElement("a");
      link.href = URL.createObjectURL(blob);
      link.download = `rustget-config-${new Date().toISOString().slice(0, 10)}.json`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(link.href), 1000);
      toast("Configuration exported");
    } catch (error) { toast(error.message, true); }
  }

  async function importConfig(file) {
    try {
      const bundle = JSON.parse(await file.text());
      const result = await api("/api/v1/import", {
        method: "POST",
        body: JSON.stringify(bundle),
      });
      toast(`Imported ${result.imported} download(s), ${result.settings} setting(s)`);
      await loadAll();
    } catch (error) { toast(error.message, true); }
  }

  async function probeAdd() {
    const url = $("#addUrl").value.trim();
    const hint = $("#probeHint");
    if (!/^https?:\/\//i.test(url)) {
      hint.textContent = "Enter an http(s) URL first.";
      return;
    }
    hint.textContent = "Checking…";
    try {
      const probe = await api("/api/v1/probe", {
        method: "POST",
        body: JSON.stringify({ url }),
      });
      const size = probe.total_size ? bytes(probe.total_size) : "unknown size";
      const ranges = probe.range_supported ? "resumable" : "single stream";
      hint.textContent = `${probe.filename} · ${size} · ${ranges}`;
      if (/\.(m3u8|mpd)(\?|$)/i.test(url)) {
        try {
          const media = await api("/api/v1/media/probe", {
            method: "POST",
            body: JSON.stringify({ url }),
          });
          const select = $("#addVariant");
          select.innerHTML = '<option value="">Auto (best)</option>';
          for (const variant of media.variants || []) {
            const option = document.createElement("option");
            option.value = String(variant.index);
            option.textContent = variant.label;
            select.appendChild(option);
          }
          $("#variantField").hidden = !(media.variants || []).length;
        } catch {
          $("#variantField").hidden = true;
        }
      } else {
        $("#variantField").hidden = true;
      }
    } catch (error) {
      hint.textContent = error.message;
    }
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
    const mirrors = $("#addMirrors").value.split(/[\s,]+/).map((item) => item.trim()).filter(Boolean);
    if (mirrors.length) body.mirrors = mirrors;
    const variant = $("#addVariant").value;
    if (variant !== "") body.media_variant = Number(variant);
    const tags = $("#addTags").value.split(",").map((item) => item.trim()).filter(Boolean);
    if (tags.length) body.tags = tags;
    const proxy = $("#addProxy").value.trim();
    if (proxy) body.proxy = proxy;
    const clientCert = $("#addClientCert").value.trim();
    if (clientCert) body.client_cert = clientCert;
    const digestUser = $("#addDigestUser").value.trim();
    const digestPass = $("#addDigestPass").value;
    if (digestUser && digestPass) body.digest_auth = [digestUser, digestPass];
    const basicUser = $("#addBasicUser").value.trim();
    const basicPass = $("#addBasicPass").value;
    if (basicUser && basicPass) body.basic_auth = [basicUser, basicPass];
    if ($("#addYtdlp").checked) body.use_ytdlp = true;
    if ($("#addCookieJar").checked) body.use_cookie_jar = true;
    if ($("#addExtract").checked) body.extract = true;
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
    $("#schedulesBtn").addEventListener("click", openSchedules);
    $("#logsBtn").addEventListener("click", openLogs);
    $("#logsClose").addEventListener("click", closeLogs);
    $("#logsRefresh").addEventListener("click", refreshLogs);
    $("#wizardNext").addEventListener("click", () => wizardStep(2));
    $("#wizardBack").addEventListener("click", () => wizardStep(1));
    $("#wizardFinish").addEventListener("click", finishWizard);
    $("#wizardToken").addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); finishWizard(); }
    });
    $("#schedulesClose").addEventListener("click", () => $("#schedulesDialog").close());
    $("#scheduleForm").addEventListener("submit", submitSchedule);
    $("#themeBtn").addEventListener("click", cycleTheme);
    $("#settingsSearch").addEventListener("input", (event) => {
      const query = event.target.value.trim().toLowerCase();
      for (const node of document.querySelectorAll(
        "#settingsForm .field, #settingsForm details, #settingsForm .two",
      )) {
        node.hidden = Boolean(query) && !node.textContent.toLowerCase().includes(query);
      }
    });
    $("#languageSelect").addEventListener("change", (event) => {
      applyLanguage(event.target.value);
      renderCategories();
    });
    $("#addForm").addEventListener("submit", submitAdd);
    $("#pauseBtn").addEventListener("click", pauseSelected);
    $("#resumeBtn").addEventListener("click", resumeSelected);
    $("#retryBtn").addEventListener("click", retrySelected);
    $("#removeBtn").addEventListener("click", removeSelected);
    $("#openBtn").addEventListener("click", openSelected);
    $("#upBtn").addEventListener("click", () => changePriority(1));
    $("#downBtn").addEventListener("click", () => changePriority(-1));

    $("#pasteBtn").addEventListener("click", async () => {
      try {
        const text = await navigator.clipboard.readText();
        if (text) $("#addUrl").value = text.trim();
      } catch { toast("Clipboard access was blocked by the browser", true); }
    });
    $("#probeBtn").addEventListener("click", probeAdd);
    $("#tagAdd").addEventListener("click", addTagFromInput);
    $("#tagInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); addTagFromInput(); }
    });
    $("#exportConfigBtn").addEventListener("click", exportConfig);
    $("#importConfigBtn").addEventListener("click", () => $("#importConfigFile").click());
    $("#importConfigFile").addEventListener("change", (event) => {
      const file = event.target.files?.[0];
      if (file) importConfig(file);
      event.target.value = "";
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

    const hasDroppedLink = (event) =>
      Array.from(event.dataTransfer?.types || []).some(
        (type) => type === "text/uri-list" || type === "text/plain" || type === "Files",
      );
    document.addEventListener("dragover", (event) => {
      if (!hasDroppedLink(event)) return;
      event.preventDefault();
      document.body.classList.add("drop-target");
    });
    document.addEventListener("dragleave", (event) => {
      if (!event.relatedTarget) document.body.classList.remove("drop-target");
    });
    document.addEventListener("drop", (event) => {
      document.body.classList.remove("drop-target");
      event.preventDefault();
      const files = Array.from(event.dataTransfer?.files || []);
      if (files.length > 0) {
        (async () => {
          let imported = 0;
          for (const file of files) {
            const form = new FormData();
            form.append("file", file, file.name);
            const headers = state.token ? { Authorization: `Bearer ${state.token}` } : {};
            const response = await fetch(apiBase() + "/api/v1/import-file", {
              method: "POST",
              headers,
              body: form,
            });
            if (!response.ok) {
              const detail = await response.text();
              toast(`Could not import ${file.name}: ${detail}`, true);
              return;
            }
            imported += 1;
          }
          toast(`Imported ${imported} local file${imported === 1 ? "" : "s"}`);
          await load();
        })();
        return;
      }
      const raw = event.dataTransfer?.getData("text/uri-list") || event.dataTransfer?.getData("text/plain") || "";
      const links = raw.split(/\r?\n/).map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
      const url = links.find((line) => /^https?:\/\//i.test(line));
      if (!url) {
        toast("Drop an http(s) link or local file", true);
        return;
      }
      openAdd();
      $("#addUrl").value = url;
      $("#probeHint").textContent = links.length > 1 ? `${links.length} links dropped; added the first` : "Link dropped — press Check or Add.";
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

  window.rustgetExternalUrl = (url) => {
    openAdd();
    $("#addUrl").value = url;
    $("#probeHint").textContent = "Clipboard link detected — press Check or Add.";
  };

  function boot() {
    readToken();
    const requested = new URLSearchParams(location.search).get("lang");
    if (requested && I18N[requested]) {
      localStorage.setItem("rustget.lang", requested);
      language = requested;
    }
    applyTheme();
    applyLanguage();
    bind();
    renderAll();
    if (!state.token) {
      openWizard();
    } else {
      const token = state.token;
      fetch("/api/v1/stats", { headers: { Authorization: `Bearer ${token}` } })
        .then((response) => response.ok ? load() : openWizard())
        .catch(() => openWizard());
    }
  }

  boot();
})();
