import { historicalRects, historicalAbsence } from './history-geometry.ts';
import { utcDate } from './history-activity.ts';
import { bytes } from './types.ts';
import type { PaintSurface } from './paint-surface.ts';
import type { HistoryClip, HistoryGifRequest } from './history-clip.ts';

export const HISTORY_WIDTH = 1000,
  HISTORY_HEIGHT = 560;
export interface HistoryStyle {
  surface: string;
  text: string;
  secondary: string;
}

export function paintHistoryFrame(
  surface: PaintSurface,
  clip: HistoryClip,
  position: number,
  style: HistoryStyle,
): void {
  const frame = clip.frames[position];
  if (!frame) throw new Error('Missing recorded history frame.');
  surface.fillStyle = style.surface;
  surface.fillRect(0, 0, HISTORY_WIDTH, HISTORY_HEIGHT);
  surface.textAlign = 'left';
  surface.textBaseline = 'alphabetic';
  surface.fillStyle = style.text;
  surface.font = '600 17px "Segoe UI", sans-serif';
  surface.fillText('Voldiq · Recorded history', 16, 28);
  surface.font = '12px "Segoe UI", sans-serif';
  const fit = (value: string, width: number): string => {
    const characters: string[] = [];
    for (const character of value) {
      if (characters.length === 4096) break;
      characters.push(character);
    }
    const full = characters.join('');
    if (full === value && surface.measureText(full).width <= width) return full;
    let low = 0,
      high = characters.length;
    while (low < high) {
      const middle = Math.ceil((low + high) / 2);
      if (surface.measureText(characters.slice(0, middle).join('') + '\u2026').width <= width)
        low = middle;
      else high = middle - 1;
    }
    return characters.slice(0, low).join('') + '\u2026';
  };
  surface.fillText(fit(clip.root_path?.path ?? 'Recorded root · full paths withheld', 968), 16, 50);
  surface.fillStyle = style.secondary;
  surface.fillText(
    `Capture ${position + 1} of ${clip.frames.length} · saved ${utcDate(frame.captured)}`,
    16,
    72,
  );
  surface.fillText(`Measured ${utcDate(frame.observation?.completed_at ?? null)}`, 16, 92);
  surface.fillStyle = style.text;
  surface.fillText(
    `${frame.incomplete ? 'At least ' : ''}${frame.logical ?? 'unknown'} logical bytes`,
    16,
    112,
  );
  surface.save();
  surface.translate(0, 128);
  const rects = historicalRects(frame, Number(clip.maximum_logical), HISTORY_WIDTH);
  surface.lineWidth = 1;
  for (const rect of rects) {
    surface.fillStyle = rect.paint.fill;
    surface.fillRect(rect.x, rect.y, rect.width, rect.height);
    surface.strokeStyle = style.surface;
    surface.strokeRect(rect.x, rect.y, rect.width, rect.height);
    if (rect.width >= 80 && rect.height >= 34) {
      surface.fillStyle = rect.paint.ink;
      surface.font = '12px "Segoe UI", sans-serif';
      surface.fillText(fit(rect.name, rect.width - 12), rect.x + 6, rect.y + 15);
      surface.fillText(bytes(rect.value), rect.x + 6, rect.y + 29);
    }
  }
  if (!rects.length) {
    surface.fillStyle = style.secondary;
    surface.fillText(historicalAbsence(frame), 16, 40);
  }
  surface.restore();
  surface.fillStyle = style.secondary;
  surface.font = '12px "Segoe UI", sans-serif';
  surface.fillText(
    frame.incomplete
      ? 'Partial capture; sizes are lower bounds.'
      : 'Recorded summary; current contents are unverified.',
    16,
    476,
  );
  surface.fillText(
    clip.accounting === 'unknown'
      ? 'Legacy accounting comparability is unverified.'
      : `Scale: ${clip.maximum_logical} recorded logical bytes.`,
    16,
    496,
  );
  surface.fillText(
    'Up to 160 ranked entries + Other · no search/cloud filter · blank area is not free disk space.',
    16,
    516,
  );
  surface.fillText(
    'Playback timing is illustrative. GIF uses up to 256 colors per frame.',
    16,
    540,
  );
}

export async function prepareHistoryFrames(
  clip: HistoryClip,
  style: HistoryStyle,
  signal: AbortSignal,
  progress: (done: number, total: number) => void,
): Promise<Uint8Array[]> {
  if (clip.frames.length < 2 || clip.frames.length > 60) throw new Error('Choose 2–60 captures.');
  const canvas = document.createElement('canvas');
  const images: Uint8Array[] = [];
  let total = 0;
  try {
    canvas.width = HISTORY_WIDTH;
    canvas.height = HISTORY_HEIGHT;
    const context = canvas.getContext('2d', { colorSpace: 'srgb' });
    if (!context) throw new Error('Canvas history export is unavailable.');
    for (let index = 0; index < clip.frames.length; index++) {
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
      signal.throwIfAborted();
      paintHistoryFrame(context, clip, index, style);
      const blob = await new Promise<Blob>((resolve, reject) =>
        canvas.toBlob(
          (value) => (value ? resolve(value) : reject(new Error('History frame encoding failed.'))),
          'image/png',
        ),
      );
      signal.throwIfAborted();
      if (blob.size > 16 * 1024 * 1024 || total + blob.size > 32 * 1024 * 1024)
        throw new Error('The animation is too large. Choose fewer captures.');
      images.push(new Uint8Array(await blob.arrayBuffer()));
      total += blob.size;
      signal.throwIfAborted();
      progress(index + 1, clip.frames.length);
    }
    return images;
  } finally {
    canvas.width = canvas.height = 0;
  }
}

export function gifPacket(
  requestId: string,
  request: HistoryGifRequest,
  images: Uint8Array[],
): Uint8Array {
  if (
    images.length !== request.clip.snapshot_ids.length ||
    images.length < 2 ||
    images.length > 60 ||
    images.some((image) => !image.length || image.length > 16 * 1024 * 1024)
  )
    throw new Error('Invalid GIF frame set.');
  const total = images.reduce((sum, image) => sum + image.length, 0);
  const header = new TextEncoder().encode(
    JSON.stringify({ requestId, request, lengths: images.map((image) => image.length) }),
  );
  if (header.length > 64 * 1024 || total > 32 * 1024 * 1024)
    throw new Error('GIF packet exceeds its export limit.');
  const packet = new Uint8Array(4 + header.length + total);
  new DataView(packet.buffer).setUint32(0, header.length, true);
  packet.set(header, 4);
  let offset = 4 + header.length;
  for (const image of images) {
    packet.set(image, offset);
    offset += image.length;
  }
  return packet;
}
