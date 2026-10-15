import { invoke, isTauri } from "@tauri-apps/api/core";
import { GridMask, type Selection, type Point } from "./grid";
import { ImageViewport } from "./viewport";
import "./style.css";
type Mode = "alpha" | "entropy" | "sidecar";
type Tool = "paint" | "erase" | "hand";
type View = {
  name: string;
  width: number;
  height: number;
  bpp: number;
  source_bpp: number;
  mode: Mode | null;
  preview_url: string;
  preview_width: number;
  preview_height: number;
  selection: Selection | null;
};
const native = isTauri();
let doc: View | null = null,
  mask: GridMask | null = null,
  applied: GridMask | null = null,
  camera: ImageViewport | null = null;
let busy = false,
  dirty = false,
  operation: "protect" | "restore" = "protect",
  mode: Mode = "alpha",
  tool: Tool = "paint",
  space = false,
  brushCells = 1;
let pixelBrush = 32;
let assumedBlockMask: GridMask | null = null;
let appliedMode: Mode = "alpha";
let undo: GridMask[] = [],
  redo: GridMask[] = [];
let gesture: {
  id: number;
  kind: "paint" | "pan";
  last: Point;
  before: GridMask | null;
  erase: boolean;
} | null = null;
const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;
const icon = (name: string) =>
  ({ open: "↗", save: "↓", undo: "↶", clear: "×" })[name] ?? name;
document.querySelector("#app")!.innerHTML = `
<header><a class="brand" href="#" aria-label="Lossless Mosaic"><span class="brand-mark">▦</span><span>LOSSLESS<span class="brand-sub">MOSAIC</span></span></a><span class="header-divider"></span><span class="workspace-label">可逆图像工作台</span><div class="header-right"><span class="local-dot"></span>本地处理 · 文件不上传<span class="version">v0.1</span></div></header>
<main><section class="workspace"><div class="toolbar"><button id="open" class="button">${icon("open")} 打开 BMP <kbd>⌘O</kbd></button><span class="tool-separator"></span><button id="undo" class="icon-button" title="撤销笔触（⌘Z）" aria-label="撤销笔触">${icon("undo")}</button><button id="clear" class="icon-button" title="清空蒙版" aria-label="清空蒙版">${icon("clear")}</button><div class="toolbar-right"><div class="zoom-controls"><button id="zoom-out" title="缩小" aria-label="缩小">−</button><span id="zoom">100%</span><button id="zoom-in" title="放大" aria-label="放大">+</button><button id="fit" title="适应画布">适应</button><button id="actual" title="原始尺寸 100%">1:1</button></div><button id="save" class="button save-button">${icon("save")} 另存为</button></div></div>
<div class="document-bar"><span id="file-name">未打开文件</span><span id="file-meta">BMP · 24 / 32 bit</span></div>
<div class="viewport" id="viewport"><div class="empty" id="empty"><div class="empty-art"><span></span><span></span><span></span><span></span><i>⌖</i></div><p class="eyebrow">A LITTLE PRIVACY. FULLY REVERSIBLE.</p><h1>遮住此刻，保留原貌。</h1><p>打开一张 BMP，涂抹你想保护的区域。<br>用一个密码，将隐藏的细节重新找回。</p><button class="primary" id="open-empty">打开 BMP 图片 <span>↗</span></button><small>支持未压缩 BMP · 最大 64 MiB</small></div><div class="canvas-wrap" id="canvas-wrap" hidden><canvas id="image"></canvas><canvas id="overlay" tabindex="0" aria-label="图像网格笔刷：涂抹选择，反打码擦除，空格拖动平移"></canvas></div><div id="brush-cursor" hidden></div><div class="canvas-hint" id="canvas-hint" hidden>打码 / 反打码笔刷 · 空格拖动平移 · Ctrl/⌘ + 滚轮缩放</div></div>
<footer><span id="status-dot" class="status-dot"></span><span id="status" role="status" aria-live="polite">准备就绪</span><span class="footer-right" id="selection-count">0 个选区</span></footer></section>
<aside><div class="panel-scroll"><div class="panel-heading"><p class="eyebrow">YOUR IMAGE, YOUR KEY</p><h2>隐私，由你掌握。</h2><p>加密选区，或找回完整画面。</p></div><div class="segmented"><button id="tab-protect" class="active">编辑打码</button><button id="tab-restore">密码还原</button></div>
<section class="settings-section"><div class="section-label"><span>01</span><h3>标记方式</h3></div><div class="modes" role="radiogroup" aria-label="标记方式">
<button class="mode-card selected" data-mode="alpha" role="radio" aria-checked="true"><span class="mode-icon">◩</span><span><strong>α 通道 <em>推荐</em></strong><small>还原信息随图携带，单文件分发</small></span><i></i></button>
<button class="mode-card" data-mode="entropy" role="radio" aria-checked="false"><span class="mode-icon">▦</span><span><strong>整区块</strong><small>零附加数据 · 熵建议 + 手动涂抹</small></span><i></i></button>
<button class="mode-card" data-mode="sidecar" role="radio" aria-checked="false"><span class="mode-icon">▤</span><span><strong>附加文件</strong><small>完整保留原始数据，需同时分发两个文件</small></span><i></i></button></div><p class="mode-note" id="mode-note"></p><label class="warning-check" id="alpha-warning" hidden><input type="checkbox" id="alpha-consent">我知道原有 α 数据会被覆盖，透明度无法还原。</label><button id="attach" class="text-button">导入 .lmm 还原文件 ↗</button></section>
<section class="settings-section"><div class="section-label"><span>02</span><h3>设置密码</h3></div><label class="field-label" for="password">密码</label><div class="password-field"><input id="password" type="password" autocomplete="new-password" maxlength="256" placeholder="输入用于加密与还原的密码"><button id="reveal" aria-label="显示密码" title="显示或隐藏密码">◉</button></div><div id="confirm-wrap"><label class="field-label" for="confirm">确认密码</label><input id="confirm" type="password" autocomplete="new-password" maxlength="256" placeholder="再次输入密码"></div><p class="password-note">请妥善保管密码。建议使用较长且独特的密码。</p></section>
<section class="settings-section selection-section"><div class="section-label"><span>03</span><h3>笔刷设置</h3></div>
<div class="brush-tools" role="group" aria-label="笔刷工具"><button id="paint" class="active" title="涂抹经过的网格">▧ 打码</button><button id="erase" title="移除经过的网格">▱ 反打码</button><button id="hand" title="拖动平移画布">✥ 平移</button></div>
<div class="setting-row" id="block-settings"><label for="block-size">区块尺寸</label><select id="block-size"><option value="8">8 × 8 · 密</option><option value="16">16 × 16</option><option value="32" selected>32 × 32</option><option value="64">64 × 64 · 疏</option></select></div>
<div class="setting-row"><label for="brush-size">笔刷宽度</label><output id="brush-size-label">1 格</output></div><input id="brush-size" aria-label="笔刷宽度" type="range" min="1" max="8" step="1" value="1">
<p class="mode-note" id="selection-summary">经过即选中，重复涂抹不会反转。</p><div class="selection-info"><span id="selection-title">0 个网格</span><button id="suggest" class="text-button" hidden>熵检测建议</button></div><button id="adopt" class="text-button" hidden>将当前网格记作已有打码</button></section>
</div><div class="panel-bottom"><button id="apply" class="primary full" disabled>应用打码 <span>→</span></button><p id="action-note">加密后请另存为，源文件不会被覆盖。</p></div></aside></main>
<div id="toast" role="alert" hidden></div>`;
const canvas = $<HTMLCanvasElement>("image"),
  overlay = $<HTMLCanvasElement>("overlay"),
  ctx = overlay.getContext("2d")!,
  viewport = $("viewport");
let toastTimer: ReturnType<typeof setTimeout>;
function notice(text: string, error = false) {
  $("status").textContent = text;
  $("status-dot").classList.toggle("error", error);
  $("toast").textContent = text;
  $("toast").classList.toggle("error", error);
  $("toast").hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    $("toast").hidden = true;
  }, 6500);
}
function pending() {
  return !!mask && !!applied && (!mask.equals(applied) || mode !== appliedMode);
}
function remember(before: GridMask) {
  if (mask && !mask.equals(before)) {
    undo.push(before);
    if (undo.length > 50) undo.shift();
    redo = [];
  }
}
function undoStroke() {
  if (!mask || !undo.length || busy) return;
  redo.push(mask);
  mask = undo.pop()!;
  update();
}
function redoStroke() {
  if (!mask || !redo.length || busy) return;
  undo.push(mask);
  mask = redo.pop()!;
  update();
}
function draw(clip?: {
  left: number;
  top: number;
  right: number;
  bottom: number;
}) {
  if (!doc || !mask || !camera) return;
  if (clip)
    clip = {
      left: Math.floor(clip.left / mask.size) * mask.size,
      top: Math.floor(clip.top / mask.size) * mask.size,
      right: Math.ceil(clip.right / mask.size) * mask.size,
      bottom: Math.ceil(clip.bottom / mask.size) * mask.size,
    };
  if (clip)
    ctx.clearRect(
      (clip.left * overlay.width) / doc.width,
      (clip.top * overlay.height) / doc.height,
      ((clip.right - clip.left) * overlay.width) / doc.width,
      ((clip.bottom - clip.top) * overlay.height) / doc.height,
    );
  else ctx.clearRect(0, 0, overlay.width, overlay.height);
  ctx.save();
  ctx.scale(overlay.width / doc.width, overlay.height / doc.height);
  let left = Math.max(0, Math.floor(-camera.x / camera.scale / mask.size)),
    right = Math.min(
      mask.columns,
      Math.ceil((viewport.clientWidth - camera.x) / camera.scale / mask.size),
    );
  let top = Math.max(0, Math.floor(-camera.y / camera.scale / mask.size)),
    bottom = Math.min(
      mask.rows,
      Math.ceil((viewport.clientHeight - camera.y) / camera.scale / mask.size),
    );
  if (clip) {
    left = Math.max(left, Math.floor(clip.left / mask.size));
    right = Math.min(right, Math.ceil(clip.right / mask.size));
    top = Math.max(top, Math.floor(clip.top / mask.size));
    bottom = Math.min(bottom, Math.ceil(clip.bottom / mask.size));
  }
  ctx.fillStyle = "rgba(175,235,125,.27)";
  for (let y = top; y < bottom; y++) {
    let run = -1;
    for (let x = left; x <= right; x++) {
      const selected = x < right && mask.has(y * mask.columns + x);
      if (selected && run < 0) run = x;
      if (!selected && run >= 0) {
        ctx.fillRect(
          run * mask.size,
          y * mask.size,
          Math.min(doc.width, x * mask.size) - run * mask.size,
          Math.min(mask.size, doc.height - y * mask.size),
        );
        run = -1;
      }
    }
  }
  if (mode === "entropy" && mask.size * camera.scale >= 6) {
    ctx.beginPath();
    ctx.strokeStyle = "rgba(222,239,209,.28)";
    ctx.lineWidth = 1 / camera.scale;
    for (let x = left; x <= right; x++) {
      ctx.moveTo(x * mask.size, top * mask.size);
      ctx.lineTo(x * mask.size, Math.min(doc.height, bottom * mask.size));
    }
    for (let y = top; y <= bottom; y++) {
      ctx.moveTo(left * mask.size, y * mask.size);
      ctx.lineTo(Math.min(doc.width, right * mask.size), y * mask.size);
    }
    ctx.stroke();
  }
  ctx.restore();
}
function layout() {
  if (!camera || !doc) return;
  $("brush-cursor").hidden = true;
  const w = $("canvas-wrap");
  w.style.width = `${doc.width * camera.scale}px`;
  w.style.height = `${doc.height * camera.scale}px`;
  w.style.transform = `translate(${camera.x}px,${camera.y}px)`;
  $("zoom").textContent = `${Math.round(camera.scale * 100)}%`;
  draw();
}
function fit() {
  if (camera) {
    camera.fit(viewport.clientWidth, viewport.clientHeight);
    layout();
  }
}
function zoom(
  factor: number,
  at: Point = { x: viewport.clientWidth / 2, y: viewport.clientHeight / 2 },
) {
  if (camera) {
    camera.zoom(factor, at);
    layout();
  }
}
new ResizeObserver(() => {
  if (camera) {
    const center = camera.image({
      x: viewport.clientWidth / 2,
      y: viewport.clientHeight / 2,
    });
    camera.x = viewport.clientWidth / 2 - center.x * camera.scale;
    camera.y = viewport.clientHeight / 2 - center.y * camera.scale;
    layout();
  }
}).observe(viewport);
function update() {
  $("paint").title =
    mode === "entropy" ? "涂抹选中经过的网格" : "按像素精度涂抹";
  $("erase").title = mode === "entropy" ? "移除经过的网格" : "按像素精度擦除";
  overlay.setAttribute(
    "aria-label",
    mode === "entropy"
      ? "区块笔刷：经过即选择整格"
      : "像素笔刷：支持单像素涂抹与擦除",
  );
  const count = mask?.count ?? 0,
    changed = pending(),
    restoreAll = operation === "restore" && mode !== "entropy";
  $("tab-protect").classList.toggle("active", operation === "protect");
  $("tab-restore").classList.toggle("active", operation === "restore");
  $("confirm-wrap").hidden = operation === "restore";
  for (const t of ["paint", "erase", "hand"] as const) {
    $(t).classList.toggle("active", t === tool);
    $<HTMLButtonElement>(t).disabled =
      busy || !doc || (restoreAll && t !== "hand");
  }
  viewport.classList.toggle(
    "panning",
    space || tool === "hand" || gesture?.kind === "pan",
  );
  $("mode-note").textContent =
    mode === "alpha"
      ? "单文件分发。24 位图片完整还原；32 位图片会覆盖原 α。像素精度涂抹；容量不足时可改用附加文件。"
      : mode === "entropy"
        ? "零附加数据，含 α 的全部通道参与 XOR。还原需选相同区块尺寸；密码错误无法检测，同密码同尺寸会复用密钥流。"
        : "BMP 与同名 .lmm 文件须一起分发。包括原 α 在内的数据可完整还原。";
  $("alpha-warning").hidden = !(
    mode === "alpha" &&
    doc?.source_bpp === 32 &&
    operation === "protect"
  );
  $("attach").hidden = operation !== "restore" || mode !== "sidecar";
  $("suggest").hidden = mode !== "entropy" || !doc;
  $("adopt").hidden =
    mode !== "entropy" || !doc || !!doc.mode || !!doc.selection;
  $<HTMLButtonElement>("adopt").disabled =
    busy || count === 0 || !!assumedBlockMask;
  $("selection-summary").textContent = restoreAll
    ? "完整还原所有加密像素。局部反打码请切换「编辑打码」，用反打码笔刷擦除。"
    : operation === "restore"
      ? "涂抹需要还原的网格；选择加密时相同的区块尺寸，再应用还原。"
      : mode === "entropy"
        ? "经过即选中整格，反打码移除整格；重叠涂抹覆盖旧状态。"
        : "像素精度圆形笔刷；打码涂抹，反打码擦除，支持重叠覆盖。";
  $("selection-count").textContent =
    `${count} ${mode === "entropy" ? "个网格" : "像素"}${changed ? " · 未应用" : ""}`;
  $("selection-title").textContent =
    mode === "entropy"
      ? `${count} 个网格 · ${mask?.size ?? 32} px`
      : `${count} 个选中像素`;
  $("block-settings").hidden = mode !== "entropy";
  $("brush-size-label").textContent =
    mode === "entropy" ? `${brushCells} 格` : `${pixelBrush} px`;
  $<HTMLInputElement>("brush-size").max = mode === "entropy" ? "8" : "256";
  $<HTMLInputElement>("brush-size").value = String(
    mode === "entropy" ? brushCells : pixelBrush,
  );
  if (mask && mask.size !== 1)
    $<HTMLSelectElement>("block-size").value = String(mask.size);
  const pw = $<HTMLInputElement>("password").value,
    confirmed = pw === $<HTMLInputElement>("confirm").value;
  const alphaConsent =
    mode !== "alpha" ||
    doc?.source_bpp !== 32 ||
    operation === "restore" ||
    $<HTMLInputElement>("alpha-consent").checked;
  $<HTMLButtonElement>("apply").disabled =
    busy ||
    !native ||
    !doc ||
    !pw ||
    !alphaConsent ||
    (operation === "protect" && !confirmed) ||
    (operation === "protect" && !changed && count === 0) ||
    (operation === "restore" && mode === "entropy" && count === 0);
  $("apply").innerHTML = busy
    ? "处理中…"
    : `${operation === "restore" ? "应用还原" : "应用打码 / 反打码"} <span>→</span>`;
  for (const id of [
    "open",
    "open-empty",
    "save",
    "attach",
    "suggest",
    "tab-protect",
    "tab-restore",
    "fit",
    "actual",
    "zoom-in",
    "zoom-out",
  ])
    $<HTMLButtonElement>(id).disabled =
      busy ||
      (!doc &&
        !["open", "open-empty", "tab-protect", "tab-restore"].includes(id));
  $<HTMLButtonElement>("undo").disabled = busy || !undo.length;
  $<HTMLButtonElement>("clear").disabled = busy || !count;
  $<HTMLSelectElement>("block-size").disabled = busy || !doc || restoreAll;
  $<HTMLInputElement>("brush-size").disabled = busy || !doc;
  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((b) => {
    b.disabled = busy;
    b.classList.toggle("selected", b.dataset.mode === mode);
    b.setAttribute("aria-checked", String(b.dataset.mode === mode));
  });
  $("action-note").textContent = changed
    ? "蒙版已修改：应用后才改变图片。"
    : dirty
      ? "结果尚未保存，请点击「另存为」。"
      : "涂抹 → 应用 → 另存为；源文件不会覆盖。";
  $("canvas-hint").textContent =
    "滚轮平移 · Ctrl/⌘ + 滚轮缩放 · 空格 / 中键拖动 · [ ] 调笔刷";
  draw();
}
function show(v: View, reset = true) {
  const previous = camera;
  doc = v;
  assumedBlockMask = null;
  if (v.mode) mode = v.mode;
  else if (v.selection) mode = "entropy";
  else if (reset) mode = "alpha";
  const size =
    v.selection?.block_size ??
    (mode === "entropy" ? Number($<HTMLSelectElement>("block-size").value) : 1);
  mask = new GridMask(v.width, v.height, size, v.selection?.bits);
  if (mode !== "entropy" && mask.size !== 1) mask = mask.resized(1);
  applied = mask.clone();
  appliedMode = mode;
  undo = [];
  redo = [];
  camera =
    previous &&
    !reset &&
    previous.width === v.width &&
    previous.height === v.height
      ? previous
      : new ImageViewport(v.width, v.height);
  $("empty").hidden = true;
  $("canvas-wrap").hidden = false;
  $("canvas-hint").hidden = false;
  $("file-name").textContent = v.name;
  $("file-meta").textContent = `${v.width} × ${v.height} px · ${v.bpp} bit`;
  canvas.width = overlay.width = v.preview_width;
  canvas.height = overlay.height = v.preview_height;
  const preview = new Image();
  preview.onload = () => {
    if (doc === v) canvas.getContext("2d")!.drawImage(preview, 0, 0);
  };
  preview.src = v.preview_url;
  if (reset) fit();
  else layout();
  update();
}
async function run<T>(fn: () => Promise<T>): Promise<T | undefined> {
  if (busy) return;
  busy = true;
  update();
  try {
    return await fn();
  } catch (e) {
    notice(String(e), true);
  } finally {
    busy = false;
    update();
  }
}
async function openFile() {
  if (!native) {
    notice("完整图片操作请运行 npm run desktop。");
    return;
  }
  if (
    (dirty || pending()) &&
    !window.confirm("当前有未保存或未应用的修改，是否丢弃并打开其他图片？")
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
          ? "已载入已有网格，可继续打码或反打码；应用时输入原密码。"
          : "图片已打开，可按像素精度涂抹。",
      );
    }
  });
}
$("open").onclick = $("open-empty").onclick = openFile;
$("save").onclick = () => {
  if (pending()) {
    notice("蒙版修改尚未应用，请先点击「应用打码 / 反打码」。", true);
    return;
  }
  void run(async () => {
    const path = await invoke<string | null>("save_document");
    if (path) {
      dirty = false;
      notice(`已保存：${path}`);
    }
  });
};
$("attach").onclick = () =>
  run(async () => {
    const v = await invoke<View | null>("attach_sidecar");
    if (v) {
      show(v, false);
      notice("还原文件已导入。");
    }
  });
$("adopt").onclick = () => {
  if (mask) {
    assumedBlockMask = mask.clone();
    applied = mask.clone();
    appliedMode = mode;
    undo = [];
    redo = [];
    notice(
      "已记作已有打码。可用反打码移除、打码添加，再用原密码应用。区块模式不能验证密码或识别结果。",
    );
    update();
  }
};
$("undo").onclick = undoStroke;
$("clear").onclick = () => {
  if (mask) {
    const before = mask;
    mask = new GridMask(mask.width, mask.height, mask.size);
    remember(before);
    update();
  }
};
$("suggest").onclick = () =>
  run(async () => {
    if (!mask) return;
    const before = mask;
    const s = await invoke<Selection>("suggest_blocks", {
      blockSize: mask.size,
    });
    mask = new GridMask(mask.width, mask.height, s.block_size, s.bits);
    remember(before);
    notice(`建议 ${mask.count} 个网格，请用打码 / 反打码笔刷修正。`);
  });
$("tab-protect").onclick = () => {
  operation = "protect";
  update();
};
$("tab-restore").onclick = () => {
  operation = "restore";
  update();
};
for (const t of ["paint", "erase", "hand"] as const)
  $(t).onclick = () => {
    tool = t;
    update();
  };
document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach(
  (b) =>
    (b.onclick = () => {
      if (mode === b.dataset.mode) return;
      mode = b.dataset.mode as Mode;
      if (mask) {
        mask = mask.resized(
          mode === "entropy"
            ? Number($<HTMLSelectElement>("block-size").value)
            : 1,
        );
        undo = [];
        redo = [];
      }
      $<HTMLInputElement>("alpha-consent").checked = false;
      update();
    }),
);
$("block-size").onchange = () => {
  if (mask) {
    const before = mask;
    mask = mask.resized(Number($<HTMLSelectElement>("block-size").value));
    remember(before);
    notice(
      "网格已重新划分；与旧选区相交的新网格全部选中，应用时重新生成结果。",
    );
    update();
  }
};
$("brush-size").oninput = () => {
  if (mode === "entropy")
    brushCells = Number($<HTMLInputElement>("brush-size").value);
  else pixelBrush = Number($<HTMLInputElement>("brush-size").value);
  $("brush-size-label").textContent =
    mode === "entropy" ? `${brushCells} 格` : `${pixelBrush} px`;
};
for (const id of ["password", "confirm", "alpha-consent"])
  for (const event of ["input", "change"])
    $(id).addEventListener(event, update);
$("reveal").onclick = () => {
  const p = $<HTMLInputElement>("password");
  p.type = p.type === "password" ? "text" : "password";
  $("reveal").setAttribute(
    "aria-label",
    p.type === "password" ? "显示密码" : "隐藏密码",
  );
  update();
};
$("apply").onclick = () =>
  run(async () => {
    if (!mask) return;
    try {
      const v = await invoke<View>("transform", {
        password: $<HTMLInputElement>("password").value,
        mode,
        selection: mask.serialize(),
        decrypt: operation === "restore",
        allowAlphaLoss: $<HTMLInputElement>("alpha-consent").checked,
        existingSelection: assumedBlockMask?.serialize() ?? null,
      });
      const was = operation;
      dirty = true;
      show(v, false);
      operation = "protect";
      notice(
        was === "restore"
          ? "还原完成，请另存为。"
          : "打码 / 反打码已应用，可继续涂抹。请另存为。",
      );
    } finally {
      $<HTMLInputElement>("password").value = "";
      $<HTMLInputElement>("confirm").value = "";
      $<HTMLInputElement>("alpha-consent").checked = false;
    }
  });
$("zoom-in").onclick = () => zoom(1.25);
$("zoom-out").onclick = () => zoom(1 / 1.25);
$("fit").onclick = fit;
$("actual").onclick = () => {
  if (camera) zoom(1 / camera.scale);
};
function local(e: { clientX: number; clientY: number }): Point {
  const r = viewport.getBoundingClientRect();
  return { x: e.clientX - r.left, y: e.clientY - r.top };
}
function cursor(p: Point) {
  const el = $("brush-cursor");
  if (!camera || !mask || tool === "hand" || space || busy) {
    el.hidden = true;
    return;
  }
  const q = camera.image(p);
  if (q.x < 0 || q.y < 0 || q.x >= mask.width || q.y >= mask.height) {
    el.hidden = true;
    return;
  }
  const center = camera.screen({
    x: (Math.floor(q.x / mask.size) + 0.5) * mask.size,
    y: (Math.floor(q.y / mask.size) + 0.5) * mask.size,
  });
  const size =
    (mode === "entropy" ? brushCells * mask.size : pixelBrush) * camera.scale;
  el.style.borderRadius = mode === "entropy" ? "0" : "50%";
  el.hidden = false;
  el.style.width = `${size}px`;
  el.style.height = `${size}px`;
  el.style.left = `${center.x - size / 2}px`;
  el.style.top = `${center.y - size / 2}px`;
  el.classList.toggle("erasing", tool === "erase");
}
function paintStroke(a: Point, b: Point, erase: boolean) {
  if (!mask) return;
  if (mode === "entropy") mask.stroke(a, b, brushCells, !erase);
  else mask.pixelStroke(a, b, pixelBrush, !erase);
  const radius =
    mode === "entropy" ? brushCells * mask.size : pixelBrush / 2 + 2;
  draw({
    left: Math.min(a.x, b.x) - radius,
    top: Math.min(a.y, b.y) - radius,
    right: Math.max(a.x, b.x) + radius,
    bottom: Math.max(a.y, b.y) + radius,
  });
}
viewport.onpointerdown = (e) => {
  if (!doc || !mask || !camera || busy || gesture || ![0, 1].includes(e.button))
    return;
  const p = local(e),
    q = camera.image(p),
    inside = q.x >= 0 && q.y >= 0 && q.x < doc.width && q.y < doc.height;
  const pan = e.button === 1 || space || tool === "hand" || !inside;
  if (!pan && operation === "restore" && mode !== "entropy") return;
  e.preventDefault();
  gesture = {
    id: e.pointerId,
    kind: pan ? "pan" : "paint",
    last: p,
    before: pan ? null : mask.clone(),
    erase: tool === "erase",
  };
  viewport.setPointerCapture(e.pointerId);
  if (!pan) {
    paintStroke(q, q, gesture.erase);
  }
  viewport.classList.toggle("panning", pan);
};
viewport.onpointermove = (e) => {
  const p = local(e);
  cursor(p);
  if (!gesture || gesture.id !== e.pointerId || !camera || !mask) return;
  if (gesture.kind === "pan") {
    camera.pan(p.x - gesture.last.x, p.y - gesture.last.y);
    layout();
  } else {
    paintStroke(camera.image(gesture.last), camera.image(p), gesture.erase);
  }
  gesture.last = p;
};
function finish(cancel = false) {
  if (!gesture) return;
  if (gesture.before) {
    if (cancel) mask = gesture.before;
    else remember(gesture.before);
  }
  gesture = null;
  update();
}
viewport.onpointerup = (e) => {
  if (gesture?.id === e.pointerId) finish();
};
viewport.onpointercancel = () => finish(true);
viewport.onlostpointercapture = () => finish(true);
viewport.onpointerleave = () => {
  $("brush-cursor").hidden = true;
};
viewport.addEventListener(
  "wheel",
  (e) => {
    if (!camera || busy) return;
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) zoom(Math.exp(-e.deltaY * 0.01), local(e));
    else {
      camera.pan(-e.deltaX, -e.deltaY);
      layout();
    }
  },
  { passive: false },
);
window.addEventListener("blur", () => {
  space = false;
  finish(true);
  $("brush-cursor").hidden = true;
});
document.addEventListener("keydown", (e) => {
  const input =
    e.target instanceof HTMLInputElement ||
    e.target instanceof HTMLSelectElement;
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "o") {
    e.preventDefault();
    if (!busy) void openFile();
  }
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
    e.preventDefault();
    if (!busy && doc) $("save").click();
  }
  if (input) return;
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") {
    e.preventDefault();
    if (e.shiftKey) redoStroke();
    else undoStroke();
  }
  if (e.code === "Space") {
    e.preventDefault();
    space = true;
    viewport.classList.add("panning");
  }
  if (e.key === "Escape") finish(true);
  if (e.key === "+" || e.key === "=") zoom(1.25);
  if (e.key === "-") zoom(1 / 1.25);
  if (e.key === "0") fit();
  if (e.key === "[" || e.key === "]") {
    if (mode === "entropy")
      brushCells = Math.max(
        1,
        Math.min(8, brushCells + (e.key === "]" ? 1 : -1)),
      );
    else
      pixelBrush = Math.max(
        1,
        Math.min(256, pixelBrush + (e.key === "]" ? 1 : -1)),
      );
    update();
  }
});
document.addEventListener("keyup", (e) => {
  if (e.code === "Space") {
    space = false;
    viewport.classList.toggle("panning", tool === "hand");
  }
});
if (!native)
  $("status").textContent =
    "浏览器界面预览 · 使用 npm run desktop 启动完整客户端";
update();
