/* Detects directly playable media and manifests a small floating picker. */
(() => {
  if (window.__rustgetInstalled) return;
  window.__rustgetInstalled = true;

  const seen = new Map();
  let panel = null;
  let panelEnabled = true;
  try {
    chrome.storage.local.get({ mediaPanel: true }, (values) => {
      panelEnabled = values.mediaPanel !== false;
      if (!panelEnabled) { panel?.remove(); panel = null; }
    });
    chrome.storage.onChanged.addListener((changes) => {
      if (changes.mediaPanel) {
        panelEnabled = changes.mediaPanel.newValue !== false;
        if (panelEnabled) render(); else { panel?.remove(); panel = null; }
      }
    });
  } catch { /* storage may be unavailable on restricted pages */ }

  function absolute(url) {
    try { return new URL(url, location.href).href; } catch { return ""; }
  }

  function add(url, label) {
    url = absolute(url);
    if (!url || !/^https?:/i.test(url)) return;
    if (seen.has(url)) return;
    seen.set(url, label || "Media");
    render();
  }

  function inspect() {
    document.querySelectorAll("video, audio").forEach((element) => {
      const label = (element.getAttribute("title") || element.getAttribute("aria-label") || element.currentSrc || "").trim();
      if (element.currentSrc) add(element.currentSrc, label || "Video");
      if (element.src) add(element.src, label || "Video");
      element.querySelectorAll("source").forEach((source) => add(source.src, label || "Video"));
    });
    document.querySelectorAll("a[href]").forEach((anchor) => {
      const href = anchor.href || "";
      if (/\.(m3u8|mpd|mp4|webm|mkv|mp3|m4a|flac)(\?|#|$)/i.test(href)) {
        add(href, anchor.textContent.trim().slice(0, 60) || "Media link");
      }
    });
  }

  function render() {
    if (!panelEnabled || !seen.size) {
      panel?.remove();
      panel = null;
      return;
    }
    if (!panel) {
      panel = document.createElement("div");
      panel.style.cssText = "position:fixed;right:18px;bottom:18px;z-index:2147483647;background:#11141c;color:#eef1f7;border:1px solid #2b3242;border-radius:12px;box-shadow:0 18px 50px rgba(0,0,0,.45);font:13px/1.4 system-ui,sans-serif;padding:10px;width:280px";
      document.documentElement.appendChild(panel);
    }
    panel.innerHTML = "";
    const head = document.createElement("div");
    head.style.cssText = "display:flex;justify-content:space-between;align-items:center;margin-bottom:6px;color:#8c95a8;font-size:11px;text-transform:uppercase;letter-spacing:.08em";
    head.textContent = "RustGet media";
    const close = document.createElement("button");
    close.textContent = "✕";
    close.style.cssText = "background:none;border:0;color:#8c95a8;cursor:pointer;font-size:14px";
    close.onclick = () => { seen.clear(); panel?.remove(); panel = null; };
    head.appendChild(close);
    panel.appendChild(head);
    let count = 0;
    for (const [url, label] of seen) {
      if (count++ >= 5) break;
      const row = document.createElement("div");
      row.style.cssText = "display:flex;gap:8px;align-items:center;padding:6px 0;border-top:1px solid #232936";
      const text = document.createElement("span");
      text.textContent = label;
      text.style.cssText = "flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:#c9d0de";
      text.title = url;
      const button = document.createElement("button");
      button.textContent = "Download";
      button.style.cssText = "border:0;border-radius:7px;background:#ff7a45;color:#1b0e05;font-weight:600;padding:5px 9px;cursor:pointer";
      button.onclick = () => {
        chrome.runtime.sendMessage({ type: "rustget-download", url });
        button.textContent = "Sent";
        button.disabled = true;
      };
      row.append(text, button);
      panel.appendChild(row);
    }
    const note = document.createElement("div");
    note.textContent = `${seen.size} item(s) detected`;
    note.style.cssText = "color:#8c95a8;font-size:10px;margin-top:6px";
    panel.appendChild(note);
  }

  let timer;
  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(inspect, 600);
  }

  inspect();
  new MutationObserver(schedule).observe(document.documentElement, { childList: true, subtree: true });
  setInterval(inspect, 5000);
})();
