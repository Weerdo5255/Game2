// End-to-end test of chapters in the story editor (panels on the main
// graph, opening a chapter, what it passes out), in a real browser.
//
//   cargo build && node tools/editor_chapters_e2e.js [screenshot-dir]
//
// Needs Node with Playwright and Chromium. Works on a temporary copy.
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-chapters-'));
const story = path.join(tmp, 'story.txt');
fs.copyFileSync(path.join(root, 'assets', 'story.txt'), story);
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const PORT = 7937;
const server = spawn(exe, ['--editor', '--story', story, '--stories', path.join(tmp, 'stories'), '--port', String(PORT), '--no-browser', '--procedural'], { stdio: 'ignore' });
const out = process.argv[2] || tmp;
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
let browser = null;
(async () => {
  await new Promise(r => setTimeout(r, 800));
  const b = browser = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1600, height: 950 } });
  p.setDefaultTimeout(8000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('dialog', d => d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  await p.waitForTimeout(600);
  const D = () => p.evaluate(() => doc);
  const idx = l => p.evaluate(l => sceneIndex(l), l);
  const panel = name => `.node.chapter[data-c^="${name}"]`;
  const centre = async sel => { const r = await p.locator(sel).first().boundingBox(); return [r.x + r.width / 2, r.y + r.height / 2]; };
  const dragTo = async (from, to) => { const a = await centre(from), z = await centre(to); await p.mouse.move(a[0], a[1]); await p.mouse.down(); await p.mouse.move(z[0], z[1], { steps: 8 }); await p.mouse.up(); };

  // ---- the main graph
  assert(await p.locator('.node.chapter').count() === 8 && await p.locator('.node:not(.chapter)').count() === 0, 'the main graph shows eight chapter panels and no loose scenes');
  const three = await p.textContent(panel('Three:'));
  assert(three.includes('7 scenes') && three.includes("Quince's trust −1…+2") && three.includes('Detente 0…+1'), 'a panel shows what the chapter passes out, as a range over every route: ' + three.match(/passes out(.*?)depends on/)[1]);
  assert(/depends on.*Quince's trust/.test(three), 'and the properties it depends on');
  assert(await p.locator(`${panel('Three:')} .in`).count() === 1 && await p.locator(`${panel('Three:')} .out`).count() === 2, 'with its ways in and its ways out');
  assert((await p.locator('#edges path').count()) >= 8, 'chapters are wired together on the main graph');
  await p.screenshot({ path: path.join(out, 'chapters.png') });

  // selecting a chapter shows its summary
  await p.click(`${panel('Three:')} .nh .lbl`);
  assert((await p.textContent('#insp')).includes('Passes out to the story') && await p.locator('#insp .wayout').count() === 2, 'selecting a chapter lists what it passes out and its ways out');

  // ---- open a chapter
  await p.dblclick(`${panel('Three:')} .nb`);
  await p.waitForFunction(() => curChapter.startsWith('Three:'));
  const shown = await p.locator('.node:not(.portal)').count();
  assert(shown === 7, 'opening a chapter shows only its scenes (' + shown + ')');
  assert(await p.locator('.node.portal').count() === 4, 'plus portals for where the story comes in from and goes out to');
  assert((await p.textContent('#crumbs')).includes('Three: Below Stairs'), 'the breadcrumb says where you are');
  await p.screenshot({ path: path.join(out, 'chapter-open.png') });

  // change a consequence inside; the panel outside follows
  await p.click(`.node[data-i="${await idx('share_nothing')}"] .nh`);
  await p.fill('#insp .blk.add input[type=number]', '-3');
  await p.dispatchEvent('#insp .blk.add input[type=number]', 'change');
  await p.click('#crumbs button >> nth=0');
  await p.waitForFunction(() => curChapter === '');
  assert((await p.textContent(panel('Three:'))).includes("Quince's trust −3…+2"), 'editing a consequence inside changes what the chapter passes out');

  // a portal leads to the scene it stands for
  await p.dblclick(`${panel('Three:')} .nb`);
  await p.waitForFunction(() => curChapter.startsWith('Three:'));
  await p.click('.node.portal:has-text("ch4") .nh');
  await p.waitForFunction(() => curChapter.startsWith('Four:'));
  assert(await p.inputValue('#insp .ihead input') === 'ch4', 'clicking a portal goes to that scene in its own chapter');

  // ---- moving scenes between chapters
  await p.locator('#insp .chapterpick select').selectOption({ label: 'Three: Below Stairs' });
  let d = await D();
  assert(d.scenes[await idx('ch4')].chapter === 'Three: Below Stairs' && await p.evaluate(() => curChapter.startsWith('Three:')), 'a scene can be moved to another chapter from its panel');
  await p.click('#undoBtn');
  assert((await D()).scenes[await idx('ch4')].chapter.startsWith('Four:'), 'undo puts it back');
  // drag a scene from the Scenes list on to a chapter panel
  await p.click('#crumbs button >> nth=0');
  await p.click('.tabs button[data-tab="scenes"]');
  await p.dragAndDrop('#leftBody .srow.scene:has-text("caught_saved")', `${panel('Six:')} .nb`);
  assert((await D()).scenes[await idx('caught_saved')].chapter.startsWith('Six:'), 'dragging a scene on to a chapter panel moves it there');
  await p.click('#undoBtn');

  // ---- rerouting between chapters on the main graph
  const exitPort = `${panel('Two:')} .out >> nth=0 >> .port`;
  await dragTo(exitPort, `${panel('Five:')} .in`);
  d = await D();
  const rerouted = d.scenes.filter(s => s.chapter.startsWith('Two:')).flatMap(s => s.items).filter(i => i.t === 'goto' && i.target === 'ch5').length;
  assert(rerouted === 1, "dragging a chapter's way out on to another chapter's way in reroutes the story");
  await p.click('#undoBtn');

  // ---- making, renaming and dissolving chapters
  const before = (await D()).chapters.length;
  await p.click('#crumbs button:has-text("+ Chapter")');
  d = await D();
  assert(d.chapters.length === before + 1 && await p.locator('#insp .ihead input').inputValue() === d.chapters[before].name, 'a new, empty chapter can be added');
  const nameBox = p.locator('#insp .ihead input');
  await nameBox.fill('Epilogue'); await nameBox.dispatchEvent('change');
  await p.click('#insp button:has-text("+ Scene in this chapter")');
  d = await D();
  assert(d.scenes.some(s => s.chapter === 'Epilogue') && await p.evaluate(() => curChapter) === 'Epilogue', 'scenes can be created inside it');
  await p.click('#crumbs button >> nth=0');
  await p.click(`${panel('Epilogue')} .nh .lbl`);
  await p.click('#insp .ihead button.danger');
  d = await D();
  assert(!d.chapters.some(c => c.name === 'Epilogue') && d.scenes.some(s => s.label.startsWith('scene') && !s.chapter), 'dissolving a chapter returns its scenes to the main graph and deletes nothing');
  assert(await p.locator('.node:not(.chapter)').count() === 1, 'a scene outside any chapter sits on the main graph beside the panels');
  await p.click('#undoBtn'); await p.click('#undoBtn'); await p.click('#undoBtn'); await p.click('#undoBtn');

  // ---- a story without chapters can be grouped from its chapter cards
  await p.evaluate(() => change(() => { doc.scenes.forEach(s => { s.chapter = ''; s.pos = null; }); doc.chapters = []; curChapter = ''; selChapter = null; }));
  const loose = await p.locator('.node').count();
  assert(await p.locator('.node.chapter').count() === 0 && loose === 47, 'without chapters every scene is on the main graph (' + loose + ')');
  await p.click('#crumbs button:has-text("Chapters from chapter cards")');
  d = await D();
  assert(d.chapters.length === 8 && d.scenes.every(s => s.chapter) && d.scenes[await idx('soup_cube')].chapter.startsWith('Chapter One'), 'chapters can be made from the chapter cards: ' + d.chapters.map(c => c.name.split(':')[0]).join(', '));
  await p.click('#undoBtn'); await p.click('#undoBtn');
  await p.waitForFunction(() => doc.chapters.length === 8);

  // ---- saving
  await p.click(`${panel('Three:')} .nh .lbl`);
  await p.locator('#insp textarea').first().fill('The servants compare notes.');
  await p.keyboard.press('Control+s');
  await p.waitForFunction(() => !dirty);
  const text = fs.readFileSync(story, 'utf8');
  assert(/^chapter Three: Below Stairs \| -?\d+ -?\d+ \| The servants compare notes\.$/m.test(text) && text.includes('*share_nothing\n# @chapter Three: Below Stairs\n'), 'chapters, their positions and notes are saved in the story file');
  const run = spawnSync(exe, ['--procedural', '--story', story, '--autoplay', '0,0,1,0,0,2,3'], { encoding: 'utf8' });
  assert(run.status === 0 && run.stdout.includes("ending: The Admiral's Peace"), 'the game plays the chaptered story exactly as before: ' + run.stdout.trim());
  await p.reload(); await p.waitForSelector('.node.chapter');
  assert(await p.locator('.node.chapter').count() === 8 && (await p.textContent(panel('Three:'))).includes('The servants compare notes.'), 'and it reloads the same');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); server.kill(); fs.rmSync(tmp, { recursive: true, force: true }); });
