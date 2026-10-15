import { invoke, isTauri } from "@tauri-apps/api/core";
import "./style.css";
type Mode = "alpha" | "sidecar" | "entropy";
type Rect = { x: number; y: number; width: number; height: number };
type View = {
  name: string;
  width: number;
  height: number;
  bpp: number;
  mode: Mode | null;
  rgba: number[];
  preview_width: number;
  preview_height: number;
  suggested_blocks: Rect[];
};
const native = isTauri();
let doc: View | null = null,
  regions: Rect[] = [],
  busy = false,
  dirty = false,
  operation: "protect" | "restore" = "protect",
  mode: Mode = "sidecar",
  start: { x: number; y: number } | null = null,
  draft: Rect | null = null;
const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;
const icon = (name: string) =>
  ({ open: "↗", save: "↓", select: "⌖", undo: "↶", clear: "×" })[name] ?? name;
document.querySelector("#app")!.innerHTML = `
<header><a class="brand" href="#" aria-label="Lossless Mosaic"><span class="brand-mark">▦</span><span>LOSSLESS<span class="brand-sub">MOSAIC</span></span></a><span class="header-divider"></span><span class="workspace-label">可逆图像工作台</span><div class="header-right"><span class="local-dot"></span>本地处理 · 文件不上传<span class="version">v0.1</span></div></header>
<main><section class="workspace"><div class="toolbar"><button id="open" class="button">${icon("open")} 打开 BMP <kbd>⌘O</kbd></button><span class="tool-separator"></span><button id="undo" class="icon-button" title="撤销选区（⌘Z）" aria-label="撤销选区">${icon("undo")}</button><button id="clear" class="icon-button" title="清空选区" aria-label="清空选区">${icon("clear")}</button><div class="toolbar-right"><span id="zoom">适应画布</span><button id="save" class="button save-button">${icon("save")} 另存为</button></div></div>
<div class="document-bar"><span id="file-name">未打开文件</span><span id="file-meta">BMP · 24 / 32 bit</span></div>
<div class="viewport" id="viewport"><div class="empty" id="empty"><div class="empty-art"><span></span><span></span><span></span><span></span><i>⌖</i></div><p class="eyebrow">A LITTLE PRIVACY. FULLY REVERSIBLE.</p><h1>遮住此刻，保留原貌。</h1><p>打开一张 BMP，框选你想保护的区域。<br>用一个密码，将隐藏的细节重新找回。</p><button class="primary" id="open-empty">打开 BMP 图片 <span>↗</span></button><small>支持未压缩 BMP · 最大 64 MiB</small></div><div class="canvas-wrap" id="canvas-wrap" hidden><canvas id="image"></canvas><canvas id="overlay" tabindex="0" aria-label="图像选区：拖动框选，区块模式点击可切换网格"></canvas></div><div class="canvas-hint" id="canvas-hint" hidden>拖动框选区域 · 可添加多个选区</div></div>
<footer><span id="status-dot" class="status-dot"></span><span id="status" role="status" aria-live="polite">准备就绪</span><span class="footer-right" id="selection-count">0 个选区</span></footer></section>
<aside><div class="panel-scroll"><div class="panel-heading"><p class="eyebrow">YOUR IMAGE, YOUR KEY</p><h2>隐私，由你掌握。</h2><p>加密选区，或找回完整画面。</p></div><div class="segmented"><button id="tab-protect" class="active">加密打码</button><button id="tab-restore">密码还原</button></div>
<section class="settings-section"><div class="section-label"><span>01</span><h3>标记方式</h3></div><div class="modes" role="radiogroup" aria-label="标记方式">
<button class="mode-card selected" data-mode="sidecar" role="radio" aria-checked="true"><span class="mode-icon">▤</span><span><strong>附加数据 <em>推荐</em></strong><small>图片 + 独立还原文件，完整保留原始数据</small></span><i></i></button>
<button class="mode-card" data-mode="alpha" role="radio" aria-checked="false"><span class="mode-icon">◩</span><span><strong>α 通道</strong><small>还原信息随图携带，无需额外文件</small></span><i></i></button>
<button class="mode-card" data-mode="entropy" role="radio" aria-checked="false"><span class="mode-icon">▦</span><span><strong>整区块 <em class="experimental">实验性</em></strong><small>32 × 32 网格 · 熵建议 + 手动标记</small></span><i></i></button></div><p class="mode-note" id="mode-note"></p><label class="warning-check" id="alpha-warning" hidden><input type="checkbox" id="alpha-consent">我知道原有 α 数据会被覆盖，透明度无法还原。</label><button id="attach" class="text-button">导入 .lmm 还原文件 ↗</button></section>
<section class="settings-section"><div class="section-label"><span>02</span><h3>设置密码</h3></div><label class="field-label" for="password">密码</label><div class="password-field"><input id="password" type="password" autocomplete="new-password" maxlength="256" placeholder="输入用于加密与还原的密码"><button id="reveal" aria-label="显示密码" title="显示或隐藏密码">◉</button></div><div id="confirm-wrap"><label class="field-label" for="confirm">确认密码</label><input id="confirm" type="password" autocomplete="new-password" maxlength="256" placeholder="再次输入密码"></div><p class="password-note">请妥善保管密码。建议使用较长且独特的密码。</p></section>
<section class="settings-section selection-section"><div class="section-label"><span>03</span><h3 id="selection-title">选择保护区域</h3></div><div class="selection-info"><span id="selection-summary">在左侧画布拖动框选</span><button id="suggest" class="text-button" hidden>熵检测建议</button></div><div id="region-list" class="region-list"></div></section>
</div><div class="panel-bottom"><button id="apply" class="primary full" disabled>加密选区 <span>→</span></button><p id="action-note">加密后请另存为，源文件不会被覆盖。</p></div></aside></main>
<div id="toast" role="alert" hidden></div>`;
const canvas = $<HTMLCanvasElement>("image"),
  overlay = $<HTMLCanvasElement>("overlay"),
  ctx = overlay.getContext("2d")!;
function notice(text: string, error = false) {
  $("status").textContent = text;
  $("status-dot").classList.toggle("error", error);
  $("toast").textContent = text;
  $("toast").classList.toggle("error", error);
  $("toast").hidden = false;
  setTimeout(() => {
    $("toast").hidden = true;
  }, 6500);
}
function block(r: Rect): Rect {
  if (!doc) return r;
  const x = Math.floor(r.x / 32) * 32,
    y = Math.floor(r.y / 32) * 32;
  return {
    x,
    y,
    width: Math.min(doc.width, Math.ceil((r.x + r.width) / 32) * 32) - x,
    height: Math.min(doc.height, Math.ceil((r.y + r.height) / 32) * 32) - y,
  };
}
function cells(rs: Rect[]): Rect[] {
  if (!doc) return [];
  const out = new Map<string, Rect>();
  for (const r0 of rs) {
    const r = block(r0);
    for (let y = r.y; y < r.y + r.height; y += 32)
      for (let x = r.x; x < r.x + r.width; x += 32)
        out.set(`${x},${y}`, {
          x,
          y,
          width: Math.min(32, doc.width - x),
          height: Math.min(32, doc.height - y),
        });
  }
  return [...out.values()];
}
function overlaps(a: Rect, b: Rect) {
  return (
    a.x < b.x + b.width &&
    b.x < a.x + a.width &&
    a.y < b.y + b.height &&
    b.y < a.y + a.height
  );
}
function draw() {
  ctx.clearRect(0, 0, overlay.width, overlay.height);
  if (!doc) return;
  const sx = overlay.width / doc.width,
    sy = overlay.height / doc.height;
  ctx.save();
  ctx.scale(sx, sy);
  if (mode === "entropy") {
    ctx.strokeStyle = "rgba(255,255,255,.18)";
    ctx.lineWidth = 1 / sx;
    ctx.beginPath();
    for (let x = 32; x < doc.width; x += 32) {
      ctx.moveTo(x, 0);
      ctx.lineTo(x, doc.height);
    }
    for (let y = 32; y < doc.height; y += 32) {
      ctx.moveTo(0, y);
      ctx.lineTo(doc.width, y);
    }
    ctx.stroke();
  }
  for (const [i, r] of [
    ...regions,
    ...(draft ? [mode === "entropy" ? block(draft) : draft] : []),
  ].entries()) {
    ctx.fillStyle = "rgba(168,232,104,.18)";
    ctx.fillRect(r.x, r.y, r.width, r.height);
    ctx.strokeStyle = "#b0ef7e";
    ctx.lineWidth = 1.5 / sx;
    ctx.setLineDash([5 / sx, 4 / sx]);
    ctx.strokeRect(r.x, r.y, r.width, r.height);
    if (mode !== "entropy") {
      ctx.setLineDash([]);
      ctx.fillStyle = "#b0ef7e";
      ctx.fillRect(r.x, r.y, 22 / sx, 19 / sy);
      ctx.fillStyle = "#172215";
      ctx.font = `${11 / sx}px sans-serif`;
      ctx.fillText(String(i + 1), r.x + 7 / sx, r.y + 13 / sy);
    }
  }
  ctx.restore();
}
function update() {
  $("tab-protect").classList.toggle("active", operation === "protect");
  $("tab-restore").classList.toggle("active", operation === "restore");
  $("confirm-wrap").hidden = operation === "restore";
  $("selection-title").textContent =
    operation === "protect"
      ? "选择保护区域"
      : mode === "entropy"
        ? "确认需还原的区块"
        : "自动识别加密区域";
  $("selection-summary").textContent =
    mode === "entropy"
      ? "点击切换区块，或拖动批量选择"
      : operation === "restore"
        ? "还原文件中所有已加密区域"
        : "在左侧画布拖动框选";
  $("mode-note").textContent =
    mode === "sidecar"
      ? "请将 BMP 与同名 .lmm 文件一起保存、分享；RGB 与 α 均加密。"
      : mode === "alpha"
        ? "24 位输入可完整还原；32 位输入将丢失原有 α 数据。通道容量不足时请选择附加数据。"
        : "不增加任何数据，RGB 与 α 均参与 XOR。熵检测可能误判，请手动确认；密码错误无法检测，同密码同尺寸会复用密钥流。";
  $("alpha-warning").hidden = !(
    mode === "alpha" &&
    doc?.bpp === 32 &&
    operation === "protect"
  );
  $("suggest").hidden = mode !== "entropy" || !doc;
  $("attach").hidden = operation !== "restore" || mode !== "sidecar";
  $("apply").innerHTML = busy
    ? "处理中…"
    : `${operation === "protect" ? "加密选区" : "还原图像"} <span>→</span>`;
  const pw = $<HTMLInputElement>("password").value;
  const consent = $<HTMLInputElement>("alpha-consent").checked;
  $<HTMLButtonElement>("apply").disabled =
    busy ||
    !doc ||
    !pw ||
    (operation === "protect" && pw !== $<HTMLInputElement>("confirm").value) ||
    ((operation === "protect" || mode === "entropy") && !regions.length) ||
    (mode === "alpha" &&
      doc?.bpp === 32 &&
      operation === "protect" &&
      !consent) ||
    !native;
  for (const id of [
    "open",
    "open-empty",
    "save",
    "undo",
    "clear",
    "attach",
    "suggest",
    "tab-protect",
    "tab-restore",
  ])
    $<HTMLButtonElement>(id).disabled =
      busy || (["save", "attach"].includes(id) && !doc);
  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((b) => {
    b.disabled = busy;
    b.classList.toggle("selected", b.dataset.mode === mode);
    b.setAttribute("aria-checked", String(b.dataset.mode === mode));
  });
  $("selection-count").textContent =
    `${regions.length} 个${mode === "entropy" ? "区块" : "选区"}`;
  $("region-list").replaceChildren();
  regions.slice(0, 8).forEach((r, i) => {
    const el = document.createElement("button");
    el.className = "region-chip";
    el.textContent = `${i + 1} · ${r.width} × ${r.height}  ×`;
    el.title = `坐标 (${r.x}, ${r.y})，点击移除`;
    el.disabled = busy;
    el.onclick = () => {
      regions.splice(i, 1);
      update();
    };
    $("region-list").append(el);
  });
  if (regions.length > 8) {
    const el = document.createElement("span");
    el.textContent = `另有 ${regions.length - 8} 个`;
    el.className = "more-regions";
    $("region-list").append(el);
  }
  $("canvas-hint").textContent =
    mode === "entropy"
      ? "点击切换网格 · 拖动批量选择 · 还原前请确认选区"
      : "拖动框选区域 · 可添加多个选区";
  $("action-note").textContent = dirty
    ? "结果尚未保存，请点击「另存为」。"
    : "加密后请另存为，源文件不会被覆盖。";
  draw();
}
function fit() {
  if (!doc) return;
  const v = $("viewport"),
    w = Math.max(1, v.clientWidth - 64),
    h = Math.max(1, v.clientHeight - 82),
    ratio = doc.width / doc.height;
  const width = Math.min(w, h * ratio);
  $("canvas-wrap").style.width = `${width}px`;
  $("canvas-wrap").style.height = `${width / ratio}px`;
}
new ResizeObserver(fit).observe($("viewport"));
function show(v: View) {
  doc = v;
  regions = [];
  draft = null;
  $("empty").hidden = true;
  $("canvas-wrap").hidden = false;
  $("canvas-hint").hidden = false;
  $("file-name").textContent = v.name;
  $("file-meta").textContent = `${v.width} × ${v.height} px · ${v.bpp} bit`;
  canvas.width = overlay.width = v.preview_width;
  canvas.height = overlay.height = v.preview_height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(
        new Uint8ClampedArray(v.rgba),
        v.preview_width,
        v.preview_height,
      ),
      0,
      0,
    );
  $("canvas-wrap").style.aspectRatio = `${v.width}/${v.height}`;
  if (v.mode) {
    mode = v.mode;
    operation = "restore";
  }
  fit();
  update();
}
async function run<T>(action: () => Promise<T>): Promise<T | undefined> {
  if (busy) return;
  busy = true;
  update();
  try {
    return await action();
  } catch (e) {
    notice(String(e), true);
  } finally {
    busy = false;
    update();
  }
}
async function openFile() {
  if (!native) {
    notice("桌面文件操作请运行 npm run desktop。浏览器预览仅展示界面。");
    return;
  }
  if (
    dirty &&
    !window.confirm("当前结果尚未保存，打开其他图片将丢弃当前结果。是否继续？")
  )
    return;
  await run(async () => {
    const v = await invoke<View | null>("open_document");
    if (v) {
      dirty = false;
      operation = "protect";
      show(v);
      notice(
        v.mode
          ? "已识别还原信息，请输入密码"
          : "图片已打开，拖动框选需要保护的区域",
      );
    }
  });
}
$("open").onclick = $("open-empty").onclick = openFile;
$("save").onclick = () =>
  run(async () => {
    const path = await invoke<string | null>("save_document");
    if (path) {
      dirty = false;
      notice(`已保存：${path}`);
    }
  });
$("attach").onclick = () =>
  run(async () => {
    const v = await invoke<View | null>("attach_sidecar");
    if (v) {
      show(v);
      notice("还原文件已导入；还原时将验证密码及文件完整性");
    }
  });
$("undo").onclick = () => {
  regions.pop();
  update();
};
$("clear").onclick = () => {
  regions = [];
  update();
};
$("suggest").onclick = () => {
  if (doc) {
    regions = [...doc.suggested_blocks];
    notice(`熵检测建议 ${regions.length} 个区块，请手动检查并增删。`);
    update();
  }
};
$("tab-protect").onclick = () => {
  operation = "protect";
  update();
};
$("tab-restore").onclick = () => {
  operation = "restore";
  if (mode === "entropy" && doc && !regions.length)
    regions = [...doc.suggested_blocks];
  update();
};
document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach(
  (b) =>
    (b.onclick = () => {
      if (mode === b.dataset.mode) return;
      mode = b.dataset.mode as Mode;
      if (mode === "entropy") {
        regions = cells(regions);
        if (operation === "restore" && !regions.length && doc)
          regions = [...doc.suggested_blocks];
      } else regions = [];
      update();
    }),
);
for (const id of ["password", "confirm", "alpha-consent"]) {
  $(id).addEventListener("input", update);
  $(id).addEventListener("change", update);
}
$("reveal").onclick = () => {
  const p = $<HTMLInputElement>("password");
  p.type = p.type === "password" ? "text" : "password";
  $("reveal").setAttribute(
    "aria-label",
    p.type === "password" ? "显示密码" : "隐藏密码",
  );
};
$("apply").onclick = () =>
  run(async () => {
    const password = $<HTMLInputElement>("password").value;
    const selected = [...regions];
    const blockMode = mode === "entropy";
    try {
      const v = await invoke<View>("transform", {
        password,
        mode,
        regions,
        decrypt: operation === "restore",
      });
      const was = operation;
      dirty = true;
      show(v);
      if (was === "restore") operation = "protect";
      if (blockMode) {
        regions = selected;
        operation = was === "protect" ? "restore" : "protect";
      }
      notice(
        was === "protect"
          ? "选区已加密，请另存为。"
          : "所选数据已还原，请检查画面并另存为。",
      );
    } finally {
      $<HTMLInputElement>("password").value = "";
      $<HTMLInputElement>("confirm").value = "";
      $<HTMLInputElement>("alpha-consent").checked = false;
    }
  });
function point(e: PointerEvent) {
  const r = overlay.getBoundingClientRect();
  return {
    x: Math.max(
      0,
      Math.min(
        doc!.width - 1,
        Math.floor(((e.clientX - r.left) / r.width) * doc!.width),
      ),
    ),
    y: Math.max(
      0,
      Math.min(
        doc!.height - 1,
        Math.floor(((e.clientY - r.top) / r.height) * doc!.height),
      ),
    ),
  };
}
overlay.onpointerdown = (e) => {
  if (
    !doc ||
    busy ||
    (operation === "restore" && mode !== "entropy") ||
    e.button !== 0
  )
    return;
  draft = null;
  start = point(e);
  overlay.setPointerCapture(e.pointerId);
};
overlay.onpointermove = (e) => {
  if (!start || !doc) return;
  const p = point(e);
  draft = {
    x: Math.min(start.x, p.x),
    y: Math.min(start.y, p.y),
    width: Math.abs(p.x - start.x) + 1,
    height: Math.abs(p.y - start.y) + 1,
  };
  draw();
};
overlay.onpointerup = (e) => {
  if (!start || !doc) return;
  const p = point(e),
    click = Math.abs(p.x - start.x) < 3 && Math.abs(p.y - start.y) < 3;
  const r = draft ?? { ...start, width: 1, height: 1 };
  start = null;
  draft = null;
  if (mode === "entropy") {
    const add = cells([r]);
    if (click) {
      const tile = add[0];
      const i = regions.findIndex((a) => a.x === tile.x && a.y === tile.y);
      if (i < 0) regions.push(tile);
      else regions.splice(i, 1);
    } else regions = cells([...regions, ...add]);
  } else if (!click) {
    if (regions.some((a) => overlaps(a, r)))
      notice("选区不可重叠，请重新框选。", true);
    else if (regions.length >= 256) notice("最多支持 256 个选区", true);
    else regions.push(r);
  }
  update();
};
overlay.onpointercancel = () => {
  start = null;
  draft = null;
  draw();
};
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "o") {
    e.preventDefault();
    if (!busy) void openFile();
  }
  if ((e.metaKey || e.ctrlKey) && e.key === "s") {
    e.preventDefault();
    if (!busy && doc) $("save").click();
  }
  if (
    (e.metaKey || e.ctrlKey) &&
    e.key === "z" &&
    !(e.target instanceof HTMLInputElement)
  ) {
    e.preventDefault();
    if (!busy) {
      regions.pop();
      update();
    }
  }
  if (e.key === "Escape") {
    start = null;
    draft = null;
    draw();
  }
});
if (!native) {
  $("status").textContent =
    "浏览器界面预览 · 使用 npm run desktop 启动完整客户端";
}
update();
