import {
  paintHistoryFrame,
  HISTORY_WIDTH,
  HISTORY_HEIGHT,
  type HistoryStyle,
} from './history-export.ts';
import type { HistoryClip, HistoryWebmRequest } from './history-clip.ts';

/** Advertised support only; construction and recording can still fail. Called on demand. */
export function historyVideoMime(): string | null {
  if (
    typeof MediaRecorder === 'undefined' ||
    typeof HTMLCanvasElement === 'undefined' ||
    typeof HTMLCanvasElement.prototype.captureStream !== 'function'
  )
    return null;
  try {
    return (
      ['video/webm;codecs=vp8', 'video/webm;codecs=vp9'].find((mime) =>
        MediaRecorder.isTypeSupported(mime),
      ) ?? null
    );
  } catch {
    return null;
  }
}

function wait(milliseconds: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    signal.throwIfAborted();
    const abort = (): void => {
      clearTimeout(timer);
      reject(signal.reason);
    };
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', abort);
      resolve();
    }, milliseconds);
    signal.addEventListener('abort', abort, { once: true });
  });
}

export async function recordHistoryVideo(
  clip: HistoryClip,
  style: HistoryStyle,
  delay: HistoryWebmRequest['delay_centiseconds'],
  signal: AbortSignal,
  progress: (done: number, total: number) => void,
): Promise<Uint8Array> {
  const mime = historyVideoMime();
  if (!mime) throw new Error('WebM recording is unavailable in this runtime. Choose GIF.');
  if (clip.frames.length < 2 || clip.frames.length > 60) throw new Error('Choose 2–60 captures.');
  const canvas = document.createElement('canvas');
  const controller = new AbortController();
  let stream: MediaStream | null = null,
    recorder: MediaRecorder | null = null;
  let repaint: ReturnType<typeof setInterval> | undefined;
  let failure: Error | null = null,
    finishing = false,
    total = 0;
  const chunks: Blob[] = [];
  const abort = (): void => controller.abort(signal.reason);
  const fail = (message: string): void => {
    failure = new Error(message);
    controller.abort(failure);
  };
  const visibility = (): void => {
    if (document.hidden)
      fail('Recording stopped because the window was hidden. Keep it visible and retry.');
  };
  signal.addEventListener('abort', abort, { once: true });
  document.addEventListener('visibilitychange', visibility);
  try {
    signal.throwIfAborted();
    visibility();
    controller.signal.throwIfAborted();
    canvas.width = HISTORY_WIDTH;
    canvas.height = HISTORY_HEIGHT;
    const context = canvas.getContext('2d', { alpha: false, colorSpace: 'srgb' });
    if (!context) throw new Error('Canvas history export is unavailable.');
    paintHistoryFrame(context, clip, 0, style, 'webm');
    stream = canvas.captureStream(30);
    if (stream.getVideoTracks().length !== 1 || stream.getAudioTracks().length)
      throw new Error('Expected a canvas-only video stream.');
    recorder = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: 1_500_000 });
    const stopped = new Promise<void>((resolve) => {
      if (!recorder) throw new Error('Missing recorder.');
      recorder.onstop = () => {
        if (!finishing) fail('The runtime stopped recording early. Retry or choose GIF.');
        resolve();
      };
    });
    recorder.onerror = () => fail('The runtime could not encode WebM. Retry or choose GIF.');
    recorder.ondataavailable = (event) => {
      total += event.data.size;
      if (total > 32 * 1024 * 1024) fail('The recording is too large. Choose fewer captures.');
      else if (!controller.signal.aborted && event.data.size) chunks.push(event.data);
    };
    recorder.start(250);
    // Canvas streams capture paints; repeat the current image so held captures retain duration.
    repaint = setInterval(() => context.drawImage(canvas, 0, 0), 100);
    for (let index = 0; index < clip.frames.length; index++) {
      controller.signal.throwIfAborted();
      if (recorder.state !== 'recording') throw new Error('The recorder is no longer active.');
      paintHistoryFrame(context, clip, index, style, 'webm');
      await wait(delay * 10, controller.signal);
      progress(index + 1, clip.frames.length);
    }
    finishing = true;
    recorder.stop();
    await Promise.race([
      stopped,
      wait(5000, controller.signal).then(() => {
        throw new Error('The recorder did not finish in time.');
      }),
    ]);
    controller.signal.throwIfAborted();
    if (!total) throw new Error('The runtime returned an empty recording.');
    const bytes = new Uint8Array(await new Blob(chunks, { type: mime }).arrayBuffer());
    controller.signal.throwIfAborted();
    return bytes;
  } finally {
    signal.removeEventListener('abort', abort);
    document.removeEventListener('visibilitychange', visibility);
    clearInterval(repaint);
    controller.abort();
    try {
      if (recorder) {
        recorder.onstop = recorder.onerror = recorder.ondataavailable = null;
        if (recorder.state !== 'inactive') recorder.stop();
      }
    } finally {
      stream?.getTracks().forEach((track) => track.stop());
      canvas.width = canvas.height = 0;
    }
  }
}
