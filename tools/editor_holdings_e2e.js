// End-to-end test of what characters have (attributes, items, information)
// and of giving, copying and trading it: set up in the editor in a real
// browser, then played by the game.
//
//   cargo build && node tools/editor_holdings_e2e.js [screenshot-dir]
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-hold-'));
const out = process.argv[2] || tmp;
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

const STORY = `title Pockets
character m | Marlowe | #96e6a0 | -
character q | Quince | #ffd27f | q
character c | Cook | #ffb070 | c

*start
bg pantry
show q neutral left
n: The pantry.
m: Good morning.
end Done
`;

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets'));
  const main = path.join(tmp, 'assets', 'story.txt');
  fs.writeFileSync(main, STORY);
  const PORT = 7946;
  procs.push(spawn(exe, ['--editor', '--story', main, '--stories', path.join(tmp, 'stories'), '--comfy-dir', path.join(root, 'comfyui'), '--port', String(PORT), '--no-browser'], { stdio: 'ignore' }));
  await sleep(800);
  const b = browser = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
  p.setDefaultTimeout(15000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('dialog', d => d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  const save = async () => { await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty); return fs.readFileSync(main, 'utf8'); };
  const play = () => { const r = spawnSync(exe, ['--story', main, '--autoplay', '0'], { encoding: 'utf8' }); return (r.stdout + r.stderr).trim().split('\n'); };
  const errors = () => p.evaluate(() => fetch('/api/validate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(doc) }).then(r => r.json()).then(r => r.problems.filter(x => x.level === 'error').map(x => x.msg)));
  const addBlock = async label => { await p.locator('#insp button', { hasText: new RegExp('^\\+ ' + label + '$') }).click(); };

  // ---- nothing to exchange yet
  await p.click('.node');
  await addBlock('Give');
  assert((await p.textContent('#toast')).includes('something to have') && await p.evaluate(() => !doc.scenes[0].items.some(i => i.t === 'give')), 'a Give block needs something to give first, and says so');

  // ---- define an item, an attribute and a piece of information
  await p.click('.tabs button[data-tab="props"]');
  for (const [kind, label, id] of [['item', 'Coins', 'coin'], ['item', 'Cellar key', 'key'], ['attribute', 'Nerve', 'nerve'], ['info', 'The fleet is canvas', 'canvas']]) {
    await p.click(`[data-add-thing="${kind}"]`);
    const card = p.locator('#things .card[data-thing]').last();
    await card.locator('input[type=text]').first().fill(label); await card.locator('input[type=text]').first().blur();
    await card.locator('.grid2 input[type=text]').first().fill(id); await card.locator('.grid2 input[type=text]').first().blur();
    await p.waitForSelector(`#things .card[data-thing="${id}"]`);
  }
  assert(await p.evaluate(() => doc.things.map(t => t.kind + ':' + t.id).join() === 'item:coin,item:key,attribute:nerve,info:canvas'), 'attributes, items and information are defined under Properties');
  // who starts with what, in the table
  const hold = id => p.locator(`#holdTable [data-hold="${id}"]`);
  await hold('m.coin').fill('5'); await hold('m.coin').blur();
  await hold('q.key').fill('1'); await hold('q.key').blur();
  await hold('q.nerve').fill('3'); await hold('q.nerve').blur();
  await hold('q.canvas').check();
  await hold('c.coin').fill('-4'); await hold('c.coin').blur();
  assert(await p.evaluate(() => doc.characters[0].holds.coin === 5 && doc.characters[1].holds.key === 1 && doc.characters[1].holds.canvas === 1 && doc.characters[2].holds.coin === 0), 'the table sets who starts with what (and nobody carries fewer than none)');
  let text = await save();
  assert(text.includes('item coin | Coins | 0') && text.includes('attribute nerve | Nerve | 0') && text.includes('info canvas | The fleet is canvas | 0') && text.includes('holds m | coin 5') && text.includes('holds q | canvas 1 | key 1 | nerve 3') && text.includes('holds c | coin 0'), 'all of it is written to the story file');
  if (out !== tmp) { fs.mkdirSync(out, { recursive: true }); await p.screenshot({ path: path.join(out, 'editor_holdings.png') }); }
  // and on the character's own card
  await p.click('.tabs button[data-tab="cast"]');
  await p.locator('#leftBody .card .head').nth(1).click();
  assert(await p.locator('#leftBody [data-hold="q.key"]').inputValue() === '1' && await p.locator('#leftBody [data-hold="q.canvas"]').isChecked(), "a character's card shows what they start with");

  // ---- exchange it in a scene
  await p.evaluate(() => select(sceneIndex('start'), 3, true));
  await addBlock('Trade');
  await p.waitForFunction(() => doc.scenes[0].items.some(i => i.t === 'trade'));
  if (process.env.DBG) console.log(await p.evaluate(() => [...document.querySelectorAll('#insp .blk')].map(b => b.className).join(' | ')), errs);
  let blk = p.locator('#insp .blk.trade');
  await blk.locator('select').nth(0).selectOption('m'); await blk.locator('input[type=number]').nth(0).fill('3'); await blk.locator('input[type=number]').nth(0).blur();
  blk = p.locator('#insp .blk.trade');
  await blk.locator('select').nth(1).selectOption('coin');
  await p.locator('#insp .blk.trade select').nth(2).selectOption('q');
  await p.locator('#insp .blk.trade select').nth(3).selectOption('key');
  assert(await p.evaluate(() => { const t = doc.scenes[0].items.find(i => i.t === 'trade'); return t.a === 'm' && t.b === 'q' && t.thing === 'coin' && t.n === 3 && t.thing2 === 'key' && t.n2 === 1; }), 'a Trade block: who offers what');
  await p.evaluate(() => select(sceneIndex('start'), doc.scenes[0].items.findIndex(i => i.t === 'trade'), true));
  await addBlock('Copy / tell');
  await p.waitForFunction(() => doc.scenes[0].items.some(i => i.t === 'copy'));
  await p.locator('#insp .blk.copy select').nth(0).selectOption('q');
  await p.locator('#insp .blk.copy select').nth(1).selectOption('m');
  assert(await p.evaluate(() => { const t = doc.scenes[0].items.find(i => i.t === 'copy'); return t.from === 'q' && t.to === 'm' && t.thing === 'canvas'; }) && (await p.locator('#insp .blk.copy').textContent()).includes('tells'), 'a Copy block defaults to telling a piece of information');
  await p.evaluate(() => select(sceneIndex('start'), doc.scenes[0].items.findIndex(i => i.t === 'copy'), true));
  await addBlock('Give');
  await p.waitForFunction(() => doc.scenes[0].items.some(i => i.t === 'give'));
  await p.locator('#insp .blk.give select').nth(0).selectOption('m');
  await p.locator('#insp .blk.give select').nth(1).selectOption('c');
  await p.locator('#insp .blk.give input[type=number]').fill('9'); await p.locator('#insp .blk.give input[type=number]').blur();
  await p.evaluate(() => select(sceneIndex('start'), doc.scenes[0].items.findIndex(i => i.t === 'give'), true));
  await addBlock('Gain or lose');
  await p.waitForFunction(() => doc.scenes[0].items.some(i => i.t === 'grant'));
  await p.locator('#insp .blk.grant select').nth(0).selectOption('q');
  await p.locator('#insp .blk.grant select').nth(1).selectOption('-1');
  await p.locator('#insp .blk.grant select').nth(2).selectOption('nerve');
  assert(await p.evaluate(() => { const g = doc.scenes[0].items.find(i => i.t === 'grant'); return g.who === 'q' && g.thing === 'nerve' && g.n === -1; }), 'a Gain or lose block changes what one character has');
  assert((await p.locator('.node .chip').allTextContents()).some(t => t.includes('Coins') && t.includes('Cellar key')), 'exchanges show on the scene');
  text = await save();
  assert(/^trade m q coin 3 for key 1$/m.test(text) && /^copy q m canvas 1$/m.test(text) && /^give m c coin 9$/m.test(text) && /^grant q nerve -1$/m.test(text), 'the blocks are written as trade, copy, give and grant lines');
  assert((await errors()).length === 0, 'and the story has no problems');

  // ---- the game carries them out
  let lines = play();
  const ex = lines.find(l => l.startsWith('exchanges:')) || '';
  assert(ex === 'exchanges: m coin -3; q coin +3; q key -1; m key +1; m canvas +1; m coin -2; c coin +2; q nerve -1', 'the game trades, tells, gives no more than there is, and takes away: ' + ex);

  // ---- a condition on what someone has
  await p.evaluate(() => change(() => { const items = doc.scenes[0].items; items.splice(items.length - 1, 0, { t: 'if', var: 'm.key', op: '>=', val: 1, target: 'start' }); }));
  await p.evaluate(() => select(sceneIndex('start'), doc.scenes[0].items.findIndex(i => i.t === 'if'), true));
  const cond = p.locator('#insp .blk.on select');
  assert((await cond.nth(0).inputValue()) === 'm' && (await cond.nth(1).inputValue()) === 'key', 'a condition on what someone has is shown as who and what');
  await cond.nth(1).selectOption('canvas');
  await p.waitForFunction(() => doc.scenes[0].items.find(i => i.t === 'if').var === 'm.canvas');
  assert((await p.locator('#insp .blk.on select').nth(2).locator('option').allTextContents()).includes('knows it'), 'for information it asks whether they know it');
  await p.keyboard.press('Control+z'); await p.keyboard.press('Control+z');
  // a fresh condition can be turned into one
  await addBlock('Condition');
  await p.waitForFunction(() => doc.scenes[0].items.some(i => i.t === 'if' && i.var === 'event.alarm'));
  await p.evaluate(() => select(sceneIndex('start'), doc.scenes[0].items.findIndex(i => i.t === 'if'), true));
  await p.locator('#insp .blk.on select').first().selectOption({ label: '🎒 what someone has…' });
  await sleep(200);
  assert(await p.evaluate(() => { const c = doc.scenes[0].items.find(i => i.t === 'if'); return c.var === 'm.coin' && c.op === '>=' && c.val === 1; }), "a condition can be turned into one on what someone has: " + await p.evaluate(() => doc.scenes[0].items.find(i => i.t === 'if').var));
  await p.evaluate(() => { while (undoStack.length && doc.scenes[0].items.some(i => i.t === 'if')) undo(); });

  // ---- renaming and deleting
  await p.click('.tabs button[data-tab="props"]');
  const idField = p.locator('#things .card[data-thing="coin"] .grid2 input[type=text]').first();
  await idField.fill('gold'); await idField.blur();
  await p.waitForSelector('#things .card[data-thing="gold"]');
  assert(await p.evaluate(() => doc.characters[0].holds.gold === 5 && !('coin' in doc.characters[0].holds) && doc.scenes[0].items.find(i => i.t === 'trade').thing === 'gold' && doc.scenes[0].items.find(i => i.t === 'give').thing === 'gold'), 'renaming a thing follows it into what people hold and every exchange');
  await p.locator('#things .card[data-thing="gold"] button.danger').click();
  assert(await p.evaluate(() => doc.things.some(t => t.id === 'gold')) && (await p.textContent('#toast')).includes('still used'), 'a thing in use cannot be deleted');
  await p.evaluate(() => change(() => { renameEverywhere('char', 'q', 'quince'); doc.characters[1].id = 'quince'; }));
  assert(await p.evaluate(() => { const t = doc.scenes[0].items.find(i => i.t === 'trade'), c = doc.scenes[0].items.find(i => i.t === 'copy'); return t.b === 'quince' && c.from === 'quince'; }), 'renaming a character follows them into every exchange');
  await p.keyboard.press('Escape');
  await p.locator('#stage').click({ position: { x: 5, y: 5 } });
  await p.keyboard.press('Control+z');
  // hidden things are not announced
  await p.evaluate(() => change(() => { doc.things.find(t => t.id === 'canvas').hidden = true; }));
  text = await save();
  assert(text.includes('info canvas | The fleet is canvas | 0 | hidden'), 'a thing can be kept from the player');
  // the picture: the game's own ledger
  const shots = path.join(tmp, 'shots');
  spawnSync(exe, ['--story', main, '--autoplay', '0', '--ledger', '--shots', shots], { encoding: 'utf8' });
  assert(fs.existsSync(path.join(shots, '001_say.png')), 'the game shows who has what');
  // export carries it; reload restores it
  const exported = await p.evaluate(() => fetch('/api/export?format=json&story=main').then(r => r.json()));
  assert(exported.story.things.length === 4 && exported.story.characters[0].holds.gold === 5, 'what characters have travels with an exported story');
  await p.reload({ waitUntil: 'commit' }); await p.waitForSelector('.node');
  assert(await p.evaluate(() => doc.things.length === 4 && doc.characters[1].holds.nerve === 3), 'and reloads as saved');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
