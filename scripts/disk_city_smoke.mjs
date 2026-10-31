import { mkdir, writeFile, utimes } from 'node:fs/promises';
import { resolve, basename } from 'node:path';
import { cpus, totalmem } from 'node:os';
import assert from 'node:assert/strict';

export async function connect(port = 9223) {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const target = targets.find((item) => item.type === 'page' && item.title === 'Voldiq');
  assert.ok(target, 'Voldiq WebView2 target must be running');
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((done, reject) => {
    socket.onopen = done;
    socket.onerror = reject;
  });
  let sequence = 0;
  const keepAlive = setInterval(() => {}, 1000);
  const pending = new Map();
  const errors = [];
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.method === 'Runtime.exceptionThrown') errors.push(message.params.exceptionDetails);
    const callback = pending.get(message.id);
    if (callback) {
      pending.delete(message.id);
      message.error
        ? callback.reject(new Error(JSON.stringify(message.error)))
        : callback.resolve(message.result);
    }
  };
  socket.onclose = () => {
    clearInterval(keepAlive);
    for (const callback of pending.values())
      callback.reject(new Error('WebView2 debug connection closed'));
    pending.clear();
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++sequence;
      pending.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const until = async (expression, timeout = 15000) => {
    const started = Date.now();
    while (Date.now() - started < timeout) {
      const value = await evaluate(expression);
      if (value) return value;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw new Error(`Timed out: ${expression}`);
  };
  const click = async (text) => {
    await evaluate(
      `(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()===${JSON.stringify(text)}||b.title===${JSON.stringify(text)}||b.getAttribute('aria-label')===${JSON.stringify(text)});if(!b||b.disabled)throw Error('Button unavailable: '+${JSON.stringify(text)});b.click();return true})()`,
    );
  };
  const capture = async (path, width, height, dpi = 1) => {
    await send('Emulation.setDeviceMetricsOverride', {
      width,
      height,
      deviceScaleFactor: dpi,
      mobile: false,
    });
    await evaluate('new Promise(done=>requestAnimationFrame(()=>requestAnimationFrame(done)))');
    const shot = await send('Page.captureScreenshot', {
      format: 'png',
      captureBeyondViewport: false,
    });
    await writeFile(path, Buffer.from(shot.data, 'base64'));
  };
  await send('Runtime.enable');
  return {
    send,
    evaluate,
    until,
    click,
    capture,
    errors,
    close: () => {
      clearInterval(keepAlive);
      socket.close();
    },
  };
}

export async function fixture() {
  const root = resolve('target/disk-city-fixture');
  const groups = [
    ['Media', 'mp4'],
    ['Photographs', 'jpg'],
    ['Source', 'rs'],
    ['Archives', 'zip'],
    ['Documents', 'pdf'],
    ['Audio', 'flac'],
    ['Tiny files', 'json'],
  ];
  for (let g = 0; g < groups.length; g++) {
    const [folder, ext] = groups[g];
    const dir = resolve(root, folder);
    await mkdir(dir, { recursive: true });
    for (let i = 0; i < (g === 6 ? 300 : 14); i++) {
      const name =
        i === 0
          ? `A long filename with spaces & <literal markup> for inspection ${g}.${ext}`.replace(
              /[<>]/g,
              '_',
            )
          : `Sample-${String(i).padStart(3, '0')}.${ext}`;
      const path = resolve(dir, name);
      const length = g === 6 ? 32 + i : Math.round(((7 - g) * 90000) / (1 + i * 0.4));
      await writeFile(path, Buffer.alloc(length, g + i));
      const date = new Date(Date.UTC(2026 - ((g + i) % 8), (g + i) % 12, 1));
      await utimes(path, date, date);
    }
  }
  await mkdir(resolve(root, 'Empty folder'), { recursive: true });
  await writeFile(resolve(root, 'Empty file.txt'), '');
  await writeFile(resolve(root, 'Read me & inspect.txt'), 'Disposable verification fixture.');
  return root;
}

async function main() {
  const baseline = process.argv.includes('--baseline');
  const root = await fixture();
  const output = resolve('benchmarks/disk-city');
  await mkdir(output, { recursive: true });
  const client = await connect();
  try {
    const preferences = await client.evaluate("localStorage.getItem('voldiq-preferences')");
    await writeFile(resolve('target/disk-city-preferences.json'), JSON.stringify({ preferences }));
    await client.evaluate(
      `(()=>{const prefs=JSON.parse(localStorage.getItem('voldiq-preferences')||'{}');prefs.recentPaths=[${JSON.stringify(root)},...(prefs.recentPaths||[]).filter(p=>p!==${JSON.stringify(root)})].slice(0,8);prefs.theme='light';localStorage.setItem('voldiq-preferences',JSON.stringify(prefs));location.reload()})()`,
    );
    await client.until(
      `document.querySelector('.welcome-recent button')?.title===${JSON.stringify(root)}`,
    );
    await client.evaluate('document.querySelector(".welcome-recent button").click()');
    await client.until(
      "document.querySelector('.map canvas') && document.querySelector('.breadcrumbs') && !document.body.textContent.includes('Scanning…')",
      60000,
    );
    if (baseline) {
      await client.capture(resolve(output, 'baseline-1366.png'), 1366, 768);
      await writeFile(
        resolve(output, 'baseline.json'),
        JSON.stringify(
          {
            captured: new Date().toISOString(),
            root,
            platform: 'Windows native Tauri WebView2',
            text: await client.evaluate('document.body.innerText'),
          },
          null,
          2,
        ),
      );
      return;
    }
    await client.click('Explorer');
    await client.click('Skyline');
    await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
    const before = await client.evaluate('document.querySelector(".storage-map").dataset.layout');
    for (const choice of ['Files', 'Depth', 'Modified']) await client.click(choice);
    await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
    assert.equal(
      await client.evaluate('document.querySelector(".storage-map").dataset.layout'),
      before,
      'height changes must not relayout',
    );
    for (const choice of ['Size', 'Age', 'Type']) await client.click(choice);
    assert.equal(
      await client.evaluate('document.querySelector(".storage-map").dataset.layout'),
      before,
      'color changes must not relayout',
    );
    const scanStatus = await client.evaluate('document.querySelector(".scan-status").innerText');
    await client.click('Treemap');
    assert.equal(
      await client.evaluate('document.querySelector(".storage-map").dataset.layout'),
      before,
    );
    await client.click('Skyline');
    await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
    const captures = [];
    for (const [width, height] of [
      [1366, 768],
      [1920, 1080],
    ])
      for (const dpi of [1, 1.25, 1.5, 2]) {
        const name = `city-${width}-${dpi}.png`;
        await client.capture(resolve(output, name), width, height, dpi);
        captures.push(name);
      }
    await client.send('Emulation.setDeviceMetricsOverride', {
      width: 1366,
      height: 768,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await new Promise((done) => setTimeout(done, 350));
    await client.evaluate(
      `(()=>{const ctx=document.querySelector('.map canvas').getContext('2d');const original=ctx.clearRect.bind(ctx);window.__draws=0;ctx.clearRect=(...args)=>{window.__draws++;return original(...args)}})()`,
    );
    const idleBefore = await client.evaluate('window.__draws');
    await new Promise((done) => setTimeout(done, 600));
    const idleAfter = await client.evaluate('window.__draws');
    assert.equal(idleAfter, idleBefore, 'idle must not schedule redraws');
    await client.click('Zoom in');
    await client.until('window.__draws > 0');
    await client.click('Fit');
    const samples = [];
    const frame = () =>
      client.evaluate('new Promise(done=>requestAnimationFrame(()=>requestAnimationFrame(done)))');
    for (let run = 0; run < 5; run++)
      for (const variant of ['flat', 'basic', 'full']) {
        await client.click(variant === 'flat' ? 'Treemap' : 'Skyline');
        await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
        if (variant !== 'flat')
          await client.evaluate(
            `(()=>{const e=[...document.querySelectorAll('.map-camera-tools input')].find(e=>e.type==='checkbox');if(e.checked!==${variant === 'full'})e.click()})()`,
          );
        await client.click('Fit');
        await frame();
        samples.push({
          dataset: 'root',
          run,
          variant,
          ...(await client.evaluate(
            `(()=>{const m=document.querySelector('.storage-map');return {drawMs:Number(m.dataset.drawMs),layoutMs:Number(m.dataset.layoutMs),geometryMs:Number(m.dataset.geometryMs),shadowMs:Number(m.dataset.shadowMs),projectionMs:Number(m.dataset.projectionMs),orderingMs:Number(m.dataset.orderingMs),blocks:Number(m.dataset.blocks),jsHeapBytes:performance.memory?.usedJSHeapSize??null}})()`,
          )),
        });
      }
    assert.equal(
      await client.evaluate('document.querySelector(".scan-status").innerText'),
      scanStatus,
      'view switches must not restart scanning',
    );
    await client.evaluate(
      `(()=>{const e=document.querySelector('.search input');e.value='Sample';e.dispatchEvent(new Event('input',{bubbles:true}))})()`,
    );
    await frame();
    assert.equal(
      await client.evaluate('document.querySelector(".storage-map").dataset.layout'),
      before,
      'search highlights without relayout',
    );
    await client.evaluate(
      `(()=>{const e=document.querySelector('.search input');e.value='';e.dispatchEvent(new Event('input',{bubbles:true}))})()`,
    );
    await client.click('Inspector');
    await client.until("document.querySelector('.details-panel h2')?.textContent==='Media'");
    await client.capture(resolve(output, 'city-inspector-1366.png'), 1366, 768);
    captures.push('city-inspector-1366.png');
    await client.capture(resolve(output, 'city-inspector-1000.png'), 1000, 768);
    captures.push('city-inspector-1000.png');
    await client.evaluate(
      "document.querySelector('.navigation').dispatchEvent(new PointerEvent('pointerenter',{bubbles:false}))",
    );
    await client.capture(resolve(output, 'city-sidebar-1920.png'), 1920, 1080);
    captures.push('city-sidebar-1920.png');
    await client.evaluate("document.documentElement.dataset.theme='dark'");
    await client.capture(resolve(output, 'city-dark-1920.png'), 1920, 1080);
    captures.push('city-dark-1920.png');
    await client.evaluate("document.documentElement.dataset.theme='light'");
    await client.click('Explore folder');
    await client.until("document.querySelector('.breadcrumbs > span')?.textContent==='Media'");
    await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
    await client.click('Items · keyboard accessible list');
    await client.until("document.querySelector('.map-list-visible .file-name')");
    await client.evaluate("document.querySelector('.map-list-visible .file-name').click()");
    await client.until("document.querySelector('.details-panel')");
    assert.equal(
      await client.evaluate(
        "document.querySelector('.map-list-visible .table-row.selected .file-name')?.textContent.trim()",
      ),
      await client.evaluate("document.querySelector('.details-panel h2').textContent.trim()"),
      'list selection synchronizes inspector',
    );
    await client.capture(resolve(output, 'city-list-selection.png'), 1366, 768);
    captures.push('city-list-selection.png');
    await client.click('Close details');
    await client.click('Scan root');
    await client.until(
      "!document.querySelector('.breadcrumbs > span') && document.querySelector('.storage-map')?.dataset.ready==='true'",
    );
    await client.click('List');
    await client.until(
      "[...document.querySelectorAll('.file-name')].some(b=>b.textContent.includes('Tiny files'))",
    );
    await client.evaluate(
      "[...document.querySelectorAll('.file-name')].find(b=>b.textContent.includes('Tiny files')).dispatchEvent(new MouseEvent('dblclick',{bubbles:true}))",
    );
    await client.until("document.querySelector('.breadcrumbs > span')?.textContent==='Tiny files'");
    await client.click('Skyline');
    await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
    await client.capture(resolve(output, 'city-dense-1920.png'), 1920, 1080);
    captures.push('city-dense-1920.png');
    for (let run = 0; run < 5; run++)
      for (const variant of ['flat', 'basic', 'full']) {
        await client.click(variant === 'flat' ? 'Treemap' : 'Skyline');
        await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
        if (variant !== 'flat')
          await client.evaluate(
            `(()=>{const e=[...document.querySelectorAll('.map-camera-tools input')].find(e=>e.type==='checkbox');if(e.checked!==${variant === 'full'})e.click()})()`,
          );
        await client.click('Fit');
        await frame();
        samples.push({
          dataset: 'dense',
          run,
          variant,
          ...(await client.evaluate(
            `(()=>{const m=document.querySelector('.storage-map');return {drawMs:Number(m.dataset.drawMs),layoutMs:Number(m.dataset.layoutMs),geometryMs:Number(m.dataset.geometryMs),shadowMs:Number(m.dataset.shadowMs),projectionMs:Number(m.dataset.projectionMs),orderingMs:Number(m.dataset.orderingMs),blocks:Number(m.dataset.blocks),jsHeapBytes:performance.memory?.usedJSHeapSize??null}})()`,
          )),
        });
      }
    await client.click('Treemap');
    await client.evaluate(
      "window.__originalWorker=window.Worker;window.Worker=class{constructor(){throw new Error('Worker capability unavailable for fallback test')}}",
    );
    await client.click('Skyline');
    await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
    await client.evaluate('window.Worker=window.__originalWorker;delete window.__originalWorker');
    await client.click('Treemap');
    await client.evaluate(
      `(()=>{const Native=window.Worker;window.__originalWorker=Native;window.Worker=class extends Native{delivered=false;constructor(...args){super(...args);this.addEventListener('message',()=>{this.delivered=true})}terminate(){const late=!this.delivered&&this.onerror;super.terminate();if(late)setTimeout(()=>late(new Event('error')),10)}}})()`,
    );
    await client.click('Skyline');
    for (const choice of ['Files', 'Depth', 'Modified']) await client.click(choice);
    await client.until("document.querySelector('.storage-map')?.dataset.ready==='true'");
    await new Promise((done) => setTimeout(done, 120));
    assert.equal(
      await client.evaluate("document.querySelector('.storage-map')?.dataset.ready"),
      'true',
      'late errors from cancelled workers must not cancel the latest scene',
    );
    await client.evaluate('window.Worker=window.__originalWorker;delete window.__originalWorker');
    assert.equal(client.errors.length, 0, JSON.stringify(client.errors));
    await writeFile(
      resolve(output, 'native-smoke.json'),
      JSON.stringify(
        {
          captured: new Date().toISOString(),
          root,
          platform: 'Windows Tauri WebView2',
          cpu: cpus()[0].model,
          logicalProcessors: cpus().length,
          memoryBytes: totalmem(),
          captures,
          errors: client.errors,
          idleDraws: idleAfter - idleBefore,
          samples,
          dpiScope:
            'Viewport and device pixel ratios emulated inside native Windows WebView2; operating-system display scaling was not changed.',
          timingScope:
            'Canvas command submission on the UI thread. Geometry includes worker startup; no GPU presentation timing or speedup claim.',
          text: await client.evaluate('document.body.innerText'),
        },
        null,
        2,
      ),
    );
  } finally {
    client.close();
  }
}

if (process.argv[1] && basename(process.argv[1]) === 'disk_city_smoke.mjs')
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
