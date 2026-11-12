const DEFAULTS = { interceptDownloads: false, mediaPanel: true };
const intercept = document.getElementById("interceptDownloads");
const mediaPanel = document.getElementById("mediaPanel");
const status = document.getElementById("status");

async function load() {
  const values = await chrome.storage.local.get(DEFAULTS);
  intercept.checked = !!values.interceptDownloads;
  mediaPanel.checked = values.mediaPanel !== false;
}
async function save() {
  await chrome.storage.local.set({
    interceptDownloads: intercept.checked,
    mediaPanel: mediaPanel.checked,
  });
}
intercept.addEventListener("change", save);
mediaPanel.addEventListener("change", save);
document.getElementById("test").addEventListener("click", () => {
  status.className = "status";
  status.textContent = "Checking…";
  chrome.runtime.sendMessage({ type: "rustget-ping" }, (reply) => {
    if (chrome.runtime.lastError || !reply?.ok) {
      status.className = "status bad";
      status.textContent = `Not connected: ${chrome.runtime.lastError?.message || reply?.error || "start rustget serve"}`;
    } else {
      status.className = "status ok";
      status.textContent = `Connected to ${reply.data?.api || "RustGet"} (${reply.data?.version || "?"})`;
    }
  });
});
load();
