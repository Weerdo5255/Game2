// End-to-end test of choosing how places are shown (paintings or a 3D
// world), for a whole story and place by place, in a real browser.
//
//   cargo build && node tools/editor_world_e2e.js
//
// The 3D engine itself is not needed here: the choice is part of the story.
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-world-'));
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets'));
  const main = path.join(tmp, 'assets', 'story.txt');
  fs.copyFileSync(path.join(root, 'assets', 'story.txt'), main);
  const PORT = 7942;
  procs.push(spawn(exe, ['--editor', '--story', main, '--stories', path.join(tmp, 'stories'), '--comfy-dir', path.join(root, 'comfyui'), '--port', String(PORT), '--no-browser'], { stdio: 'ignore' }));
  await sleep(800);
  const b = browser = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1600, height: 950 } });
  p.setDefaultTimeout(15000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('dialog', d => d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  const save = async () => { await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty); return fs.readFileSync(main, 'utf8'); };
  const chips = () => p.evaluate(() => { enterChapter(doc.chapters[1].name); return [...document.querySelectorAll('.node .chip')].filter(c => c.textContent === '3D').length; });

  assert(await p.evaluate(() => doc.render === '' && Object.keys(doc.locales).length === 0), 'a story starts as paintings');
  await p.click('.tabs button[data-tab="scenes"]');
  await p.selectOption('#renderSel', '3d');
  let text = await save();
  assert(/^render 3d$/m.test(text), "the whole story can be set to 3D: 'render 3d' in the story file");
  assert(await chips() > 0, 'scenes in 3D places are marked on the graph');
  // one place back to a painting, from the list
  await p.evaluate(() => document.querySelector('#placeModes').open = true);
  await p.selectOption('[data-place-mode="study"]', '2d');
  text = await save();
  assert(/^render 3d$/m.test(text) && /^render study 2d$/m.test(text), 'one place can differ from the story');
  // and from a Background block
  await p.evaluate(() => { const si = doc.scenes.findIndex(s => s.items.some(i => i.t === 'bg' && i.name === 'dock')); select(si, doc.scenes[si].items.findIndex(i => i.t === 'bg'), true); });
  await p.waitForSelector('#insp [data-place-mode="dock"]');
  await p.selectOption('#insp [data-place-mode="dock"]', '2d');
  assert(await p.evaluate(() => doc.locales.dock === '2d' && !is3d('dock') && is3d('pantry') && !is3d('study')), 'a Background block sets how its place is shown');
  await p.keyboard.press('Control+z');
  assert(await p.evaluate(() => !doc.locales.dock && is3d('dock')), 'and that can be undone');
  // the other way round: a painted story with one 3D place
  await p.click('.tabs button[data-tab="scenes"]');
  await p.selectOption('#renderSel', '');
  await p.evaluate(() => document.querySelector('#placeModes').open = true);
  await p.selectOption('[data-place-mode="study"]', '');
  await p.selectOption('[data-place-mode="vista"]', '3d');
  text = await save();
  assert(!/^render 3d$/m.test(text) && /^render vista 3d$/m.test(text) && !/render study/.test(text), 'or a painted story can have single places in 3D');
  assert(await p.evaluate(() => is3d('vista') && !is3d('pantry')), 'the page agrees');
  const note = await p.locator('#leftBody .pmsg').allTextContents();
  const has3d = await p.evaluate(() => meta.world3d);
  assert(has3d || note.some(t => t.includes('no 3D engine')), 'a build without the 3D engine says so' + (has3d ? ' (this build has it)' : ''));
  // the choice travels with an exported story
  const exported = await p.evaluate(() => fetch('/api/export?format=json&story=main').then(r => r.json()));
  assert(exported.story.locales.vista === '3d' && exported.story.render === '', 'and is part of an exported story');
  // the story still plays the same without a window, and reloads the same
  const run = spawnSync(exe, ['--story', main, '--autoplay', '0,0,1,0,0,2,3'], { encoding: 'utf8' });
  assert(run.stdout.includes("ending: The Admiral's Peace"), 'the story plays exactly as before: ' + run.stdout.trim().split('\n').pop());
  await p.reload({ waitUntil: 'commit' }); await p.waitForSelector('.node');
  assert(await p.evaluate(() => doc.locales.vista === '3d' && doc.render === ''), 'and reloads the same');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
