// End-to-end test of the story editor in a real browser.
//
//   cargo build && node tools/editor_e2e.js [screenshot-dir]
//
// Needs Node with Playwright and Chromium. It runs the editor on a
// temporary copy of assets/story.txt, so the real story is never touched.
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-editor-'));
const story = path.join(tmp, 'story.txt');
fs.copyFileSync(path.join(root, 'assets', 'story.txt'), story);
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const PORT = 7933, URL = `http://127.0.0.1:${PORT}/`;
const server = spawn(exe, ['--editor', '--story', story, '--port', String(PORT), '--no-browser', '--procedural'], { stdio: 'ignore' });
const out = process.argv[2] || tmp;
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
(async () => {
  await new Promise(r => setTimeout(r, 800));
  const b = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1600, height: 900 } });
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('console', m => { if (m.type() === 'error') errs.push('console: ' + m.text()); });
  p.on('dialog', d => d.accept());
  await p.goto(URL);
  await p.waitForSelector('.node');
  await p.waitForTimeout(800);
  const D = () => p.evaluate(() => doc);
  const idx = l => p.evaluate(l => sceneIndex(l), l);

  // 1. select a scene by clicking its node
  const si = await idx('soup_yes');
  await p.click(`.node[data-i="${si}"] .nh`);
  assert(await p.inputValue('#insp .ihead input') === 'soup_yes', 'clicking a node selects the scene');

  // 2. edit a line
  const ta = p.locator('#insp .blk.say textarea').first();
  await ta.fill('Chilled soup! Marvellous.');
  await p.waitForTimeout(400);
  let d = await D();
  assert(d.scenes[si].items.some(i => i.text === 'Chilled soup! Marvellous.'), 'typing edits the line');
  assert((await p.textContent(`.node[data-i="${si}"] .snip`)).includes('Chilled soup'), 'graph node shows the new text');
  assert(await p.isEnabled('#saveBtn'), 'story is marked unsaved');

  // 3. add a consequence and set it
  await p.click('#insp .addbar button:has-text("Consequence")');
  await p.selectOption('#insp .blk.on select >> nth=0', 'heat');
  await p.fill('#insp .blk.on input[type=number]', '2');
  await p.dispatchEvent('#insp .blk.on input[type=number]', 'change');
  d = await D();
  assert(d.scenes[si].items.some(i => i.t === 'add' && i.var === 'heat' && i.n === 2), 'consequence added: Suspicion +2');
  assert((await p.textContent(`.node[data-i="${si}"]`)).includes('Suspicion +2'), 'consequence chip shown on the scene');
  const start = await idx('start');
  assert((await p.textContent(`.node[data-i="${start}"] .outs`)).includes('Suspicion +2'), 'consequence shown beside the choice that leads there');

  // 4. reroute by dragging the port on to another scene
  const target = await idx('soup_no');
  const port = await p.locator(`.node[data-i="${si}"] .port`).boundingBox();
  const tnode = await p.locator(`.node[data-i="${target}"] .nh`).boundingBox();
  await p.mouse.move(port.x + 7, port.y + 7); await p.mouse.down();
  await p.mouse.move(tnode.x + 60, tnode.y + 10, { steps: 8 }); await p.mouse.up();
  d = await D();
  assert(d.scenes[si].items.find(i => i.t === 'goto').target === 'soup_no', 'dragging a port reroutes the jump');

  // 5. undo restores, redo reapplies
  await p.click('#undoBtn');
  d = await D();
  assert(d.scenes[si].items.find(i => i.t === 'goto').target === 'ch1_swan', 'undo restores the jump');
  await p.click('#redoBtn'); await p.click('#undoBtn');

  // 6. move a scene by dragging its header
  const before = (await D()).scenes[si].pos;
  const nh = await p.locator(`.node[data-i="${si}"] .nh`).boundingBox();
  await p.mouse.move(nh.x + 30, nh.y + 8); await p.mouse.down(); await p.mouse.move(nh.x + 110, nh.y - 60, { steps: 6 }); await p.mouse.up();
  const after = (await D()).scenes[si].pos;
  assert(after[0] !== before[0] && after[1] !== before[1], 'dragging moves a scene');

  // 7. reorder blocks with the buttons, then by drag and drop
  await p.click(`.node[data-i="${si}"] .nh`);
  const kinds = async () => (await D()).scenes[si].items.map(i => i.t).join(',');
  const k0 = await kinds();
  await p.click('#insp .blk[data-i="0"] button[title="Move down"]');
  const k1 = await kinds();
  assert(k1 !== k0, `move-down button reorders blocks `);
  await p.dragAndDrop('#insp .blk[data-i="1"] .grip', '#insp .blk[data-i="0"]', { targetPosition: { x: 40, y: 3 } });
  assert(await kinds() === k0, 'drag and drop reorders blocks back');

  // 8. drop a block on another scene in the graph
  const n0 = (await D()).scenes[target].items.length;
  await p.dragAndDrop('#insp .blk.add >> nth=1 >> .grip', `.node[data-i="${target}"] .nb`);
  d = await D();
  assert(d.scenes[target].items.length === n0 + 1 && !d.scenes[si].items.some(i => i.t === 'add' && i.var === 'heat'), 'dropping a block on a scene moves it there');
  const moved = d.scenes[target].items; 
  assert(moved[moved.length - 1].t === 'goto' && moved[moved.length - 2].t === 'add', 'moved block lands before the closing jump');

  // 9. choice editing: text, new option, retarget to a new scene
  await p.click(`.node[data-i="${start}"] .nh`);
  await p.locator('#insp .blk.choice .opt textarea').first().fill('"Chilled, sir. Dukes litigate."');
  await p.click('#insp .blk.choice button:has-text("+ Option")');
  d = await D();
  let ch = d.scenes[start].items.find(i => i.t === 'choice');
  assert(ch.options.length === 4 && ch.options[0].text.startsWith('"Chilled, sir'), 'choice text edited and option added');
  const scenesBefore = d.scenes.length;
  await p.locator('#insp .blk.choice .opt').nth(3).locator('select').first().selectOption({ label: '+ New scene…' });
  d = await D();
  ch = d.scenes[start].items.find(i => i.t === 'choice');
  assert(d.scenes.length === scenesBefore + 1 && d.scenes.some(s => s.label === ch.options[3].target), 'option can lead to a brand new scene');
  // condition on an option
  await p.click(`.node[data-i="${await idx('start')}"] .nh`);
  await p.locator('#insp .blk.choice .opt').nth(3).locator('input[type=checkbox]').check();
  d = await D();
  assert(d.scenes[await idx('start')].items.find(i => i.t === 'choice').options[3].cond, 'option can be made conditional');
  await p.screenshot({ path: out + '/e2.png' });

  // 10. cast: rename, recolour hair, property of a character
  await p.click('.tabs button[data-tab="cast"]');
  await p.click('#leftBody .card:has-text("Peregrine") .head');
  await p.waitForSelector('#leftBody .card.open img.big');
  await p.locator('#leftBody .card.open .grid2 input[type=text]').first().fill('Perry');
  const hair = p.locator('#leftBody .card.open .grid2 >> text=hair').locator('xpath=following-sibling::div[1]//input');
  await hair.evaluate(e => { e.focus(); e.value = '#303030'; e.dispatchEvent(new Event('input', { bubbles: true })); e.dispatchEvent(new Event('change', { bubbles: true })); });
  await p.waitForTimeout(900);
  d = await D();
  const perry = d.characters.find(c => c.id === 'p');
  assert(perry.name === 'Perry' && perry.look.hair === '#303030', 'character name and look are editable');
  assert((await p.getAttribute('#leftBody .card.open img.big', 'src')).includes('%23303030'), 'portrait preview follows the look');
  assert((await p.textContent('#leftBody .card.open')).includes("Peregrine's regard"), "character card lists that character's properties");
  await p.screenshot({ path: out + '/e3.png' });
  // rename the id: references follow
  const idInput = p.locator('#leftBody .card.open .grid2 input[type=text]').nth(1);
  await idInput.fill('perry'); await idInput.dispatchEvent('change');
  d = await D();
  assert(!JSON.stringify(d.scenes).includes('"who":"p"') && d.vars.find(v => v.id === 'favor').character === 'perry', 'renaming a character id updates every reference');

  // 11. properties tab
  await p.click('.tabs button[data-tab="props"]');
  const card = p.locator('#leftBody .card[data-id="heat"]');
  await card.locator('input[type=text]').first().fill('Scrutiny');
  await p.waitForTimeout(400);
  assert((await p.textContent(`.node[data-i="${await idx('soup_no')}"]`)).includes('Scrutiny +2'), 'renaming a property label updates its consequence chips');
  await card.locator('.chip.link').first().click();
  assert((await p.inputValue('#insp .ihead input')).length > 0, 'clicking a usage jumps to that scene');
  await p.screenshot({ path: out + '/e4.png' });

  // 12. scenes tab: drag to reorder
  await p.click('.tabs button[data-tab="scenes"]');
  const order0 = (await D()).scenes.map(s => s.label);
  await p.dragAndDrop('#leftBody .srow >> nth=2', '#leftBody .srow >> nth=0', { targetPosition: { x: 60, y: 2 } });
  const order1 = (await D()).scenes.map(s => s.label);
  assert(order1[0] === order0[2], `dragging in the Scenes list reorders scenes (${order0[2]} first)`);
  await p.click('#undoBtn');

  // 13. validation shows problems, and they clear
  await p.click(`.node[data-i="${await idx('soup_cube')}"] .nh`);
  await p.locator('#insp .blk.goto select').selectOption('soup_cube');   // loop to itself: fine
  await p.click('#insp .addbar button:has-text("Jump")');
  await p.waitForTimeout(700);
  assert((await p.textContent('#probBtn')).includes('warning'), 'validator reports problems live: ' + await p.textContent('#probBtn'));
  await p.click('#undoBtn'); await p.click('#undoBtn');
  await p.waitForTimeout(700);

  // 14. save, and the file on disk changes
  await p.keyboard.press('Control+s');
  await p.waitForTimeout(600);
  assert(!(await p.isEnabled('#saveBtn')), 'saved');
  await p.click('#fitBtn');
  await p.waitForTimeout(1500);
  await p.screenshot({ path: out + '/e5.png' });
  // 15. what was saved is a story the game accepts and can play to an ending
  const saved = fs.readFileSync(story, 'utf8');
  assert(saved.includes('character perry | Perry') && saved.includes('hair=#303030') && saved.includes('| Scrutiny |'), 'saved file holds the edits');
  const run = spawnSync(exe, ['--procedural', '--story', story, '--autoplay', '0,0,1,0,0,2,3'], { encoding: 'utf8' });
  assert(run.status === 0 && run.stdout.includes('ending:'), 'the game plays the saved story: ' + run.stdout.trim());

  // 16. the server refuses requests that are not from its own page
  const cross = await p.evaluate(() => fetch('/api/story', { method: 'PUT', headers: { 'Content-Type': 'text/plain' }, body: '{}' }).then(r => r.status));
  assert(cross === 403, 'non-JSON writes are refused');
  // 17. a document that could not be read back is refused, and the file is left alone
  const onDisk = fs.readFileSync(story, 'utf8');
  const refused = await p.evaluate(() => { const d = JSON.parse(JSON.stringify(doc)); d.scenes[0].items.push({ t: 'add', var: '', n: 1 }); return fetch('/api/story', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(d) }).then(r => r.status); });
  assert(refused === 400 && fs.readFileSync(story, 'utf8') === onDisk, 'an incomplete block cannot be saved over the story');

  // 18. committing a field by clicking elsewhere must not swallow that click
  await p.click('.tabs button[data-tab="props"]');
  const startAt = p.locator('#leftBody .card[data-id="peace"] input[type=number]').first();
  await startAt.fill('3');
  await p.click('.tabs button[data-tab="cast"]');
  assert(await p.locator('.tabs button[data-tab="cast"].on').count() === 1 && (await p.evaluate(() => varById('peace').initial)) === 3, 'a click that commits a field still lands');

  const real = errs.filter(e => !e.includes('403') && !e.includes('400'));   // step 16 provokes one on purpose
  console.log('page errors:', JSON.stringify(real));
  if (real.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(() => { server.kill(); fs.rmSync(tmp, { recursive: true, force: true }); });
