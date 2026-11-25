// End-to-end test of the editor's story library and Art view, in a real
// browser, against the mock ComfyUI server (no GPU or models needed).
//
//   cargo build && node tools/editor_art_e2e.js [screenshot-dir]
//
// Needs Node with Playwright and Chromium, and Python 3. Everything happens
// in a temporary folder; the repository's stories and settings are not touched.
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-art-'));
const out = process.argv[2] || tmp;
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

(async () => {
  // a private copy of the main story and of the ComfyUI suite
  fs.mkdirSync(path.join(tmp, 'assets'));
  const mainStory = path.join(tmp, 'assets', 'story.txt');
  fs.copyFileSync(path.join(root, 'assets', 'story.txt'), mainStory);
  fs.cpSync(path.join(root, 'comfyui', 'workflows'), path.join(tmp, 'comfyui', 'workflows'), { recursive: true });
  fs.copyFileSync(path.join(root, 'comfyui', 'node_specs.json'), path.join(tmp, 'comfyui', 'node_specs.json'));
  const log = path.join(tmp, 'mock.json');
  const mock = spawn('python3', [path.join(root, 'tools', 'mock_comfy_server.py'), log], { stdio: ['ignore', 'pipe', 'inherit'] });
  procs.push(mock);
  const mockUrl = await new Promise(r => mock.stdout.once('data', d => r(String(d).trim())));
  const cfg = JSON.parse(fs.readFileSync(path.join(root, 'comfyui', 'config.json'), 'utf8'));
  cfg.server = mockUrl;
  fs.writeFileSync(path.join(tmp, 'comfyui', 'config.json'), JSON.stringify(cfg, null, 2));
  const PORT = 7934;
  procs.push(spawn(exe, ['--editor', '--story', mainStory, '--stories', path.join(tmp, 'stories'), '--comfy-dir', path.join(tmp, 'comfyui'),
    '--port', String(PORT), '--no-browser', '--job-timeout', '20'], { stdio: 'ignore' }));
  await sleep(800);
  const sent = () => JSON.parse(fs.readFileSync(log, 'utf8'));
  const classes = p => Object.values(p).map(n => n.class_type);

  const b = browser = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1600, height: 950 } });
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  let answer = 'Second Tale';
  p.on('dialog', d => d.type() === 'prompt' ? d.accept(answer) : d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  const jobsIdle = async () => { for (let i = 0; i < 200; i++) { await sleep(150); if (!(await p.evaluate(() => fetch('/api/art/jobs').then(r => r.json()))).jobs.some(j => j.state === 'queued' || j.state === 'running')) return; } throw new Error('jobs never finished'); };
  const jobs = () => p.evaluate(() => fetch('/api/art/jobs').then(r => r.json()).then(r => r.jobs));

  // ---- stories
  assert((await p.locator('#storySel option').allTextContents()).includes('Below Stairs, Above Stars'), 'the main story is listed by its title');
  await p.selectOption('#storySel', { label: '+ New story…' });
  await p.waitForFunction(() => storyId === 'second-tale');
  const second = path.join(tmp, 'stories', 'second-tale', 'story.txt');
  assert(fs.existsSync(second) && fs.readFileSync(second, 'utf8').includes('title Second Tale'), 'a new story gets its own folder and file');
  await p.waitForFunction(() => doc.title === 'Second Tale' && document.querySelectorAll('.node').length === 1);
  assert(await p.evaluate(() => location.hash) === '#story=second-tale', 'the editor switched to the new story');
  await p.click('.tabs button[data-tab="scenes"]');
  await p.locator('#leftBody .card input').nth(1).fill('A much shorter one');
  await p.click(`.node .nh`);
  await p.locator('#insp .blk.say textarea').first().fill('This tale is not that tale.');
  await p.keyboard.press('Control+s'); await sleep(500);
  const text2 = fs.readFileSync(second, 'utf8');
  assert(text2.includes('title Second Tale | A much shorter one') && text2.includes('This tale is not that tale.'), 'edits are saved to the new story');
  assert(fs.readFileSync(mainStory, 'utf8') === fs.readFileSync(path.join(root, 'assets', 'story.txt'), 'utf8'), 'the main story is untouched');
  const played = spawnSync(exe, ['--story', second, '--autoplay', '0'], { encoding: 'utf8' });
  assert(played.status === 0 && played.stdout.includes('ending: The End'), 'the game plays the new story: ' + played.stdout.trim().split('\n').pop());

  // ---- art
  await p.click('#artBtn');
  await p.waitForSelector('#art .acard');
  await p.waitForFunction(() => art.server !== null);
  assert(await p.locator('#art .lamp.ok').count() === 1, 'the Art view sees the ComfyUI server');
  assert((await p.locator('#art .bar select').first().locator('option').allTextContents()).includes('sd_xl_base_1.0.safetensors'), "checkpoints come from the server's own list");
  console.log('   cards', await p.locator('#art .acard').count(), 'faces', await p.locator('#art .crow .face').count(), 'server', JSON.stringify(await p.evaluate(() => art.server)).slice(0, 200));
  assert(await p.locator('#art .acard').count() === 7 && await p.locator('#art .crow .face').count() === 6, 'seven backgrounds and the six expressions of the one character are offered');

  // one background, default workflow
  await p.locator('#art .acard:has-text("vista") >> nth=0').locator('button:has-text("Send")').first().click();
  await jobsIdle();
  const enh = path.join(tmp, 'stories', 'second-tale', 'enhanced');
  assert(fs.existsSync(path.join(enh, 'bg_vista.png')), 'the enhanced background lands in the story\'s own folder');
  assert(classes(sent().prompts[0]).includes('VAEEncode') && classes(sent().prompts[0]).includes('ControlNetApplyAdvanced'), 'default workflow: img2img with ControlNet');
  await p.waitForFunction(() => document.querySelector('#art .acard .badge.enh'));
  assert(await p.locator('#art .acard .badge.enh').count() === 1, 'the card shows it as enhanced');

  // a different workflow for all backgrounds, and one sent through a third, once
  await p.locator('#art h2:has-text("Backgrounds") select').selectOption({ label: 'Reimagine (structure only)' });
  await sleep(300);
  assert(JSON.parse(fs.readFileSync(path.join(tmp, 'stories', 'second-tale', 'art.json'), 'utf8')).workflows.background === 'background_reimagine.json', 'the workflow choice is remembered per story');
  await p.locator('#art .acard:has-text("pantry")').locator('button:has-text("Send")').first().click();
  await jobsIdle();
  assert(classes(sent().prompts[1]).includes('EmptyLatentImage'), 'chosen workflow is used: reimagine starts from noise');
  await p.locator('#art .acard:has-text("vista") >> nth=0').locator('select').selectOption({ label: 'Refine (light second pass)' });
  await jobsIdle();
  const refine = sent();
  assert(!classes(refine.prompts[2]).some(c => c.startsWith('ControlNet')) && refine.files.includes('bsas_second-tale_bg_vista_current.png'), 'a one-off workflow: refine starts from the current enhanced image');

  // a character: own prompt, neutral then expressions built on it
  const promptBox = p.locator('#art .crow textarea').first();
  await promptBox.fill('a weathered sea captain with a white beard'); await promptBox.dispatchEvent('change');
  await sleep(300);
  await p.click('#art .crow button:has-text("Send all six")');
  await jobsIdle();
  const all = sent(), spr = all.prompts.slice(3);
  assert(spr.length === 6 && fs.readdirSync(enh).filter(f => f.startsWith('spr_hero_')).length === 6, 'all six expressions were generated');
  assert(spr.every(q => JSON.stringify(q).includes('a weathered sea captain with a white beard')), "the story's own prompt for the character is used");
  assert(!classes(spr[0]).includes('SetLatentNoiseMask') && spr.slice(1).every(q => classes(q).includes('SetLatentNoiseMask')), 'neutral first, then face-only expression passes');
  const neutralBytes = fs.readFileSync(path.join(enh, 'spr_hero_neutral.png'));
  assert(neutralBytes[25] === 6, 'character results are transparent PNGs');
  await p.waitForFunction(() => document.querySelectorAll('#art .face .badge.enh').length === 6);
  await p.screenshot({ path: path.join(out, 'art.png') });

  // failure is reported, not swallowed
  await p.evaluate(() => fetch('/api/art/config', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ checkpoint: 'missing.safetensors' }) }));
  await p.locator('#art .acard:has-text("study")').locator('button:has-text("Send")').first().click();
  await jobsIdle();
  const failed = (await jobs()).find(j => j.asset === 'bg_study');
  assert(failed.state === 'failed' && failed.message.includes('missing.safetensors'), 'a failed job says why: ' + failed.message.slice(0, 90));
  assert(!fs.existsSync(path.join(enh, 'bg_study.png')), 'and writes nothing');
  await p.evaluate(() => fetch('/api/art/config', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ checkpoint: 'sd_xl_base_1.0.safetensors' }) }));
  // an expression cannot be made before its neutral
  await p.evaluate(() => fetch('/api/art/revert', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ story: 'second-tale', assets: ['spr_hero_neutral'] }) }));
  await p.evaluate(() => fetch('/api/art/run', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ story: 'second-tale', assets: ['spr_hero_smug'] }) }));
  await jobsIdle();
  assert((await jobs()).find(j => j.asset === 'spr_hero_smug' && j.state === 'failed').message.includes('neutral portrait first'), 'an expression without its enhanced neutral is refused with a clear message');

  // revert
  await p.waitForFunction(() => !document.querySelector('#art .badge.run')); await sleep(400);
  await p.locator('#art .acard:has-text("pantry")').locator('button:has-text("Revert")').click();
  await sleep(600);
  assert(!fs.existsSync(path.join(enh, 'bg_pantry.png')) && fs.existsSync(path.join(enh, '.removed', 'bg_pantry.png')), 'revert sets the enhanced file aside');

  // ---- independence, copying, removing
  const used = spawnSync(exe, ['--story', second, '--autoplay', '0'], { encoding: 'utf8' });
  assert(used.stdout.includes('enhanced art: ' + enh), 'the game uses that story\'s enhanced folder');
  await p.click('#artBtn');
  await p.selectOption('#storySel', 'main');
  await p.waitForFunction(() => storyId === 'main' && doc.scenes.length > 40);
  const mainArt = await p.evaluate(() => fetch('/api/art?story=main').then(r => r.json()));
  assert(mainArt.assets.length === 49 && !mainArt.assets.some(a => a.enhanced) && !fs.existsSync(path.join(tmp, 'assets', 'enhanced')), "the main story's art is separate and untouched");
  answer = 'Third Tale';
  await p.selectOption('#storySel', 'second-tale');
  await p.waitForFunction(() => storyId === 'second-tale');
  await p.selectOption('#storySel', { label: '⧉ Duplicate this story…' });
  await p.waitForFunction(() => storyId === 'third-tale');
  const third = path.join(tmp, 'stories', 'third-tale');
  assert(fs.readFileSync(path.join(third, 'story.txt'), 'utf8').includes('This tale is not that tale.') && fs.existsSync(path.join(third, 'enhanced', 'bg_vista.png')), 'a duplicate carries the story and its art');
  await p.selectOption('#storySel', { label: '✕ Remove this story…' });
  await p.waitForFunction(() => storyId === 'main');
  assert(!fs.existsSync(third) && fs.existsSync(path.join(tmp, 'stories', '.removed', 'third-tale', 'story.txt')), 'removing a story moves its folder aside');
  await p.waitForFunction(() => doc.scenes.length > 40);
  assert((await p.locator('#storySel option').allTextContents()).filter(t => t.includes('Tale')).length === 1, 'the story menu follows');
  const bad = await p.evaluate(() => fetch('/api/story?story=..%2F..%2Fetc').then(r => r.status));
  assert(bad === 404, 'story ids cannot escape the library');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
