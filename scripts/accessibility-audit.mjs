// Usage: node scripts/accessibility-audit.mjs <cdp-port> <page-url> <axe.min.js>
import fs from "node:fs";

const [port, url, axePath] = process.argv.slice(2);
if (!port || !url || !axePath) {
  console.error("usage: node accessibility-audit.mjs PORT URL AXE_JS");
  process.exit(2);
}
const axeSource = fs.readFileSync(axePath, "utf8");
let nextId = 1;
const pending = new Map();

async function target() {
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find((item) => item.type === "page");
      if (page) return page;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("no CDP page target");
}

const page = await target();
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    pending.get(message.id)(message);
    pending.delete(message.id);
  }
});

function send(method, params = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, (message) => {
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

await send("Page.enable");
await send("Runtime.enable");
await send("Page.navigate", { url });
await new Promise((resolve) => setTimeout(resolve, 4000));
const injected = await send("Runtime.evaluate", { expression: axeSource });
if (injected.exceptionDetails) throw new Error(JSON.stringify(injected.exceptionDetails));
const result = await send("Runtime.evaluate", {
  expression:
    'axe.run(document, {resultTypes:["violations"]}).then(r => JSON.stringify(r.violations.map(v => ({id:v.id, impact:v.impact, help:v.help, nodes:v.nodes.map(n => n.target)}))))',
  awaitPromise: true,
  returnByValue: true,
});
if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
console.log(result.result.value);
socket.close();
