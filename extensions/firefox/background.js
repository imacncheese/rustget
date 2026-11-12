/* RustGet Chromium integration. No remote code, no analytics. */
const HOST = "com.rustget.app";
const pending = new Map();

function native(message) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(HOST, message, (response) => {
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
        return;
      }
      if (!response) {
        reject(new Error("RustGet did not answer. Is `rustget serve` running?"));
        return;
      }
      if (response.ok === false) {
        reject(new Error(response.error || "RustGet rejected the request"));
        return;
      }
      resolve(response.data);
    });
  });
}

async function cookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((cookie) => `${cookie.name}=${cookie.value}`).join("; ");
  } catch {
    return "";
  }
}

async function sendDownload(url, referrer) {
  if (!/^https?:/i.test(url || "")) return;
  try {
    const data = await native({
      type: "add",
      url,
      referrer: referrer || "",
      cookie: await cookieHeader(url),
    });
    await flash("↓", "#43d17a", "Added to RustGet");
    if (data && data.id) {
      chrome.storage.local.set({ lastDownload: data.id });
    }
  } catch (error) {
    await flash("!", "#ff5f6d", String(error.message || error));
  }
}

async function flash(text, color, title) {
  await chrome.action.setBadgeBackgroundColor({ color });
  await chrome.action.setBadgeText({ text });
  await chrome.action.setTitle({ title: `RustGet: ${title}` });
  setTimeout(() => chrome.action.setBadgeText({ text: "" }), 3500);
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "rustget-link", title: "Download with RustGet", contexts: ["link", "video", "audio"] });
    chrome.contextMenus.create({ id: "rustget-page", title: "Download page with RustGet", contexts: ["page"] });
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl || (info.menuItemId === "rustget-page" ? info.pageUrl : null);
  if (url) sendDownload(url, tab?.url || info.pageUrl || "");
});

chrome.action.onClicked.addListener(async (tab) => {
  const stored = await chrome.storage.session?.get("detected") || {};
  const first = stored.detected?.[0];
  if (first) sendDownload(first.url, tab?.url || first.pageUrl);
  else sendDownload(tab?.url, "");
});

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  if (message?.type === "rustget-media") {
    const item = { url: message.url, pageUrl: sender.tab?.url || "" };
    chrome.storage.session?.get("detected").then(({ detected = [] }) => {
      if (!detected.some((entry) => entry.url === item.url)) detected.push(item);
      chrome.storage.session?.set({ detected: detected.slice(-20) });
      chrome.action.setBadgeBackgroundColor({ color: "#ff7a45" });
      chrome.action.setBadgeText({ text: String(detected.length) });
    });
    reply({ ok: true });
    return true;
  }
  if (message?.type === "rustget-download") {
    sendDownload(message.url, sender.tab?.url || "").then(
      () => reply({ ok: true }),
      (error) => reply({ ok: false, error: String(error.message || error) }),
    );
    return true;
  }
  return false;
});
