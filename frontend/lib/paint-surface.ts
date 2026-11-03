/** The drawing operations used by storage maps; the real Canvas 2D context satisfies this contract. */
export interface PaintSurface extends Pick<
  CanvasRenderingContext2D,
  'fillStyle' | 'strokeStyle' | 'globalAlpha' | 'lineWidth' | 'font' | 'textAlign' | 'textBaseline'
> {
  beginPath(): void;
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  closePath(): void;
  rect(x: number, y: number, width: number, height: number): void;
  arc(
    x: number,
    y: number,
    radius: number,
    start: number,
    end: number,
    counterclockwise?: boolean,
  ): void;
  fill(): void;
  stroke(): void;
  clip(): void;
  fillRect(x: number, y: number, width: number, height: number): void;
  strokeRect(x: number, y: number, width: number, height: number): void;
  fillText(text: string, x: number, y: number): void;
  measureText(text: string): Pick<TextMetrics, 'width'>;
  save(): void;
  restore(): void;
  translate(x: number, y: number): void;
}
