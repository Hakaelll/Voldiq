// Developer-only owned fixtures; production SVG uses browser Canvas measurements, not this runtime.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { SvgSurface } from '../frontend/lib/svg-surface.ts';
import { createStorageLayout } from '../frontend/lib/storage-map.ts';
import { buildCity, fitCamera } from '../frontend/lib/disk-city.ts';
import { paintFlat, paintCity, prepareShadows } from '../frontend/lib/map-painter.ts';
import { radialLayout } from '../frontend/lib/radial-map.ts';
import { paintRadial } from '../frontend/lib/radial-painter.ts';
import { computeVoronoi } from '../frontend/lib/voronoi.ts';
import { paintVoronoi } from '../frontend/lib/voronoi-painter.ts';

const runtimeModules = process.argv[2];
if (!runtimeModules || !path.isAbsolute(runtimeModules))
  throw new Error('Pass the absolute bundled node_modules directory.');
const require = createRequire(path.join(runtimeModules, 'package.json'));
const { createCanvas, loadImage } = require('@napi-rs/canvas');
const probes = path.resolve('target/probes');
if (fs.lstatSync(probes).isSymbolicLink() || !fs.statSync(probes).isDirectory())
  throw new Error('Expected an ordinary workspace probe directory.');
const output = fs.mkdtempSync(path.join(probes, 'svg-painters-'));
const nodes = [
  {
    id: 1,
    name: 'Projects',
    value: 70,
    directory: true,
    files: 20,
    children: [
      { id: 2, name: 'Source & notes.txt', value: 45, directory: false, files: 1 },
      { id: 3, name: '<script>archive.zip</script>', value: 25, directory: false, files: 1 },
    ],
  },
  { id: 4, name: 'Video.mp4', value: 20, directory: false, files: 1 },
  { id: 5, name: 'Other.bin', value: 10, directory: false, files: 1 },
];
const source = createStorageLayout(nodes, 3);
const width = 1000,
  height = 700;
const base = {
  rects: source.rects,
  camera: { x: 0, y: 0, scale: 1 },
  width,
  height,
  selected: null,
  hovered: null,
  matches: null,
  color: 'type',
  maximum: 70,
  now: 0,
  detail: true,
  style: { text: '#242c29', surface: '#fcfcf9', border: '#dce1d9', dark: false },
};
const files = [];
for (const mode of ['treemap', 'sunburst', 'circles', 'voronoi', 'city']) {
  const canvas = createCanvas(width, height);
  const context = canvas.getContext('2d');
  const vector = new SvgSurface(width, height, (text, font, baseline) => {
    context.save();
    context.font = font;
    context.textBaseline = baseline;
    const metrics = context.measureText(text);
    context.restore();
    return metrics;
  });
  let options = base;
  let paint;
  if (mode === 'city') {
    const scene = buildCity(source.rects, (node) => 50 + (node.value / 70) * 150);
    const shadows = prepareShadows(scene);
    options = { ...base, camera: fitCamera(scene.bounds, width, height) };
    paint = (target) => paintCity(target, scene, shadows, options);
  } else if (mode === 'treemap') paint = (target) => paintFlat(target, options);
  else if (mode === 'voronoi') {
    const cells = computeVoronoi(source.rects, source.total);
    paint = (target) => paintVoronoi(target, cells, options);
  } else {
    const radial = radialLayout(source, mode);
    paint = (target) => paintRadial(target, radial, options);
  }
  vector.fillStyle = base.style.surface;
  vector.fillRect(0, 0, width, height);
  const drawn = paint(vector);
  const svg = vector.finish();
  const svgPath = path.join(output, `${mode}.svg`);
  fs.writeFileSync(svgPath, svg, { flag: 'wx' });
  context.fillStyle = base.style.surface;
  context.fillRect(0, 0, width, height);
  if (paint(context) !== drawn) throw new Error('Painter draw counts differ between targets.');
  fs.writeFileSync(path.join(output, `${mode}-canvas.png`), canvas.toBuffer('image/png'), {
    flag: 'wx',
  });
  const image = await loadImage(Buffer.from(svg));
  const rendered = createCanvas(width, height);
  rendered.getContext('2d').drawImage(image, 0, 0);
  fs.writeFileSync(path.join(output, `${mode}-svg.png`), rendered.toBuffer('image/png'), {
    flag: 'wx',
  });
  files.push({
    mode,
    drawn,
    bytes: Buffer.byteLength(svg),
    sha256: crypto.createHash('sha256').update(svg).digest('hex').toUpperCase(),
  });
}
console.log(
  JSON.stringify(
    {
      checkedUtc: new Date().toISOString(),
      output,
      width,
      height,
      files,
      scope:
        'Owned synthetic source painters on Canvas and SVG targets, using bundled Skia font measurement and rendering. No native WebView, save workflow, metadata/privacy binding or Windows scaling acceptance.',
    },
    null,
    2,
  ),
);
