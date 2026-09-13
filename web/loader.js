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

// The page's own few strings, in the languages the app's chrome has
// (crates/i18n). Chosen the way the app chooses: the first of the
// browser's preferred languages that is here, by language alone, else
// English. The app reads navigator.languages itself once it is up.
const STRINGS = {
  en: {
    loading: "Loading…",
    starting: "Starting…",
    webgpu:
      "Schist needs WebGPU, which this browser doesn't offer. " +
      "Chrome/Edge 113+, Firefox 141+ and Safari 26+ do.",
    noticeTitle: "Schist runs best as a desktop app",
    noticeBody:
      "You're using the web version, which is a lighter build: it " +
      "composites on the CPU and leaves out some features — Photoshop " +
      "and third-party plug-ins, image gallery, the AI panel, HEIC " +
      "import and font downloads — and files are saved as downloads. " +
      "The free desktop app for macOS, Windows and Linux has all of it.",
    noticeLink: "Get the desktop app.",
    ok: "OK",
  },
  sv: {
    loading: "Läser in…",
    starting: "Startar…",
    webgpu:
      "Schist behöver WebGPU, som den här webbläsaren inte har. " +
      "Chrome/Edge 113+, Firefox 141+ och Safari 26+ har det.",
    noticeTitle: "Schist fungerar bäst som skrivbordsprogram",
    noticeBody:
      "Du använder webbversionen, som är en lättare variant: den " +
      "komponerar på processorn och saknar vissa funktioner – " +
      "Photoshop- och tredjepartsplugin, bildgalleriet, AI-panelen, " +
      "HEIC-import och teckensnittshämtning – och filer sparas som " +
      "hämtningar. Det kostnadsfria skrivbordsprogrammet för macOS, " +
      "Windows och Linux har allt.",
    noticeLink: "Hämta skrivbordsprogrammet.",
    ok: "OK",
  },
  de: {
    loading: "Wird geladen…",
    starting: "Wird gestartet…",
    webgpu:
      "Schist braucht WebGPU, das dieser Browser nicht bietet. " +
      "Chrome/Edge 113+, Firefox 141+ und Safari 26+ haben es.",
    noticeTitle: "Schist läuft am besten als Desktop-App",
    noticeBody:
      "Du verwendest die Web-Version, eine leichtere Ausgabe: Sie " +
      "komponiert auf der CPU und lässt einige Funktionen weg – " +
      "Photoshop- und Drittanbieter-Plug-ins, die Bildgalerie, das " +
      "KI-Bedienfeld, HEIC-Import und Schrift-Downloads – und Dateien " +
      "werden als Downloads gesichert. Die kostenlose Desktop-App für " +
      "macOS, Windows und Linux hat alles davon.",
    noticeLink: "Desktop-App laden.",
    ok: "OK",
  },
  ja: {
    loading: "読み込み中…",
    starting: "起動中…",
    webgpu:
      "Schist には WebGPU が必要ですが、このブラウザーは対応していません。" +
      "Chrome/Edge 113 以降、Firefox 141 以降、Safari 26 以降が対応しています。",
    noticeTitle: "Schist はデスクトップ版が最適です",
    noticeBody:
      "ご覧になっているのはウェブ版で、軽量な構成です。合成を CPU で行い、" +
      "一部の機能——Photoshop 用およびサードパーティのプラグイン、画像ギャラリー、" +
      "AI パネル、HEIC の読み込み、フォントのダウンロード——を省いており、" +
      "ファイルはダウンロードとして保存されます。macOS、Windows、Linux 向けの" +
      "無料のデスクトップ版にはそのすべてがあります。",
    noticeLink: "デスクトップ版を入手",
    ok: "OK",
  },
  zh: {
    loading: "正在载入…",
    starting: "正在启动…",
    webgpu:
      "Schist 需要 WebGPU，而此浏览器不支持。" +
      "Chrome/Edge 113+、Firefox 141+ 和 Safari 26+ 支持。",
    noticeTitle: "Schist 在桌面应用中体验最佳",
    noticeBody:
      "你正在使用网页版，这是一个精简版本：它在 CPU 上合成，并且缺少部分" +
      "功能——Photoshop 和第三方增效工具、图库、AI 面板、HEIC 导入和字体" +
      "下载——文件会以下载方式存储。适用于 macOS、Windows 和 Linux 的免费" +
      "桌面应用具备全部功能。",
    noticeLink: "获取桌面应用。",
    ok: "好",
  },
};

function pickLanguage() {
  const preferred = navigator.languages?.length
    ? navigator.languages
    : [navigator.language ?? "en"];
  for (const tag of preferred) {
    const language = String(tag).toLowerCase().split(/[-_]/)[0];
    if (STRINGS[language]) return language;
  }
  return "en";
}

const LANG = pickLanguage();
const S = STRINGS[LANG];
document.documentElement.lang = LANG === "zh" ? "zh-Hans" : LANG;
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
  const fontFiles = manifest.fonts.filter((f) => !f.lang || f.lang === LANG);
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
