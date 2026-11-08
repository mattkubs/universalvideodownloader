"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const el = {
  url: $("url"), fetch: $("btn-fetch"), engineNote: $("engine-note"),
  fmtGroup: $("fmt-group"), fmtList: $("fmt-list"),
  folder: $("folder"), browse: $("btn-browse"),
  auto: $("auto-switch"), cookies: $("cookies"), cookieFile: $("cookie-file"),
  dl: $("btn-dl"), dlPanel: $("dl-panel"), dlLabel: $("dl-label"), dlPct: $("dl-pct"),
  dlFill: $("dl-fill"), dlSpeed: $("dl-speed"), dlEta: $("dl-eta"), status: $("status"),
  convPath: $("conv-path"), choose: $("btn-choose"), conv: $("btn-conv"),
  convPanel: $("conv-panel"), convPct: $("conv-pct"), convFill: $("conv-fill"), convStatus: $("conv-status"),
  histGroup: $("hist-group"), histList: $("hist-list"), clear: $("btn-clear"),
  cancelBar: $("cancel-bar"), cancel: $("btn-cancel"),
  footLeft: $("foot-left"), footRight: $("foot-right"),
};

const state = {
  ready: false,
  busy: false,
  title: "",
  selected: null,
  cookieFile: "",
  cookieChoice: "none",
};

// ---------------------------------------------------------------- prefs
// per-machine conveniences only; the app works fine if storage is unavailable
const prefs = {
  get(k, d) { try { const v = localStorage.getItem("uvd." + k); return v === null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem("uvd." + k, JSON.stringify(v)); } catch { /* ignore */ } },
};

// ---------------------------------------------------------------- helpers
function setStatus(node, msg, kind = "ok") {
  node.textContent = msg || "";
  node.classList.toggle("error", kind === "error");
  node.classList.toggle("muted", kind === "muted");
}

function kindOf(msg) {
  if (msg.startsWith("✕")) return "muted";
  if (msg.startsWith("✓")) return "ok";
  return "error";
}

function cookiesArg() {
  const v = el.cookies.value;
  if (v === "none") return { mode: "none", value: "" };
  if (v === "file") return { mode: "file", value: state.cookieFile };
  return { mode: "browser", value: v };
}

function refreshButtons() {
  el.fetch.disabled = !state.ready;
  el.fetch.textContent = state.ready ? "Get formats" : "Preparing…";
  el.dl.disabled = !state.ready || state.busy || !state.selected;
  el.conv.disabled = state.busy || !el.convPath.value;
}

function setBusy(on) {
  state.busy = on;
  el.cancelBar.classList.toggle("hidden", !on);
  el.cancel.disabled = false;
  el.cancel.textContent = "CANCEL";
  if (!on) el.footLeft.textContent = "idle";
  refreshButtons();
}

const icon = (t) => {
  const s = document.createElement("span");
  s.textContent = t;
  return s;
};

// ---------------------------------------------------------------- engine
async function prepareEngine() {
  state.ready = false;
  refreshButtons();
  el.footLeft.textContent = "checking yt-dlp updates…";
  el.engineNote.className = "note";
  el.engineNote.textContent = "Checking for the latest yt-dlp…";
  try {
    const info = await invoke("prepare_engine");
    const label = {
      current: "up to date",
      updated: "updated just now",
      offline: "offline, using installed copy",
      "update-failed": "update failed, using installed copy",
    }[info.status] || info.status;
    el.footRight.textContent = `yt-dlp ${info.version} · ${label}`;
    el.footRight.title = info.note || "";
    el.engineNote.classList.add("hidden");
    if (!info.ffmpeg) {
      el.engineNote.className = "note";
      el.engineNote.textContent = "ffmpeg not found: merging and MP4 conversion may fail.";
    }
    state.ready = true;
    el.footLeft.textContent = "idle";
  } catch (e) {
    el.engineNote.className = "note error";
    el.engineNote.textContent = String(e);
    const retry = document.createElement("button");
    retry.className = "btn secondary";
    retry.textContent = "Retry";
    retry.onclick = prepareEngine;
    el.engineNote.appendChild(retry);
    el.footLeft.textContent = "engine unavailable";
  }
  refreshButtons();
}

listen("engine-progress", (e) => {
  el.engineNote.textContent = `Downloading the latest yt-dlp… ${Math.floor(e.payload)}%`;
  el.footLeft.textContent = `updating yt-dlp ${Math.floor(e.payload)}%`;
});

// ---------------------------------------------------------------- formats
function renderCards(cards) {
  el.fmtList.replaceChildren();
  for (const c of cards) {
    const card = document.createElement("div");
    card.className = "card";

    const name = document.createElement("span");
    name.className = "name";
    name.textContent = c.label;
    card.appendChild(name);

    if (c.best) {
      const b = document.createElement("span");
      b.className = "badge";
      b.textContent = "BEST";
      card.appendChild(b);
    }
    for (const chip of c.chips) {
      const ch = document.createElement("span");
      ch.className = "chip";
      ch.textContent = chip;
      card.appendChild(ch);
    }
    const sp = document.createElement("span");
    sp.className = "spacer";
    card.appendChild(sp);

    const size = document.createElement("span");
    size.className = "size";
    size.textContent = c.size;
    card.appendChild(size);

    const check = icon("✓");
    check.className = "check";
    card.appendChild(check);

    card.addEventListener("click", () => select(c, card));
    el.fmtList.appendChild(card);
  }
  el.fmtGroup.classList.toggle("hidden", cards.length === 0);
  const first = el.fmtList.firstElementChild;
  if (first) select(cards[0], first);
}

function select(card, node) {
  state.selected = card;
  for (const n of el.fmtList.children) n.classList.toggle("selected", n === node);
  refreshButtons();
}

async function fetchFormats() {
  const url = el.url.value.trim();
  if (!url || !state.ready) return;
  if (el.cookies.value === "file" && !state.cookieFile) return;
  el.fetch.disabled = true;
  el.fetch.textContent = "Fetching…";
  el.footLeft.textContent = "querying formats…";
  setStatus(el.status, "");
  try {
    const res = await invoke("fetch_formats", { url, cookies: cookiesArg() });
    state.title = res.title;
    renderCards(res.cards);
    setStatus(el.status, `Found formats for: ${res.title}`);
    el.footLeft.textContent = "ready";
  } catch (e) {
    let msg = String(e);
    if (/sign in|login|log in|cookies|private/i.test(msg)) {
      msg += "  Tip: pick a cookies source below and try again.";
    }
    setStatus(el.status, "Error: " + msg, "error");
    el.footLeft.textContent = "error";
    state.selected = null; // don't leave the previous link's formats downloadable
    renderCards([]);
  }
  refreshButtons();
}

// ---------------------------------------------------------------- download
async function startDownload() {
  if (!state.selected || state.busy) return;
  const req = {
    url: el.url.value.trim(),
    spec: state.selected.spec,
    folder: el.folder.value,
    cookies: cookiesArg(),
    autoConvert: el.auto.getAttribute("aria-checked") === "true",
  };
  setStatus(el.status, "");
  setStatus(el.convStatus, "");
  // Go busy *before* invoking: a fast failure's events can arrive before invoke resolves.
  state.lastLabel = state.selected.label;
  setBusy(true);
  el.dl.classList.add("hidden");
  el.dlPanel.classList.remove("hidden");
  el.dlFill.style.width = "0%";
  el.dlPct.textContent = "0%";
  el.dlLabel.textContent = "Starting…";
  el.dlSpeed.textContent = "";
  el.dlEta.textContent = "";
  try {
    await invoke("start_download", { req });
  } catch (e) {
    el.dlPanel.classList.add("hidden");
    el.dl.classList.remove("hidden");
    setBusy(false);
    setStatus(el.status, "Error: " + e, "error");
  }
}

listen("dl-progress", ({ payload: p }) => {
  const pct = Math.max(0, Math.min(100, p.pct));
  el.dlFill.style.width = pct + "%";
  el.dlPct.textContent = Math.floor(pct) + "%";
  el.dlSpeed.textContent = p.speed;
  el.dlEta.textContent = p.eta;
  el.dlLabel.textContent =
    pct >= 100 ? "Processing…" :
    p.stream === "audio" ? "Downloading audio…" :
    p.stream === "video" ? "Downloading video…" : "Downloading…";
  el.footLeft.textContent = `downloading ${Math.floor(pct)}%`;
});

listen("dl-note", ({ payload }) => setStatus(el.status, payload, "muted"));

listen("dl-done", ({ payload: d }) => {
  el.dlPanel.classList.add("hidden");
  el.dl.classList.remove("hidden");
  setStatus(el.status, d.msg, kindOf(d.msg));
  if (d.ok) addHistory(state.title || "video", state.lastLabel || "");
});

// ---------------------------------------------------------------- convert
async function chooseConvertFile() {
  const p = await invoke("pick_file", { kind: "video", current: el.folder.value });
  if (p) {
    el.convPath.value = p;
    el.convPath.title = p;
    setStatus(el.convStatus, "");
  }
  refreshButtons();
}

async function startConvert() {
  if (state.busy || !el.convPath.value) return;
  setBusy(true);
  try {
    await invoke("start_convert", { path: el.convPath.value });
  } catch (e) {
    setBusy(false);
    setStatus(el.convStatus, "Error: " + e, "error");
  }
}

listen("conv-start", () => {
  el.convPanel.classList.remove("hidden");
  el.convFill.style.width = "0%";
  el.convPct.textContent = "0%";
  setStatus(el.convStatus, "");
  el.footLeft.textContent = "converting…";
});

listen("conv-progress", ({ payload: pct }) => {
  el.convFill.style.width = pct + "%";
  el.convPct.textContent = Math.floor(pct) + "%";
  el.footLeft.textContent = `converting ${Math.floor(pct)}%`;
});

listen("conv-done", ({ payload: d }) => {
  el.convPanel.classList.add("hidden");
  setStatus(el.convStatus, d.msg, kindOf(d.msg));
});

listen("op-end", () => setBusy(false));

// ---------------------------------------------------------------- cancel
el.cancel.addEventListener("click", () => {
  el.cancel.disabled = true;
  el.cancel.textContent = "CANCELLING…";
  invoke("cancel_op");
});

// ---------------------------------------------------------------- history
const FILM_SVG =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<rect x="2" y="3" width="20" height="18" rx="2"/><path d="M7 3v18M17 3v18M2 8h5M2 16h5M17 8h5M17 16h5"/></svg>';

function addHistory(title, fmt) {
  el.histGroup.classList.remove("hidden");
  const row = document.createElement("div");
  row.className = "hist";
  row.innerHTML = FILM_SVG;
  const t = document.createElement("span");
  t.className = "t";
  t.textContent = title;
  t.title = title;
  const now = new Date();
  const meta = document.createElement("span");
  meta.className = "meta";
  meta.textContent = `${fmt} · ${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
  const done = document.createElement("span");
  done.className = "done";
  done.textContent = "Done";
  row.append(t, meta, done);
  el.histList.prepend(row);
}

el.clear.addEventListener("click", () => {
  el.histList.replaceChildren();
  el.histGroup.classList.add("hidden");
});

// ---------------------------------------------------------------- settings
el.auto.addEventListener("click", () => {
  const on = el.auto.getAttribute("aria-checked") !== "true";
  el.auto.setAttribute("aria-checked", String(on));
  prefs.set("auto", on);
});

el.browse.addEventListener("click", async () => {
  const d = await invoke("pick_folder", { current: el.folder.value });
  if (d) {
    el.folder.value = d;
    el.folder.title = d;
    prefs.set("folder", d);
  }
});

function showCookieFile() {
  const isFile = el.cookies.value === "file" && state.cookieFile;
  el.cookieFile.classList.toggle("hidden", !isFile);
  if (isFile) {
    el.cookieFile.textContent = state.cookieFile.split(/[\\/]/).pop();
    el.cookieFile.title = state.cookieFile;
  }
}

el.cookies.addEventListener("change", async () => {
  if (el.cookies.value === "file") {
    const f = await invoke("pick_file", { kind: "cookies", current: "" });
    if (!f) {
      el.cookies.value = state.cookieChoice; // cancelled: keep previous choice
      showCookieFile();
      return;
    }
    state.cookieFile = f;
    prefs.set("cookieFile", f);
  }
  state.cookieChoice = el.cookies.value;
  prefs.set("cookies", el.cookies.value);
  showCookieFile();
});

el.cookieFile.addEventListener("click", () => {
  el.cookies.value = "file";
  el.cookies.dispatchEvent(new Event("change"));
});

el.fetch.addEventListener("click", fetchFormats);
el.url.addEventListener("keydown", (e) => { if (e.key === "Enter") fetchFormats(); });
el.dl.addEventListener("click", startDownload);
el.choose.addEventListener("click", chooseConvertFile);
el.conv.addEventListener("click", startConvert);

// ---------------------------------------------------------------- init
(async function init() {
  // Safari cookies only exist on macOS
  if (!/Mac/i.test(navigator.userAgent)) {
    el.cookies.querySelector('option[value="safari"]').remove();
  }
  el.auto.setAttribute("aria-checked", String(prefs.get("auto", true)));
  state.cookieFile = prefs.get("cookieFile", "");
  const savedCookies = prefs.get("cookies", "none");
  if ([...el.cookies.options].some((o) => o.value === savedCookies) &&
      (savedCookies !== "file" || state.cookieFile)) {
    el.cookies.value = savedCookies;
  }
  state.cookieChoice = el.cookies.value;
  showCookieFile();

  el.folder.value = prefs.get("folder", "") || (await invoke("default_folder"));
  el.folder.title = el.folder.value;
  refreshButtons();
  prepareEngine();
})();
