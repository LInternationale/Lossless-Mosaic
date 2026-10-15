import type { Point } from "./grid";
export class ImageViewport {
  scale = 1;
  x = 0;
  y = 0;
  constructor(
    readonly width: number,
    readonly height: number,
  ) {}
  fit(width: number, height: number) {
    this.scale = Math.min(
      Math.max(1, width - 64) / this.width,
      Math.max(1, height - 84) / this.height,
    );
    this.x = (width - this.width * this.scale) / 2;
    this.y = (height - this.height * this.scale) / 2 - 14;
  }
  image(p: Point): Point {
    return { x: (p.x - this.x) / this.scale, y: (p.y - this.y) / this.scale };
  }
  screen(p: Point): Point {
    return { x: p.x * this.scale + this.x, y: p.y * this.scale + this.y };
  }
  zoom(factor: number, anchor: Point) {
    const before = this.image(anchor);
    this.scale = Math.max(0.01, Math.min(16, this.scale * factor));
    this.x = anchor.x - before.x * this.scale;
    this.y = anchor.y - before.y * this.scale;
  }
  pan(dx: number, dy: number) {
    this.x += dx;
    this.y += dy;
  }
}
