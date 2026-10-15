export type Point = { x: number; y: number };
export type Selection = { block_size: number; bits: number[] };
export const BLOCK_SIZES = [8, 16, 32, 64] as const;

// Liang–Barsky clipping: includes every cell touched by a continuous stroke,
// even when pointer events skip many cells or the stroke crosses a corner.
function intersects(
  a: Point,
  b: Point,
  left: number,
  top: number,
  right: number,
  bottom: number,
): boolean {
  let lo = 0,
    hi = 1;
  for (const [p, d, min, max] of [
    [a.x, b.x - a.x, left, right],
    [a.y, b.y - a.y, top, bottom],
  ]) {
    if (d === 0) {
      if (p < min || p > max) return false;
      continue;
    }
    const t0 = (min - p) / d,
      t1 = (max - p) / d;
    lo = Math.max(lo, Math.min(t0, t1));
    hi = Math.min(hi, Math.max(t0, t1));
    if (lo > hi) return false;
  }
  return true;
}
export class GridMask {
  readonly columns: number;
  readonly rows: number;
  readonly bits: Uint8Array;
  constructor(
    readonly width: number,
    readonly height: number,
    readonly size: number,
    bits?: ArrayLike<number>,
  ) {
    if (
      (size !== 1 &&
        !BLOCK_SIZES.includes(size as (typeof BLOCK_SIZES)[number])) ||
      width <= 0 ||
      height <= 0
    )
      throw new Error("无效网格尺寸");
    this.columns = Math.ceil(width / size);
    this.rows = Math.ceil(height / size);
    this.bits = bits
      ? Uint8Array.from(bits)
      : new Uint8Array(Math.ceil((this.columns * this.rows) / 8));
    if (this.bits.length !== Math.ceil((this.columns * this.rows) / 8))
      throw new Error("网格数据尺寸错误");
  }
  clone() {
    return new GridMask(this.width, this.height, this.size, this.bits);
  }
  has(i: number) {
    return !!(this.bits[i >>> 3] & (1 << (i & 7)));
  }
  set(i: number, on: boolean) {
    if (on) this.bits[i >>> 3] |= 1 << (i & 7);
    else this.bits[i >>> 3] &= ~(1 << (i & 7));
  }
  get count() {
    let n = 0;
    for (let v of this.bits) {
      v = v - ((v >>> 1) & 0x55);
      v = (v & 0x33) + ((v >>> 2) & 0x33);
      n += (v + (v >>> 4)) & 0x0f;
    }
    return n;
  }
  selected(): number[] {
    const out: number[] = [];
    for (let i = 0; i < this.columns * this.rows; i++)
      if (this.has(i)) out.push(i);
    return out;
  }
  serialize(): Selection {
    return { block_size: this.size, bits: Array.from(this.bits) };
  }
  stroke(a: Point, b: Point, brushCells: number, on: boolean) {
    const r = ((Math.max(1, brushCells) - 1) * this.size) / 2;
    const minX = Math.max(0, Math.floor((Math.min(a.x, b.x) - r) / this.size)),
      maxX = Math.min(
        this.columns - 1,
        Math.floor((Math.max(a.x, b.x) + r) / this.size),
      );
    const minY = Math.max(0, Math.floor((Math.min(a.y, b.y) - r) / this.size)),
      maxY = Math.min(
        this.rows - 1,
        Math.floor((Math.max(a.y, b.y) + r) / this.size),
      );
    for (let y = minY; y <= maxY; y++)
      for (let x = minX; x <= maxX; x++)
        if (
          intersects(
            a,
            b,
            x * this.size - r,
            y * this.size - r,
            Math.min(this.width, (x + 1) * this.size) + r,
            Math.min(this.height, (y + 1) * this.size) + r,
          )
        )
          this.set(y * this.columns + x, on);
  }
  pixelStroke(a: Point, b: Point, diameter: number, on: boolean) {
    if (this.size !== 1) throw new Error("像素笔刷需要像素蒙版");
    if (diameter === 1) {
      this.stroke(a, b, 1, on);
      return;
    }
    const r = diameter / 2,
      dx = b.x - a.x,
      dy = b.y - a.y,
      len = dx * dx + dy * dy;
    for (
      let y = Math.max(0, Math.floor(Math.min(a.y, b.y) - r));
      y <= Math.min(this.height - 1, Math.floor(Math.max(a.y, b.y) + r));
      y++
    )
      for (
        let x = Math.max(0, Math.floor(Math.min(a.x, b.x) - r));
        x <= Math.min(this.width - 1, Math.floor(Math.max(a.x, b.x) + r));
        x++
      ) {
        const t =
          len === 0
            ? 0
            : Math.max(
                0,
                Math.min(
                  1,
                  ((x + 0.5 - a.x) * dx + (y + 0.5 - a.y) * dy) / len,
                ),
              );
        if (
          (x + 0.5 - a.x - t * dx) ** 2 + (y + 0.5 - a.y - t * dy) ** 2 <=
          r * r
        )
          this.set(y * this.width + x, on);
      }
  }
  resized(size: number) {
    const next = new GridMask(this.width, this.height, size);
    for (let i = 0; i < this.columns * this.rows; i++) {
      if (!this.has(i)) continue;
      const left = (i % this.columns) * this.size,
        top = Math.floor(i / this.columns) * this.size;
      for (
        let y = Math.floor(top / size);
        y <= Math.floor((Math.min(this.height, top + this.size) - 1) / size);
        y++
      )
        for (
          let x = Math.floor(left / size);
          x <= Math.floor((Math.min(this.width, left + this.size) - 1) / size);
          x++
        )
          next.set(y * next.columns + x, true);
    }
    return next;
  }
  equals(other: GridMask) {
    return (
      this.size === other.size &&
      this.bits.length === other.bits.length &&
      this.bits.every((v, i) => v === other.bits[i])
    );
  }
}
