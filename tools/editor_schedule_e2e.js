// End-to-end test of character schedules, interruptions, the clock and
// events: set up in the editor in a real browser, then played by the game.
//
//   cargo build && node tools/editor_schedule_e2e.js [screenshot-dir]
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-sched-'));
const out = process.argv[2] || tmp;
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

const STORY = `title Clockwork
character m | Marlowe | #96e6a0 | -
character p | Peregrine | #c8b4ff | p
character q | Quince | #ffd27f | q
var alarm | Alarm | 0 | bad

*start
bg pantry
n: Morning in the pantry.
m: Is anyone about?
bg study
n: The study.
m: And here?
end Done
`;

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets'));
  const main = path.join(tmp, 'assets', 'story.txt');
  fs.writeFileSync(main, STORY);
  const PORT = 7944;
  procs.push(spawn(exe, ['--editor', '--story', main, '--stories', path.join(tmp, 'stories'), '--comfy-dir', path.join(root, 'comfyui'), '--port', String(PORT), '--no-browser'], { stdio: 'ignore' }));
  await sleep(800);
  const b = browser = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
  p.setDefaultTimeout(15000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  let answer = 'fire';
  p.on('dialog', d => d.type() === 'prompt' ? d.accept(answer) : d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  const save = async () => { await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty); return fs.readFileSync(main, 'utf8'); };
  const play = () => { const r = spawnSync(exe, ['--story', main, '--autoplay', '0'], { encoding: 'utf8' }); return (r.stdout + r.stderr).trim().split('\n'); };
  const problemsNow = () => p.evaluate(() => fetch('/api/validate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(doc) }).then(r => r.json()).then(r => r.problems.filter(x => x.level === 'error').map(x => x.msg)));

  // ---- a schedule without a clock is pointed out; then the clock
  await p.click('.tabs button[data-tab="cast"]');
  await p.locator('#leftBody .card .head').nth(2).click();   // Quince
  await p.click('[data-add="stop"]');
  assert((await problemsNow()).some(m => m.includes('keeps no clock')), 'a schedule without a clock is a problem the editor reports');
  await p.click('.tabs button[data-tab="scenes"]');
  await p.check('#clockOn');
  await p.fill('#perLine', '10'); await p.locator('#perLine').blur();
  await p.locator('#leftBody input[type=time]').first().fill('08:00'); await p.locator('#leftBody input[type=time]').first().blur();
  assert(await p.evaluate(() => doc.clock.start === 480 && doc.clock.per_line === 10), 'the story is given a clock: when the day starts, how long a line takes');

  // ---- Quince's day
  await p.click('.tabs button[data-tab="cast"]');
  const stop = k => p.locator(`[data-stop="${k}"]`);
  await stop(0).locator('input[type=time]').fill('06:00'); await stop(0).locator('input[type=time]').blur();
  await stop(0).locator('select').selectOption('pantry');
  await p.click('[data-add="stop"]');
  assert(await p.evaluate(() => doc.characters[2].schedule[1].at === 420 && doc.characters[2].schedule[1].place === 'pantry'), 'a new entry follows on an hour after the last');
  await stop(1).locator('input[type=time]').fill('08:15'); await stop(1).locator('input[type=time]').blur();
  await stop(1).locator('select').selectOption('away');
  await stop(1).locator('input[type=text]').fill('errands'); await stop(1).locator('input[type=text]').blur();
  // ---- and what interrupts it: an event, made up on the spot
  await p.click('[data-add="interrupt"]');
  const intr = p.locator('[data-interrupt="0"]');
  await intr.locator('select').first().selectOption({ label: '⚡ a new event…' });
  await p.waitForFunction(() => doc.characters[2].interrupts[0].when.var === 'event.fire');
  await p.locator('[data-interrupt="0"] select').nth(2).selectOption('study');
  assert(await p.evaluate(() => { const i = doc.characters[2].interrupts[0]; return i.when.op === '==' && i.when.val === 1 && i.place === 'study'; }), 'an interruption sends them somewhere while an event is on');
  assert((await p.locator('#dayTable').textContent()).includes('06:00') && (await p.locator('#dayTable').textContent()).includes('Quince → study when fire'), 'the day table shows the schedule and the interruption');
  let text = await save();
  assert(text.includes('clock 08:00 | 10') && text.includes('schedule q | 06:00 pantry | 08:15 away errands') && text.includes('interrupt q | event fire | study'), 'all of it is written to the story file');
  assert((await problemsNow()).length === 0, 'and the story has no problems');
  if (out !== tmp) { fs.mkdirSync(out, { recursive: true }); await p.screenshot({ path: path.join(out, 'editor_schedule.png') }); }

  // ---- the game: there by schedule, gone by schedule
  let lines = play();
  let moves = (lines.find(l => l.startsWith('schedule:')) || '');
  assert(moves.includes('08:00 q is in pantry') && !moves.includes('study'), 'the game has Quince in the pantry by schedule alone, and not in the study: ' + moves);
  const example = spawnSync(exe, ['--story', path.join(root, 'assets', 'example_schedule.txt'), '--autoplay', '0'], { encoding: 'utf8' }).stdout;
  assert(example.includes('08:20 q leaves pantry') && example.includes('c arrives in vista') && example.includes('c leaves vista') && example.includes('q is in study') && example.includes('ending: Done'), 'the example story has people leave on time, come running for an event, and go back after it');

  // ---- an Event block brings them to the study; a Time block moves the clock
  await p.evaluate(() => { const si = sceneIndex('start'); select(si, 3, true); });
  await p.locator('#insp button', { hasText: /^\+ Event$/ }).first().click().catch(() => {});
  if (!(await p.evaluate(() => doc.scenes[0].items.some(i => i.t === 'event')))) await p.evaluate(() => change(() => doc.scenes[0].items.splice(3, 0, { t: 'event', name: 'fire', on: true })));
  assert(await p.evaluate(() => doc.scenes[0].items.some(i => i.t === 'event' && i.name === 'fire' && i.on)), 'an Event block starts the event');
  text = await save();
  assert(/^event fire$/m.test(text), "written as 'event fire'");
  moves = (play().find(l => l.startsWith('schedule:')) || '');
  assert(moves.includes('q is in study'), 'and the interruption takes Quince to the study: ' + moves);
  await p.evaluate(() => change(() => { const items = doc.scenes[0].items; items.splice(items.findIndex(i => i.t === 'event'), 1, { t: 'time', at: 7 * 60 }, { t: 'pass', minutes: 5 }); }));
  await p.evaluate(() => select(sceneIndex('start'), -1, true));
  await p.waitForSelector('#insp .blk input[type=time]');
  assert((await p.locator('.node .chip').allTextContents()).some(t => t.includes('07:00')), 'a Time block shows on the scene');
  text = await save();
  assert(/^time 07:00$/m.test(text) && /^pass 5$/m.test(text), "Time and Time passes are written as 'time 07:00' and 'pass 5'");
  moves = (play().find(l => l.startsWith('schedule:')) || '');
  assert(moves === 'schedule: 08:00 q is in pantry', 'a time already past is tomorrow\'s: at five past seven next morning Quince is not yet in the study: ' + moves);

  // ---- conditions can ask who is here, and the time
  await p.evaluate(() => change(() => { doc.scenes[0].items.splice(3, 0, { t: 'if', var: '@q', op: '==', val: 1, target: 'start' }); }));
  await p.evaluate(() => select(sceneIndex('start'), 3, true));
  const condSel = p.locator('#insp .blk.on select').first();
  assert((await condSel.locator('option').allTextContents()).some(t => t.includes('Quince is here')) && (await condSel.locator('option').allTextContents()).some(t => t.includes('the time of day')), 'a condition can ask whether someone is here, or the time');
  await condSel.selectOption('clock');
  await p.waitForSelector('#insp .blk.on input[type=time]');
  await p.locator('#insp .blk.on input[type=time]').fill('13:30'); await p.locator('#insp .blk.on input[type=time]').blur();
  assert(await p.evaluate(() => { const c = doc.scenes[0].items[3]; return c.var === 'clock' && c.op === '>=' && c.val === 810; }), 'the time is entered as a time');
  await p.keyboard.press('Control+z'); await p.keyboard.press('Control+z'); await p.keyboard.press('Control+z');
  // renaming a character keeps their day, and conditions about them
  await p.evaluate(() => change(() => { doc.scenes[0].items.splice(3, 0, { t: 'if', var: '@q.study', op: '==', val: 1, target: 'start' }); renameEverywhere('char', 'q', 'quince'); doc.characters[2].id = 'quince'; }));
  assert(await p.evaluate(() => doc.scenes[0].items[3].var === '@quince.study' && doc.characters[2].schedule.length === 2), 'renaming a character keeps their schedule and the conditions that ask after them');
  await p.keyboard.press('Control+z');
  // an interrupt may not wait on where someone is
  await p.evaluate(() => change(() => doc.characters[1].interrupts.push({ when: { var: '@q', op: '==', val: 1 }, place: 'study', note: '' })));
  assert((await problemsNow()).some(m => m.includes('depends on where someone is')), 'an interruption that waits on where someone is, is refused');
  await p.keyboard.press('Control+z');
  // round trip through export
  const exported = await p.evaluate(() => fetch('/api/export?format=json&story=main').then(r => r.json()));
  assert(exported.story.clock.start === 480 && exported.story.characters[2].schedule.length === 2 && exported.story.characters[2].interrupts[0].when.var === 'event.fire', 'schedules travel with an exported story');
  await p.reload({ waitUntil: 'commit' }); await p.waitForSelector('.node');
  assert(await p.evaluate(() => doc.characters[2].schedule[1].note === 'errands' && doc.clock.per_line === 10), 'and reload as saved');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
