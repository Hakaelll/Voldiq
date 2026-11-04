// Developer-only rendering of an owned clip, through the production historical painter.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import {
  paintHistoryFrame,
  HISTORY_WIDTH,
  HISTORY_HEIGHT,
} from '../frontend/lib/history-export.ts';

const [modules, directory] = process.argv.slice(2);
if (!modules || !directory || !path.isAbsolute(modules) || !path.isAbsolute(directory))
  throw new Error('Pass absolute developer module and owned fixture directories.');
const stat = fs.lstatSync(directory);
if (!stat.isDirectory() || stat.isSymbolicLink())
  throw new Error('Expected an ordinary fixture directory.');
const source = path.join(directory, 'clip.json');
if (!fs.lstatSync(source).isFile() || fs.lstatSync(source).isSymbolicLink())
  throw new Error('Expected an ordinary clip fixture.');
const clip = JSON.parse(fs.readFileSync(source, 'utf8'));
if (clip.schema_version !== 1 || clip.frames.length !== 2 || clip.paths_included)
  throw new Error('Expected two private synthetic capture summaries.');
const require = createRequire(path.join(modules, 'package.json'));
const { createCanvas } = require('@napi-rs/canvas');
const style = { surface: '#fcfcf9', text: '#242c29', secondary: '#505c54' };
for (let index = 0; index < clip.frames.length; index++) {
  const canvas = createCanvas(HISTORY_WIDTH, HISTORY_HEIGHT);
  paintHistoryFrame(canvas.getContext('2d'), clip, index, style);
  fs.writeFileSync(path.join(directory, `history-${index}.png`), canvas.toBuffer('image/png'), {
    flag: 'wx',
  });
}
process.stdout.write(JSON.stringify({ frames: 2, width: HISTORY_WIDTH, height: HISTORY_HEIGHT }));
