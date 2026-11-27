// End-to-end test of the Manage view (library, snapshots, locking, removed
// stories), find and replace, and draft recovery, in a real browser.
//
//   cargo build && node tools/editor_manage_e2e.js [screenshot-dir]
//
// Everything happens in a temporary folder; the repository is not touched.
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-manage-'));
const out = process.argv[2] || tmp;
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets', 'enhanced', '.removed'), { recursive: true });
  const main = path.join(tmp, 'assets', 'story.txt');
  fs.copyFileSync(path.join(root, 'assets', 'story.txt'), main);
  fs.writeFileSync(path.join(tmp, 'assets', 'enhanced', '.removed', 'bg_old.png'), Buffer.alloc(300000));
  const PORT = 7940, stories = path.join(tmp, 'stories');
  procs.push(spawn(exe, ['--editor', '--story', main, '--stories', stories, '--comfy-dir', path.join(root, 'comfyui'), '--port', String(PORT), '--no-browser'], { stdio: 'ignore' }));
  await sleep(800);
  const b = browser = await chromium.launch();
  const ctx = await b.newContext({ viewport: { width: 1600, height: 950 } });
  const p = await ctx.newPage();
  p.setDefaultTimeout(15000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  let answer = '', accept = true;
  const asked = [];
  p.on('dialog', d => { if (d.type() === 'beforeunload') return d.accept(); asked.push(d.message()); if (!accept) return d.dismiss(); return d.type() === 'prompt' ? d.accept(answer) : d.accept(); });
  const post = (url, body) => p.evaluate(([url, body]) => fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).then(async r => [r.status, await r.json()]), [url, body]);
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');

  // ---- find and replace
  await p.keyboard.press('Control+f');
  await p.fill('#findQ', 'swan');
  const count = await p.textContent('#findCount');
  assert(/^\d\d+ matches in \d+ places/.test(count) && await p.locator('#find .hit mark').count() >= 10, `find lists every match in the story's text: ${count}`);
  await p.locator('#find .hit').nth(1).click();
  assert(await p.evaluate(() => sel >= 0 && selItem >= 0 && /swan/i.test(JSON.stringify(doc.scenes[sel].items[selItem]))), 'clicking a result opens that line');
  await p.fill('#findTo', 'goose');
  await p.click('#replaceAll');
  assert(await p.evaluate(() => !/swan/i.test(doc.scenes.map(s => s.items.filter(i => i.t === 'say').map(i => i.text).join(' ')).join(' ')) && doc.scenes.some(s => s.label === 'ch1_swan') && dirty), 'replace all changes the text of the story, and only the text');
  await p.keyboard.press('Escape');
  await p.locator('#stage').click({ position: { x: 5, y: 5 } });
  await p.keyboard.press('Control+z');
  assert(await p.evaluate(() => /[Ss]wan /.test(doc.scenes.map(s => s.items.filter(i => i.t === 'say').map(i => i.text).join(' ')).join(' '))), 'and is one step to undo');
  await p.keyboard.press('Control+f');
  await p.fill('#findQ', 'a.b(c'); 
  assert(await p.textContent('#findCount') === 'No matches', 'what is typed is looked for as it is, not as a pattern');
  await p.keyboard.press('Escape');
  assert(!(await p.locator('#find').isVisible()), 'Esc closes it');
  await p.keyboard.press('Control+z'); await p.keyboard.press('Control+Shift+z'); await p.keyboard.press('Control+z');

  // ---- drafts
  await p.evaluate(() => change(() => { doc.subtitle = 'A draft never saved'; }));
  await sleep(1600);
  await p.reload({ waitUntil: 'commit' }); await p.waitForSelector('.node');
  await p.waitForFunction(() => doc.subtitle === 'A draft never saved');
  assert(asked.some(m => /Unsaved changes to this story/.test(m)) && await p.evaluate(() => dirty), 'unsaved work is offered back after the page went away');
  await p.keyboard.press('Control+z');
  assert(await p.evaluate(() => doc.subtitle !== 'A draft never saved'), 'restoring it can be undone');
  await p.keyboard.press('Control+Shift+z');
  await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty);
  asked.length = 0;
  await p.reload({ waitUntil: 'commit' }); await p.waitForSelector('.node');
  await sleep(300);
  assert(!asked.length && await p.evaluate(() => !dirty && doc.subtitle === 'A draft never saved'), 'once saved there is nothing to offer');
  await p.evaluate(() => change(() => { doc.subtitle = 'Thrown away'; }));
  await sleep(1600);
  accept = false; asked.length = 0;
  await p.reload({ waitUntil: 'commit' }); await p.waitForSelector('.node'); await sleep(300);
  assert(asked.length === 1 && await p.evaluate(() => !dirty && doc.subtitle === 'A draft never saved'), 'a draft can be declined');
  accept = true; asked.length = 0;
  await p.reload({ waitUntil: 'commit' }); await p.waitForSelector('.node'); await sleep(300);
  assert(!asked.length, 'and is then gone');

  // a draft belongs to a story file, and a clean tab leaves another tab's draft alone
  await p.evaluate(() => change(() => { doc.subtitle = 'In the other tab'; }));
  await sleep(1600);
  const p2 = await ctx.newPage(); const asked2 = [];
  p2.on('dialog', d => { asked2.push(d.message()); d.dismiss(); });
  await p2.goto(`http://127.0.0.1:${PORT}/`); await p2.waitForSelector('.node');
  await p.evaluate(() => { doc.subtitle += '.'; writeDraft(); });
  await p2.close({ runBeforeUnload: false });
  const keys = await p.evaluate(() => Object.keys(localStorage).filter(k => k.startsWith('story-editor-draft:')));
  assert(keys.length === 1 && keys[0].endsWith('story.txt') && asked2.length === 1, 'drafts are filed under the story file, and closing a clean tab leaves them: ' + keys.join() + ' asked ' + asked2.length);
  await p.keyboard.press('Control+z'); await p.evaluate(() => { dirty = false; dropDraft(); updateBar(); });

  // ---- Manage: figures
  await p.click('#adminBtn');
  await p.waitForSelector('#admin [data-story="main"]');
  const stats = await p.locator('#admin .stat').allTextContents();
  assert(stats[0] === '47scenes' && /words/.test(stats[2]) && /min/.test(stats[3]), 'the open story has its figures: ' + stats.join(' | '));
  const row = await p.locator('#admin [data-story="main"] td').allTextContents();
  assert(row[1] === '47' && /KB/.test(row[3]) && /KB/.test(row[6]), 'the library lists each story with its size, set-aside files included: ' + row.slice(1, 8).join(' | '));

  // ---- snapshots
  assert(await p.locator('#admin [data-snapshot]').count() === 1, 'saving a change kept the version before it');
  const first = fs.readdirSync(path.join(tmp, 'assets', '.history'));
  assert(first.length === 1 && /^\d+_auto\.txt$/.test(first[0]) && !fs.readFileSync(path.join(tmp, 'assets', '.history', first[0]), 'utf8').includes('A draft never saved'), 'as a file beside the story');
  answer = 'Before the rewrite! été 日本';
  await p.locator('#admin button', { hasText: 'Take a snapshot now' }).click();
  await p.waitForFunction(() => document.querySelectorAll('#admin [data-snapshot]').length === 2);
  assert((await p.locator('#admin [data-snapshot]').first().textContent()).includes('Before the rewrite! été 日本'), 'a snapshot can be taken by hand, with a name kept as typed');
  await p.click('#adminBtn');
  await p.evaluate(() => change(() => { doc.scenes.splice(5); doc.subtitle = 'Ruined'; }));
  await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty);
  assert(fs.readdirSync(path.join(tmp, 'assets', '.history')).filter(f => f.endsWith('.txt')).length === 2, 'automatic snapshots are spaced out, not one per save');
  await p.click('#adminBtn');
  await p.waitForSelector('#admin [data-snapshot]');
  await p.locator('#admin [data-snapshot]').first().locator('button', { hasText: 'Restore' }).click();
  await p.waitForFunction(() => doc.scenes.length === 47);
  assert(await p.evaluate(() => dirty && doc.subtitle === 'A draft never saved' && !admin.open), 'restoring a snapshot shows that version, unsaved');
  await p.keyboard.press('Control+z');
  assert(await p.evaluate(() => doc.scenes.length === 5), 'and can be undone');
  await p.keyboard.press('Control+Shift+z');
  await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty);
  assert((fs.readFileSync(main, 'utf8').match(/^\*/gm) || []).length === 47, 'saving keeps it');
  const kept = fs.readdirSync(path.join(tmp, 'assets', '.history')).filter(f => f.endsWith('.txt'));
  assert(kept.length === 3 && kept.some(f => fs.readFileSync(path.join(tmp, 'assets', '.history', f), 'utf8').includes('Ruined')), 'and the version it replaced is kept too, however recent the last snapshot');
  let [st, r] = await post('/api/history/remove', { story: 'main', name: '../story.txt' });
  assert(st === 404, 'snapshot names cannot point elsewhere');
  st = await p.evaluate(() => fetch('/api/history/get?story=main&name=..%2Fstory.txt').then(r => r.status));
  assert(st === 404, 'for reading either');

  // ---- locking
  await p.click('#adminBtn');
  await p.waitForSelector('#lockBtn');
  await p.click('#lockBtn');
  await p.waitForFunction(() => locked);
  assert(fs.existsSync(main + '.locked') && (await p.textContent('#status')).includes('Locked'), 'a story can be locked');
  await p.click('#adminBtn');
  await p.evaluate(() => change(() => { doc.subtitle = 'Should not be saved'; }));
  await p.keyboard.press('Control+s'); await sleep(400);
  [st, r] = await p.evaluate(() => fetch('/api/story?story=main', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(doc) }).then(async r => [r.status, await r.json()]));
  assert(st === 423 && !fs.readFileSync(main, 'utf8').includes('Should not be saved') && await p.evaluate(() => dirty), 'a locked story is not saved over, by the page or behind its back');
  const blocked = [];
  for (const [url, body] of [['/api/admin/clean', { story: 'main' }], ['/api/art/revert', { story: 'main', assets: ['bg_vista'] }], ['/api/sound/prune', { story: 'main' }], ['/api/art/run', { story: 'main', assets: ['bg_vista'] }], ['/api/history/remove', { story: 'main', name: kept[0] }]]) blocked.push((await post(url, body))[0]);
  const prefsSt = await p.evaluate(() => fetch('/api/art/prefs?story=main', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: '{"prompts":{"bg_vista":"x"}}' }).then(r => r.status));
  assert(blocked.every(c => c === 423) && prefsSt === 423 && fs.existsSync(path.join(tmp, 'assets', 'enhanced', '.removed', 'bg_old.png')) && !fs.existsSync(path.join(tmp, 'assets', 'art.json')), 'nor are its pictures, recordings, settings or history changed: ' + blocked.join(' '));
  await p.keyboard.press('Control+z');
  await p.evaluate(() => { dirty = false; dropDraft(); updateBar(); });
  await p.click('#adminBtn'); await p.waitForSelector('#lockBtn');
  await p.click('#lockBtn');
  await p.waitForFunction(() => !locked);
  assert(!fs.existsSync(main + '.locked'), 'and unlocked again');

  // ---- set-aside files
  await p.locator('#admin [data-story="main"] button', { hasText: 'Free' }).click();
  await p.waitForFunction(() => !document.querySelector('#admin [data-story="main"] button.x') && ![...document.querySelectorAll('#admin [data-story="main"] button')].some(b => b.textContent.startsWith('Free')));
  assert(!fs.existsSync(path.join(tmp, 'assets', 'enhanced', '.removed')) && !fs.existsSync(main + '.bak') && fs.existsSync(main), 'set-aside files can be deleted to free space, and nothing else goes');

  // ---- duplicate, remove, bring back, delete for good
  answer = 'Side Story';
  await p.locator('#admin [data-story="main"] button', { hasText: 'Duplicate' }).click();
  await p.waitForSelector('#admin [data-story="side-story"]');
  assert(await p.evaluate(() => storyId === 'main') && (await p.locator('#storySel option').allTextContents()).includes('Side Story'), 'a story can be duplicated from the list without leaving the open one');
  await post('/api/admin/lock', { story: 'side-story', locked: true });
  [st, r] = await post('/api/stories/remove', { story: 'side-story' });
  assert(st === 423 && fs.existsSync(path.join(stories, 'side-story')), 'a locked story cannot be removed');
  await post('/api/admin/lock', { story: 'side-story', locked: false });
  await p.locator('#admin button', { hasText: 'Refresh' }).click();
  await p.waitForFunction(() => !document.querySelector('#admin [data-story="side-story"] button[disabled]'));
  await p.locator('#admin [data-story="side-story"] button', { hasText: 'Remove' }).click();
  await p.waitForSelector('#admin [data-trash="side-story"]');
  assert(!fs.existsSync(path.join(stories, 'side-story')) && !(await p.locator('#storySel option').allTextContents()).includes('Side Story'), 'removing moves it to Removed stories');
  if (out !== tmp) { fs.mkdirSync(out, { recursive: true }); await p.screenshot({ path: path.join(out, 'editor_manage.png') }); }
  await p.locator('#admin [data-trash="side-story"] button', { hasText: 'Bring back' }).click();
  await p.waitForSelector('#admin [data-story="side-story"]');
  assert(fs.existsSync(path.join(stories, 'side-story', 'story.txt')) && (await p.locator('#storySel option').allTextContents()).includes('Side Story'), 'it can be brought back');
  await p.locator('#admin [data-story="side-story"] button', { hasText: 'Open' }).click();
  await p.waitForFunction(() => storyId === 'side-story' && doc.title === 'Side Story');
  await p.waitForSelector('#admin tr.cur[data-story="side-story"]');
  assert(true, 'and opened from the list');
  await p.locator('#admin [data-story="side-story"] button', { hasText: 'Remove' }).click();
  await p.waitForFunction(() => storyId === 'main');
  await p.waitForSelector('#admin [data-trash="side-story"]');
  assert(true, 'removing the open story goes back to the main one');
  [st, r] = await post('/api/admin/purge', { name: '../side-story' });
  const [st2] = await post('/api/admin/restore', { name: '..' });
  assert(st === 400 && st2 === 400 && fs.existsSync(main), 'removed-story names cannot point elsewhere');
  await p.locator('#admin [data-trash="side-story"] button', { hasText: 'Delete for good' }).click();
  await p.waitForFunction(() => !document.querySelector('#admin [data-trash]'));
  assert(!fs.existsSync(path.join(stories, '.removed', 'side-story')) && asked.some(m => /for good\? This cannot be undone/.test(m)), 'deleting for good asks first, then deletes');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
