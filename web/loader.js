import { installTethered } from "./tethered.mjs";
installTethered();
import { LOCALES, STRINGS } from "./i18n-data.js";
import { negotiate, fontMatches } from "./i18n.js";

// Boot Schist in the browser.
//
// The app's wasm is served split into fixed-size chunks (tools/web-build.sh
// cuts them; manifest.json lists them in order) so a ~20 MB module downloads
// over parallel connections with a byte-accurate progress bar, instead of as
// one opaque stall. The icons and fonts the app would natively embed are
// fetched here too — the wasm carries no assets — and handed over on
// `window.__schist_boot` before the module is instantiated, so the Rust side
// reads them synchronously and never needs an async asset path.
//
// The app calls `__schistLoadingDone` once its window is up, and its panic
// hook calls `__schistLoadingFailed`, so a crash during boot reads as an
// error, never as an eternally full progress bar.

const overlay = document.getElementById("schist-loading");
const fill = document.getElementById("schist-loading-fill");
const status = document.getElementById("schist-loading-status");

const preferred = navigator.languages?.length
  ? navigator.languages
  : [navigator.language ?? "en"];
const LANG = negotiate(preferred);
const S = STRINGS[LANG];
document.documentElement.lang = LANG;
document.documentElement.dir = LOCALES[LANG].direction;
document.getElementById("schist-desktop-notice-title").textContent = S.noticeTitle;
document.getElementById("schist-desktop-notice-ok").textContent = S.ok;
{
  // The body keeps its link: put the text back around the same anchor.
  const body = document.getElementById("schist-desktop-notice-body");
  const link = document.getElementById("schist-desktop-notice-link");
  link.textContent = S.noticeLink;
  body.textContent = S.noticeBody + " ";
  body.appendChild(link);
}

// First visit only: say that the desktop app is the fuller Schist, while
// the download runs. OK records the acceptance so it never shows again.
// localStorage can throw (private modes with storage off); a visitor we
// cannot remember just sees the notice each time.
const NOTICE_KEY = "schist.desktop-notice-accepted";
try {
  if (!localStorage.getItem(NOTICE_KEY)) {
    const notice = document.getElementById("schist-desktop-notice");
    notice.style.display = "flex";
    document.getElementById("schist-desktop-notice-ok").onclick = () => {
      try {
        localStorage.setItem(NOTICE_KEY, new Date().toISOString());
      } catch {}
      notice.remove();
    };
  }
} catch {}

window.__schistLoadingDone = () => {
  overlay.classList.add("done");
  // Gone entirely once the fade finishes, so it can't sit over the canvas.
  setTimeout(() => overlay.remove(), 400);
};

window.__schistLoadingFailed = (message) => {
  if (!overlay.isConnected) return;
  overlay.classList.remove("done");
  status.remove();
  document.querySelector("#schist-loading .bar")?.remove();
  let card = document.getElementById("schist-loading-error");
  if (!card) {
    card = document.createElement("div");
    card.className = "error";
    card.id = "schist-loading-error";
    overlay.appendChild(card);
  }
  card.textContent = message;
};

function setStatus(text) {
  status.textContent = text;
}

// One shared progress count across every parallel fetch.
let totalBytes = 0;
let gotBytes = 0;
function onBytes(n) {
  gotBytes += n;
  if (totalBytes > 0) {
    fill.style.width = `${Math.min(100, (100 * gotBytes) / totalBytes)}%`;
  }
}

// Fetch one file, streaming so the bar moves per chunk received rather
// than per file completed.
async function fetchBytes(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`HTTP ${response.status} for ${url}`);
  if (!response.body) {
    const buffer = new Uint8Array(await response.arrayBuffer());
    onBytes(buffer.length);
    return buffer;
  }
  const reader = response.body.getReader();
  const parts = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    length += value.length;
    onBytes(value.length);
  }
  const out = new Uint8Array(length);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

async function boot() {
  if (!navigator.gpu) {
    throw new Error(S.webgpu);
  }

  setStatus(S.loading);
  const manifest = await (await fetch("manifest.json")).json();
  // A font tagged with a language is for that language's readers only:
  // the Chinese face is 1.5 MB nobody else needs.
  const fontFiles = manifest.fonts.filter((f) => fontMatches(f, LANG));
  totalBytes =
    manifest.wasm.reduce((n, c) => n + c.bytes, 0) +
    fontFiles.reduce((n, f) => n + f.bytes, 0) +
    manifest.assets.reduce((n, a) => n + a.bytes, 0);

  // Everything in parallel: the wasm chunks dominate, and the browser
  // pools the connections.
  const wasmChunks = Promise.all(manifest.wasm.map((c) => fetchBytes(c.file)));
  const fonts = Promise.all(fontFiles.map((f) => fetchBytes(f.file)));
  const assets = Promise.all(
    manifest.assets.map(async (a) => [a.path, await fetchBytes(a.file)]),
  );

  const chunks = await wasmChunks;
  const wasm = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let at = 0;
  for (const chunk of chunks) {
    wasm.set(chunk, at);
    at += chunk.length;
  }

  window.__schist_boot = {
    fonts: await fonts,
    assets: Object.fromEntries(await assets),
  };

  setStatus(S.starting);
  const { default: init } = await import(`./${manifest.js}`);
  await init({ module_or_path: wasm });
  // From here the app owns the page; __schistLoadingDone fires once its
  // window is up and painting.
}

boot().catch((err) => {
  console.error(err);
  window.__schistLoadingFailed(String(err?.message ?? err));
});
