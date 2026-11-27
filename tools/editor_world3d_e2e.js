// End-to-end test of the 3D environment editor (the World view) and of
// 3D models for characters, in a real browser (WebGL included).
//
//   cargo build && node tools/editor_world3d_e2e.js [screenshot-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const { boxGlb } = require('./make_glb.js');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-w3d-'));
const out = process.argv[2] || tmp;
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets'));
  const main = path.join(tmp, 'assets', 'story.txt');
  fs.copyFileSync(path.join(root, 'assets', 'story.txt'), main);
  const worldDir = path.join(tmp, 'assets', 'world');
  const PORT = 7948;
  procs.push(spawn(exe, ['--editor', '--story', main, '--stories', path.join(tmp, 'stories'), '--comfy-dir', path.join(root, 'comfyui'), '--port', String(PORT), '--no-browser'], { stdio: 'ignore' }));
  await sleep(800);
  const b = browser = await chromium.launch({ args: ['--enable-unsafe-swiftshader'] });
  const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
  p.setDefaultTimeout(15000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('dialog', d => d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  const st = (method, url, body) => p.evaluate(([method, url, body]) => fetch(url, { method, headers: { 'Content-Type': 'application/json' }, body: body == null ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)) }).then(async r => [r.status, await r.json().catch(() => null)]), [method, url, body]);
  // where a point of the world is on the page, by the editor's own camera
  const onScreen = at => p.evaluate(at => { const m = world.vp, c = document.querySelector('#worldCanvas').getBoundingClientRect(); const x = m[0] * at[0] + m[4] * at[1] + m[8] * at[2] + m[12], y = m[1] * at[0] + m[5] * at[1] + m[9] * at[2] + m[13], w = m[3] * at[0] + m[7] * at[1] + m[11] * at[2] + m[15]; return [c.left + (x / w + 1) / 2 * c.width, c.top + (1 - y / w) / 2 * c.height]; }, at);
  const pixel = (x, y) => p.evaluate(([x, y]) => { drawWorld(); const g = world.gl.gl, c = g.canvas, r = c.getBoundingClientRect(), px = new Uint8Array(4); g.readPixels(Math.round((x - r.left) / r.width * c.width), Math.round(c.height - (y - r.top) / r.height * c.height), 1, 1, g.RGBA, g.UNSIGNED_BYTE, px); return [...px]; }, [x, y]);

  // ---- the view opens on a place the game built, drawn in 3D
  await p.click('#worldBtn');
  await p.waitForFunction(() => world.data && world.gl);
  assert(await p.evaluate(() => world.place === 'pantry' && !world.custom && world.data.parts.length > 100 && world.places.length === 7), "the World view opens on the story's first place, as the game built it");
  const sky = await pixel(...(await onScreen([0, 40, 0]))), lit = await p.evaluate(() => { drawWorld(); const g = world.gl.gl, w = g.canvas.width, h = g.canvas.height, px = new Uint8Array(w * h * 4); g.readPixels(0, 0, w, h, g.RGBA, g.UNSIGNED_BYTE, px); const seen = new Set(); for (let i = 0; i < px.length; i += 4 * 97) seen.add(px[i] >> 4 << 8 | px[i + 1] >> 4 << 4 | px[i + 2] >> 4); return seen.size; });
  assert(lit > 12, `the place is drawn with WebGL: ${lit} distinct colours across the view (the sky is ${sky.slice(0, 3)})`);
  if (out !== tmp) { fs.mkdirSync(out, { recursive: true }); await p.evaluate(() => { world.cam.yaw = 0.5; world.cam.pitch = 0.75; world.cam.dist = 13; drawWorld(); }); await p.screenshot({ path: path.join(out, 'editor_world.png') }); }
  await p.selectOption('#worldPlace', 'study');
  await p.waitForFunction(() => world.place === 'study' && world.data.parts.length > 500);
  assert(true, 'another place can be chosen (' + await p.evaluate(() => world.data.parts.length) + ' parts in the study)');

  // ---- start from an empty room, add things
  await p.locator('#worldLeft button', { hasText: 'An empty room' }).click();
  await p.waitForFunction(() => world.data.parts.length === 1 && world.dirty);
  await p.click('[data-add-part="box"]');
  assert(await p.evaluate(() => world.data.parts.length === 2 && world.sel.kind === 'part' && world.sel.i === 1 && world.data.parts[1].solid), 'a box is added where the view is looking, selected and solid');
  const field = axis => p.locator(`#worldRight input[data-axis="${axis}"]`);
  await field('x').fill('2'); await field('x').blur();
  await field('z').fill('-1'); await field('z').blur();
  await field('h').fill('2'); await field('h').blur();
  await field('y').fill('1'); await field('y').blur();
  await field('ry').fill('45'); await field('ry').blur();
  assert(await p.evaluate(() => JSON.stringify(world.data.parts[1].at) === '[2,1,-1]' && world.data.parts[1].size[1] === 2 && world.data.parts[1].rot[1] === 45), 'its place, size and turn are set by number');
  await p.locator('#worldRight input[type=color]').first().evaluate(el => { el.value = '#ff2020'; el.dispatchEvent(new Event('change')); });
  const at = await onScreen([2, 1, -1]);
  await p.evaluate(() => { world.sel = null; });
  const px = await pixel(...at);
  await p.evaluate(() => { world.sel = { kind: 'part', i: 1 }; drawWorld(); });
  assert(px[0] > 120 && px[0] > px[1] * 2 && px[0] > px[2] * 2, 'the box is drawn where it was put, in its colour: ' + px.slice(0, 3));
  // click empty space, then click the box: picking
  const floorAt = await onScreen([3.5, 0, 3]);
  await p.mouse.click(floorAt[0], floorAt[1]);
  assert(await p.evaluate(() => world.sel && world.sel.kind === 'part' && world.sel.i === 0), 'clicking the floor selects the floor');
  await p.mouse.click(at[0], at[1]);
  assert(await p.evaluate(() => world.sel && world.sel.kind === 'part' && world.sel.i === 1), 'clicking the box selects the box');
  // drag it across the floor
  const to = await onScreen([3, 1, 1.5]);
  await p.mouse.move(at[0], at[1]); await p.mouse.down(); await p.mouse.move((at[0] + to[0]) / 2, (at[1] + to[1]) / 2, { steps: 4 }); await p.mouse.move(to[0], to[1], { steps: 4 }); await p.mouse.up();
  const moved = await p.evaluate(() => world.data.parts[1].at);
  assert(Math.abs(moved[0] - 3) < 0.25 && Math.abs(moved[2] - 1.5) < 0.25 && moved[1] === 1, 'dragging moves it across the floor at its own height: ' + moved);
  await p.keyboard.press('Control+z');
  assert(await p.evaluate(() => JSON.stringify(world.data.parts[1].at) === '[2,1,-1]'), 'and undo puts it back');
  await p.evaluate(() => { world.sel = { kind: 'part', i: 1 }; renderWorldPanels(); });
  await p.click('#partDup');
  assert(await p.evaluate(() => world.data.parts.length === 3 && world.data.parts[2].color === '#ff2020' && world.data.parts[2].at[0] > 2), 'a part can be duplicated');
  await p.click('#partDel');
  assert(await p.evaluate(() => world.data.parts.length === 2), 'and deleted');
  // a lamp, the cast, the start, the floor
  await p.click('[data-add-part="lamp"]');
  assert(await p.evaluate(() => world.data.lights.length === 2 && world.sel.kind === 'light'), 'a lamp is added');
  await p.locator('[data-world-row="anchor1"]').click();
  await field('z').fill('-2'); await field('z').blur();
  await p.locator('[data-world-row="spawn0"]').click();
  await field('z').fill('4'); await field('z').blur();
  await p.evaluate(() => { world.sel = null; renderWorldPanels(); });
  await p.locator('#worldRight input[data-floor="x0"]').fill('-8'); await p.locator('#worldRight input[data-floor="x0"]').blur();
  assert(await p.evaluate(() => world.data.anchors[1][2] === -2 && world.data.spawn[2] === 4 && world.data.floor[0][0] === -8), 'where the cast stands, where you start and the walkable floor are set');
  if (out !== tmp) { fs.mkdirSync(out, { recursive: true }); await p.evaluate(() => { world.sel = { kind: 'part', i: 1 }; renderWorldPanels(); drawWorld(); }); await p.screenshot({ path: path.join(out, 'editor_world_new.png') }); }

  // ---- a model file
  const glb = path.join(tmp, 'urn.glb');
  fs.writeFileSync(glb, boxGlb('#3070c0'));
  await p.setInputFiles('#worldModelFile', glb);
  await p.waitForFunction(() => world.data.parts.some(q => q.shape === 'model'));
  assert(fs.existsSync(path.join(worldDir, 'models', 'urn.glb')) && await p.evaluate(() => world.models.includes('urn.glb') && world.data.parts.find(q => q.shape === 'model').file === 'urn.glb'), "an uploaded model is kept in the story's world/models folder and placed");

  // ---- save: this story's own version
  await p.keyboard.press('Control+s');
  await p.waitForFunction(() => !world.dirty && world.custom);
  const file = path.join(worldDir, 'study.json');
  const saved = JSON.parse(fs.readFileSync(file, 'utf8'));
  assert(saved.parts.length === 3 && saved.parts[1].color === '#ff2020' && saved.parts[1].solid === true && saved.parts[2].shape === 'model' && saved.lights.length === 2 && saved.spawn[2] === 4, 'saving writes world/study.json: ' + saved.parts.length + ' parts');
  assert((await p.locator('#worldPlace option:checked').textContent()).includes("this story's own"), "and the place is marked as this story's own");
  await p.selectOption('#worldPlace', 'pantry');
  await p.waitForFunction(() => world.place === 'pantry');
  await p.selectOption('#worldPlace', 'study');
  await p.waitForFunction(() => world.place === 'study' && world.custom && world.data.parts.length === 3);
  assert(true, 'coming back to it shows the saved version');
  if (out !== tmp) await p.screenshot({ path: path.join(out, 'editor_world_saved.png') });

  // ---- the server keeps places within bounds
  let [code, r] = await st('PUT', '/api/world?story=main&place=nowhere', saved);
  assert(code === 404, 'there is no saving a place the game does not have');
  [code, r] = await st('PUT', '/api/world?story=main&place=study', '{"hello":1}');
  assert(code === 400, 'nor something that is not a place');
  [code, r] = await st('PUT', '/api/world?story=main&place=study', { ...saved, parts: [{ shape: 'teapot', at: [1e30, 0, 0], size: [0, 0, 0], color: 'url(x)', file: '../../x' }, { shape: 'model', at: [0, 0, 0], size: [1, 1, 1], file: '../../../etc/passwd' }] });
  assert(code === 200 && r.place.parts[0].shape === 'box' && r.place.parts[0].color === '#ffffff' && r.place.parts[0].at[0] === 5000 && r.place.parts[1].shape === 'box' && r.place.parts[1].file === undefined, 'a description from elsewhere is brought within bounds');
  [code, r] = await st('POST', '/api/world/model', { story: 'main', name: '../evil.glb', data: boxGlb().toString('base64') });
  const [code2] = await st('POST', '/api/world/model', { story: 'main', name: 'fake.glb', data: Buffer.from('<html>').toString('base64') });
  assert(code === 400 && code2 === 400 && !fs.existsSync(path.join(tmp, 'assets', 'evil.glb')) && !fs.existsSync(path.join(worldDir, 'models', 'fake.glb')), 'a model must be a model, with a plain name');
  await st('PUT', '/api/world?story=main&place=study', saved);

  // ---- back to the game's own
  await p.selectOption('#worldPlace', 'pantry'); await p.waitForFunction(() => world.place === 'pantry');
  await p.selectOption('#worldPlace', 'study'); await p.waitForFunction(() => world.place === 'study' && world.custom);
  await p.locator('#worldLeft button', { hasText: "The game's own" }).click();
  await p.waitForFunction(() => !world.custom && world.data.parts.length > 500);
  assert(!fs.existsSync(file) && fs.readdirSync(path.join(worldDir, '.removed')).some(f => f.startsWith('study')), "going back to the game's own sets this story's version aside");
  await st('PUT', '/api/world?story=main&place=study', saved);

  // ---- characters in 3D: a figure, or a model
  await p.click('#worldBtn');
  await p.click('.tabs button[data-tab="cast"]');
  await p.locator('#leftBody .card .head').nth(1).click();
  await p.waitForSelector('#modelSel');
  assert((await p.locator('#modelSel option').allTextContents()).join('|').includes('Model: urn.glb'), "a character's card offers a cut-out, a 3D figure, or the story's model files");
  await p.selectOption('#modelSel', 'figure');
  await p.locator('#leftBody .card .head').nth(2).click();
  await p.waitForSelector('#modelSel');
  await p.selectOption('#modelSel', 'urn.glb');
  await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty);
  const text = fs.readFileSync(main, 'utf8');
  assert(/^model p \| figure$/m.test(text) && /^model v \| urn\.glb$/m.test(text), "written to the story as 'model p | figure' and 'model v | urn.glb'");

  // ---- a locked story's places stay as they are; an exported story carries them
  await st('POST', '/api/admin/lock', { story: 'main', locked: true });
  [code] = await st('PUT', '/api/world?story=main&place=study', saved);
  const [code3] = await st('POST', '/api/world/reset', { story: 'main', place: 'study' });
  assert(code === 423 && code3 === 423 && fs.existsSync(file), 'a locked story keeps its places');
  await st('POST', '/api/admin/lock', { story: 'main', locked: false });
  const bundle = await p.evaluate(() => fetch('/api/export?format=bundle&story=main').then(r => r.json()));
  assert(bundle.files['world/study.json'] && bundle.files['world/models/urn.glb'] && bundle.story.characters[1].model === 'figure', 'an exported story carries its places, its models and who uses them');
  [code, r] = await p.evaluate(b => fetch('/api/import', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(b) }).then(async r => [r.status, await r.json()]), { ...bundle, files: { ...bundle.files, 'world/nowhere.json': bundle.files['world/study.json'], 'world/pantry.json': Buffer.from('{"not":"a place"}').toString('base64'), 'world/models/x.exe': 'TVo=' } });
  const imported = path.join(tmp, 'stories', r.id, 'world');
  assert(code === 200 && fs.existsSync(path.join(imported, 'study.json')) && fs.existsSync(path.join(imported, 'models', 'urn.glb')) && !fs.existsSync(path.join(imported, 'nowhere.json')) && !fs.existsSync(path.join(imported, 'pantry.json')) && r.skipped.length === 3, 'and they come back on import; what is not a place or a model is left out');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
