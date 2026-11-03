import type { PaintSurface } from './paint-surface.ts';

type Style = Pick<
  PaintSurface,
  'fillStyle' | 'strokeStyle' | 'globalAlpha' | 'lineWidth' | 'font' | 'textAlign' | 'textBaseline'
>;
type State = Style & { tx: number; ty: number; clips: string[] };
type Measurement = Pick<TextMetrics, 'width' | 'actualBoundingBoxAscent'>;
export type TextMeasure = (text: string, font: string, baseline: CanvasTextBaseline) => Measurement;
const LIMIT = 8 * 1024 * 1024;
const WORK_LIMIT = 500_000;
const LABEL_LIMIT = 65_536;
const TAU = Math.PI * 2;

function number(value: number): string {
  if (!Number.isFinite(value) || Math.abs(value) > 10_000_000)
    throw new Error('SVG coordinate exceeds the supported range.');
  return String(value);
}
function xml(value: string): string {
  if (value.length > LABEL_LIMIT) throw new Error('SVG text exceeds its per-label resource limit.');
  let result = '';
  for (const character of value) {
    const code = character.codePointAt(0);
    if (code === undefined) throw new Error('Invalid SVG character.');
    if (
      (code < 32 && code !== 9 && code !== 10 && code !== 13) ||
      (code >= 0xd800 && code <= 0xdfff) ||
      code === 0xfffe ||
      code === 0xffff
    )
      throw new Error('SVG text contains a character unsupported by XML.');
    result +=
      character === '&'
        ? '&amp;'
        : character === '<'
          ? '&lt;'
          : character === '>'
            ? '&gt;'
            : character === '"'
              ? '&quot;'
              : character === "'"
                ? '&apos;'
                : character;
  }
  return result;
}
function color(value: Style['fillStyle']): { rgb: string; alpha: number } {
  if (typeof value !== 'string')
    throw new Error('This SVG renderer supports solid map colors only.');
  if (/^#[\da-f]{3,4}$|^#[\da-f]{6}(?:[\da-f]{2})?$/i.test(value)) {
    let hex = value.slice(1);
    if (hex.length < 5) hex = [...hex].map((part) => part + part).join('');
    return {
      rgb: '#' + hex.slice(0, 6),
      alpha: hex.length === 8 ? parseInt(hex.slice(6), 16) / 255 : 1,
    };
  }
  const match = /^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$/.exec(value);
  if (match && match.slice(1).every((part) => Number(part) <= 255)) return { rgb: value, alpha: 1 };
  throw new Error('SVG map color is not an explicit RGB value.');
}

/** On-demand vector target for the same painters as Canvas. No DOM, images, URLs or font discovery. */
export class SvgSurface implements PaintSurface {
  readonly width: number;
  readonly height: number;
  private measure: TextMeasure;
  fillStyle: Style['fillStyle'] = '#000000';
  strokeStyle: Style['strokeStyle'] = '#000000';
  globalAlpha = 1;
  lineWidth = 1;
  font = '10px sans-serif';
  textAlign: CanvasTextAlign = 'start';
  textBaseline: CanvasTextBaseline = 'alphabetic';
  private tx = 0;
  private ty = 0;
  private clips: string[] = [];
  private stack: State[] = [];
  private path: string[] = [];
  private point: { x: number; y: number } | null = null;
  private start: { x: number; y: number } | null = null;
  private definitions: string[] = [];
  private elements: string[] = [];
  private size = 0;
  private work = 0;
  constructor(width: number, height: number, measure: TextMeasure) {
    this.width = width;
    this.height = height;
    this.measure = measure;
    if (width <= 0 || height <= 0 || width > 8192 || height > 8192)
      throw new Error('SVG viewport must be positive and at most 8192 units per side.');
    number(width);
    number(height);
  }
  private step(): void {
    if (++this.work > WORK_LIMIT) throw new Error('SVG drawing exceeds its operation limit.');
  }
  private retain(value: string): string {
    this.size += value.length;
    if (this.size > LIMIT) throw new Error('SVG drawing exceeds its text resource limit.');
    return value;
  }
  private append(value: string): void {
    this.step();
    this.path.push(this.retain(value));
  }
  private emit(element: string): void {
    this.step();
    for (const id of this.clips) element = `<g clip-path="url(#${id})">${element}</g>`;
    this.elements.push(this.retain(element));
  }
  private opacity(): string {
    if (this.globalAlpha < 0 || this.globalAlpha > 1)
      throw new Error('SVG opacity is outside its supported range.');
    return number(this.globalAlpha);
  }
  private strokeAttributes(): string {
    if (this.lineWidth <= 0 || this.lineWidth > 8192)
      throw new Error('SVG stroke width is outside its supported range.');
    return `${this.paintAttributes('stroke', this.strokeStyle)} stroke-width="${number(this.lineWidth)}" stroke-miterlimit="10"`;
  }
  private paintAttributes(kind: 'fill' | 'stroke', value: Style['fillStyle']): string {
    const paint = color(value);
    return `${kind}="${paint.rgb}" ${kind}-opacity="${number(paint.alpha)}"`;
  }
  beginPath(): void {
    this.step();
    this.path = [];
    this.point = this.start = null;
  }
  moveTo(x: number, y: number): void {
    x += this.tx;
    y += this.ty;
    this.append(`M${number(x)} ${number(y)}`);
    this.point = this.start = { x, y };
  }
  lineTo(x: number, y: number): void {
    if (!this.point) {
      this.moveTo(x, y);
      return;
    }
    x += this.tx;
    y += this.ty;
    this.append(`L${number(x)} ${number(y)}`);
    this.point = { x, y };
  }
  closePath(): void {
    if (this.start) {
      this.append('Z');
      this.point = this.start;
    }
  }
  rect(x: number, y: number, width: number, height: number): void {
    this.moveTo(x, y);
    this.lineTo(x + width, y);
    this.lineTo(x + width, y + height);
    this.lineTo(x, y + height);
    this.closePath();
  }
  arc(
    x: number,
    y: number,
    radius: number,
    start: number,
    end: number,
    counterclockwise = false,
  ): void {
    for (const value of [x, y, radius, start, end]) number(value);
    if (radius < 0) throw new Error('SVG arc radius cannot be negative.');
    const first = { x: x + Math.cos(start) * radius, y: y + Math.sin(start) * radius };
    if (this.point) this.lineTo(first.x, first.y);
    else this.moveTo(first.x, first.y);
    if (radius === 0) return;
    let delta = end - start;
    if (!counterclockwise && delta >= TAU) delta = TAU;
    else if (counterclockwise && delta <= -TAU) delta = -TAU;
    else {
      delta %= TAU;
      if (counterclockwise && delta > 0) delta -= TAU;
      if (!counterclockwise && delta < 0) delta += TAU;
    }
    if (delta === 0) return;
    // SVG endpoints alone cannot express a complete circle; two half-arcs preserve the Canvas path.
    const pieces = Math.abs(delta) >= TAU ? 2 : 1;
    for (let i = 1; i <= pieces; i++) {
      const angle = start + (delta * i) / pieces;
      const target = {
        x: x + Math.cos(angle) * radius + this.tx,
        y: y + Math.sin(angle) * radius + this.ty,
      };
      this.append(
        `A${number(radius)} ${number(radius)} 0 ${Math.abs(delta / pieces) > Math.PI ? 1 : 0} ${delta > 0 ? 1 : 0} ${number(target.x)} ${number(target.y)}`,
      );
      this.point = target;
    }
  }
  fill(): void {
    if (this.path.length)
      this.emit(
        `<path d="${this.path.join(' ')}" ${this.paintAttributes('fill', this.fillStyle)} opacity="${this.opacity()}"/>`,
      );
  }
  stroke(): void {
    if (this.path.length)
      this.emit(
        `<path d="${this.path.join(' ')}" fill="none" ${this.strokeAttributes()} opacity="${this.opacity()}"/>`,
      );
  }
  clip(): void {
    if (this.clips.length >= 32) throw new Error('SVG clip nesting exceeds its supported limit.');
    const id = `map-clip-${this.definitions.length}`;
    this.definitions.push(
      this.retain(
        `<clipPath id="${id}" clipPathUnits="userSpaceOnUse"><path d="${this.path.join(' ')}"/></clipPath>`,
      ),
    );
    this.clips = [...this.clips, id];
  }
  fillRect(x: number, y: number, width: number, height: number): void {
    for (const value of [x, y, width, height]) number(value);
    if (!width || !height) return;
    if (width < 0) {
      x += width;
      width = -width;
    }
    if (height < 0) {
      y += height;
      height = -height;
    }
    this.emit(
      `<rect x="${number(x + this.tx)}" y="${number(y + this.ty)}" width="${number(width)}" height="${number(height)}" ${this.paintAttributes('fill', this.fillStyle)} opacity="${this.opacity()}"/>`,
    );
  }
  strokeRect(x: number, y: number, width: number, height: number): void {
    const ax = number(x + this.tx),
      ay = number(y + this.ty),
      bx = number(x + width + this.tx),
      by = number(y + height + this.ty);
    this.emit(
      `<path d="M${ax} ${ay} L${bx} ${ay} L${bx} ${by} L${ax} ${by} Z" fill="none" ${this.strokeAttributes()} opacity="${this.opacity()}"/>`,
    );
  }
  private measurement(text: string, baseline: CanvasTextBaseline): Measurement {
    this.step();
    if (text.length > LABEL_LIMIT)
      throw new Error('SVG text exceeds its per-label resource limit.');
    const value = this.measure(text, this.font, baseline);
    number(value.width);
    number(value.actualBoundingBoxAscent);
    if (value.width < 0) throw new Error('SVG text measurement cannot be negative.');
    return value;
  }
  measureText(text: string): Pick<TextMetrics, 'width'> {
    return this.measurement(text, this.textBaseline);
  }
  fillText(text: string, x: number, y: number): void {
    const font = /^(?:([1-9]00) )?(\d+(?:\.\d+)?)px (?:("Segoe UI"), )?sans-serif$/.exec(this.font);
    if (!font || Number(font[2]) < 1 || Number(font[2]) > 128)
      throw new Error('SVG font is outside the shared map font set.');
    const anchor =
      this.textAlign === 'center'
        ? 'middle'
        : this.textAlign === 'right' || this.textAlign === 'end'
          ? 'end'
          : 'start';
    const current = this.measurement(text, this.textBaseline);
    const alphabetic =
      this.textBaseline === 'alphabetic' ? current : this.measurement(text, 'alphabetic');
    const offset = alphabetic.actualBoundingBoxAscent - current.actualBoundingBoxAscent;
    this.emit(
      `<text x="${number(x + this.tx)}" y="${number(y + this.ty + offset)}" ${this.paintAttributes('fill', this.fillStyle)} opacity="${this.opacity()}" font-family="${font[3] ? 'Segoe UI, sans-serif' : 'sans-serif'}" font-size="${number(Number(font[2]))}" font-weight="${font[1] ?? '400'}" text-anchor="${anchor}" dominant-baseline="alphabetic" xml:space="preserve">${xml(text.replace(/[\t\n\f\r]/g, ' '))}</text>`,
    );
  }
  save(): void {
    this.step();
    if (this.stack.length >= 32) throw new Error('SVG state nesting exceeds its supported limit.');
    this.stack.push({
      fillStyle: this.fillStyle,
      strokeStyle: this.strokeStyle,
      globalAlpha: this.globalAlpha,
      lineWidth: this.lineWidth,
      font: this.font,
      textAlign: this.textAlign,
      textBaseline: this.textBaseline,
      tx: this.tx,
      ty: this.ty,
      clips: [...this.clips],
    });
  }
  restore(): void {
    this.step();
    const state = this.stack.pop();
    if (state) {
      this.fillStyle = state.fillStyle;
      this.strokeStyle = state.strokeStyle;
      this.globalAlpha = state.globalAlpha;
      this.lineWidth = state.lineWidth;
      this.font = state.font;
      this.textAlign = state.textAlign;
      this.textBaseline = state.textBaseline;
      this.tx = state.tx;
      this.ty = state.ty;
      this.clips = state.clips;
    }
  }
  translate(x: number, y: number): void {
    this.step();
    number(x);
    number(y);
    this.tx += x;
    this.ty += y;
    number(this.tx);
    number(this.ty);
  }
  finish(): string {
    if (this.stack.length) throw new Error('SVG drawing has unbalanced saved state.');
    const document = `<svg xmlns="http://www.w3.org/2000/svg" width="${number(this.width)}" height="${number(this.height)}" viewBox="0 0 ${number(this.width)} ${number(this.height)}" overflow="hidden"><title>Voldiq map drawing</title><desc>Vector rendering only. Export integration supplies scope, filters, dates, coverage and limitations.</desc><defs>${this.definitions.join('')}</defs>${this.elements.join('')}</svg>`;
    if (document.length > LIMIT) throw new Error('SVG document exceeds its text resource limit.');
    return document;
  }
}
