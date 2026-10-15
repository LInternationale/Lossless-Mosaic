import test from "node:test";
import assert from "node:assert/strict";
import { GridMask } from "../artifacts/test-ui/grid.js";
import { ImageViewport } from "../artifacts/test-ui/viewport.js";

test("fast strokes include every crossed cell and repeated paint is idempotent", () => {
  const m = new GridMask(130, 65, 16);
  m.stroke({ x: 1, y: 7 }, { x: 129, y: 7 }, 1, true);
  assert.equal(m.count, 9);
  const before = m.clone();
  m.stroke({ x: 129, y: 7 }, { x: 1, y: 7 }, 1, true);
  assert.ok(m.equals(before));
  m.stroke({ x: 33, y: 7 }, { x: 63, y: 7 }, 1, false);
  assert.equal(m.count, 7);
  assert.equal(m.has(2), false);
  assert.equal(m.has(3), false);
  m.stroke({ x: 34, y: 7 }, { x: 62, y: 7 }, 1, true);
  assert.ok(m.equals(before));
});
test("diagonal and corner crossings do not skip cells", () => {
  const m = new GridMask(64, 64, 16);
  m.stroke({ x: 1, y: 1 }, { x: 63, y: 63 }, 1, true);
  for (let i = 0; i < 4; i++) assert.ok(m.has(i * 4 + i));
  assert.ok(m.has(1));
  assert.ok(m.has(4));
  assert.equal(m.count, 10);
});
test("brush width, clipping and partial edge cells", () => {
  const narrow = new GridMask(67, 39, 16),
    wide = narrow.clone();
  narrow.stroke({ x: 23, y: 23 }, { x: 23, y: 23 }, 1, true);
  wide.stroke({ x: 23, y: 23 }, { x: 23, y: 23 }, 3, true);
  assert.ok(wide.count > narrow.count);
  wide.stroke({ x: -100, y: -100 }, { x: 1000, y: 1000 }, 8, true);
  assert.equal(wide.count, 15);
  const edge = new GridMask(67, 39, 16);
  edge.stroke({ x: 66, y: 38 }, { x: 66, y: 38 }, 1, true);
  assert.deepEqual(edge.selected(), [14]);
});
test("regridding preserves covered area and snapshots support undo including erased cells", () => {
  const m = new GridMask(67, 39, 32);
  m.set(0, true);
  const fine = m.resized(8);
  assert.equal(fine.count, 16);
  assert.ok(fine.resized(32).equals(m));
  const snapshot = fine.clone();
  fine.stroke({ x: 1, y: 1 }, { x: 30, y: 1 }, 1, false);
  assert.equal(snapshot.count, 16);
  assert.equal(fine.count, 12);
  assert.equal(m.resized(64).count, 1);
  assert.deepEqual(
    new GridMask(67, 39, 32, m.serialize().bits).selected(),
    [0],
  );
});
test("zoom anchors stay fixed and panning preserves image coordinate mapping", () => {
  const v = new ImageViewport(640, 400);
  v.fit(900, 600);
  const pointer = { x: 287, y: 183 },
    image = v.image(pointer);
  v.zoom(2.5, pointer);
  const after = v.image(pointer);
  assert.ok(Math.abs(after.x - image.x) < 1e-10);
  assert.ok(Math.abs(after.y - image.y) < 1e-10);
  const p = v.screen(image);
  assert.ok(Math.abs(p.x - pointer.x) < 1e-10);
  v.pan(123, -57);
  assert.ok(Math.abs(v.screen(image).x - pointer.x - 123) < 1e-10);
  const m = new GridMask(640, 400, 32);
  const screen = v.screen({ x: 45, y: 55 });
  m.stroke(v.image(screen), v.image(screen), 1, true);
  assert.deepEqual(m.selected(), [21]);
});
test("zoom clamps and validates grid shape", () => {
  const v = new ImageViewport(640, 400);
  v.zoom(1e10, { x: 0, y: 0 });
  assert.equal(v.scale, 16);
  v.zoom(1e-30, { x: 0, y: 0 });
  assert.equal(v.scale, 0.01);
  assert.throws(() => new GridMask(32, 32, 7));
  assert.throws(() => new GridMask(32, 32, 8, [0]));
});

test("pixel brush selects a single pixel without selecting its neighbors", () => {
  const m = new GridMask(67, 39, 1);
  m.pixelStroke({ x: 17.3, y: 19.6 }, { x: 17.3, y: 19.6 }, 1, true);
  assert.deepEqual(m.selected(), [19 * 67 + 17]);
  m.pixelStroke({ x: 17.3, y: 19.6 }, { x: 17.3, y: 19.6 }, 1, true);
  assert.equal(m.count, 1);
  m.pixelStroke({ x: 17.3, y: 19.6 }, { x: 17.3, y: 19.6 }, 1, false);
  assert.equal(m.count, 0);
});
test("round pixel brush and overlapping erase preserve unpainted pixels", () => {
  const m = new GridMask(64, 64, 1);
  m.pixelStroke({ x: 20, y: 20 }, { x: 20, y: 20 }, 12, true);
  assert.ok(m.has(20 * 64 + 20));
  assert.equal(m.has(14 * 64 + 14), false);
  assert.equal(m.has(20 * 64 + 26), false);
  const before = m.clone();
  m.pixelStroke({ x: 20, y: 20 }, { x: 20, y: 20 }, 12, true);
  assert.ok(m.equals(before));
  m.pixelStroke({ x: 20, y: 20 }, { x: 28, y: 20 }, 3, false);
  assert.equal(m.has(20 * 64 + 20), false);
  assert.ok(m.has(16 * 64 + 20));
  const v = new ImageViewport(64, 64);
  v.scale = 3.7;
  v.pan(51, -12);
  const p = v.screen({ x: 40.3, y: 50.6 });
  m.pixelStroke(v.image(p), v.image(p), 1, true);
  assert.ok(m.has(50 * 64 + 40));
});

test("block restore keeps decryption tools after partial application", async () => {
  const { brushLabels, nextOperation } =
    await import("../artifacts/test-ui/editor-state.js");
  const operation = nextOperation("restore", true);
  assert.equal(operation, "restore");
  const labels = brushLabels(operation === "restore", true);
  assert.equal(labels.paint, "解密");
  assert.equal(labels.erase, "取消解密");
  const mask = new GridMask(64, 64, 16);
  mask.set(0, true);
  mask.set(1, true);
  const before = mask.clone();
  mask.stroke({ x: 1, y: 1 }, { x: 1, y: 1 }, 1, false);
  assert.equal(mask.has(0), false);
  assert.equal(mask.has(1), true);
  assert.equal(before.count, 2);
  assert.equal(nextOperation("restore", false), "protect");
  assert.equal(brushLabels(false, true).paint, "打码");
});
test("preview explains warnings without disabling intentional continuation", async () => {
  const { previewText } = await import("../artifacts/test-ui/editor-state.js");
  const text = previewText({
    encrypted_blocks: 2,
    plain_blocks: 8,
    encrypted_entropy: { min: 6.8, max: 7.1, mean: 7 },
    plain_entropy: { min: 6.7, max: 7.1, mean: 6.9 },
    entropy_auc: 0.55,
    missed_blocks: 1,
    false_positive_blocks: 3,
    uncertain_blocks: 1,
    warning: true,
  });
  assert.match(text, /55.0%/);
  assert.match(text, /误选 3/);
  assert.match(text, /漏选 1/);
});
