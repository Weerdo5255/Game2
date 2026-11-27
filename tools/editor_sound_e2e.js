// End-to-end test of the editor's Sound view (voices and music), in a real
// browser, against the mock ComfyUI server (no GPU, models or account needed).
//
//   cargo build && node tools/editor_sound_e2e.js [screenshot-dir]
//
// Everything happens in a temporary folder; the repository is not touched.
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-sound-'));
const out = process.argv[2] || tmp;
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

const STORY = `title Sound Check
character ada | Ada | #ffcc66 | -
character bo | Bo | #66ccff | p
track calm | Calm Hall | 12 | soft strings, slow, warm

*start
bg pantry
music calm
n: The hall is quiet.
ada: Good evening, sir.
bo: Is it?
ada: It is now.
music off
end The End
`;

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets'));
  const story = path.join(tmp, 'assets', 'story.txt');
  fs.writeFileSync(story, STORY);
  fs.cpSync(path.join(root, 'comfyui', 'workflows'), path.join(tmp, 'comfyui', 'workflows'), { recursive: true });
  fs.copyFileSync(path.join(root, 'comfyui', 'node_specs.json'), path.join(tmp, 'comfyui', 'node_specs.json'));
  const log = path.join(tmp, 'mock.json');
  const mock = spawn('python3', [path.join(root, 'tools', 'mock_comfy_server.py'), log], { stdio: ['ignore', 'pipe', 'inherit'] });
  procs.push(mock);
  const mockUrl = await new Promise(r => mock.stdout.once('data', d => r(String(d).trim())));
  const cfg = JSON.parse(fs.readFileSync(path.join(root, 'comfyui', 'config.json'), 'utf8'));
  cfg.server = mockUrl;
  fs.writeFileSync(path.join(tmp, 'comfyui', 'config.json'), JSON.stringify(cfg, null, 2));
  const PORT = 7936;
  procs.push(spawn(exe, ['--editor', '--story', story, '--stories', path.join(tmp, 'stories'), '--comfy-dir', path.join(tmp, 'comfyui'),
    '--port', String(PORT), '--no-browser', '--job-timeout', '20'], { stdio: 'ignore', env: { ...process.env, COMFY_ORG_API_KEY: 'test-key-not-real' } }));
  await sleep(800);
  const sent = async n => { for (let i = 0; i < 100; i++) { try { const d = JSON.parse(fs.readFileSync(log, 'utf8')); if (d.prompts.length >= n) return d; } catch (e) { /* not written yet */ } await sleep(60); } throw new Error(`the mock never logged ${n} workflows`); };
  const classes = p => Object.values(p).map(n => n.class_type);
  const audioDir = path.join(tmp, 'assets', 'audio');
  const files = () => fs.existsSync(audioDir) ? fs.readdirSync(audioDir).filter(f => !f.startsWith('.') && f !== 'samples') : [];

  const b = browser = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
  p.setDefaultTimeout(15000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('dialog', d => d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  const jobsIdle = async () => { for (let i = 0; i < 200; i++) { await sleep(150); if (!(await p.evaluate(() => fetch('/api/art/jobs').then(r => r.json()))).jobs.some(j => j.state === 'queued' || j.state === 'running')) return; } throw new Error('jobs never finished'); };
  const jobs = () => p.evaluate(() => fetch('/api/art/jobs').then(r => r.json()).then(r => r.jobs));

  // ---- the story side: tracks and Music blocks
  assert(await p.evaluate(() => doc.tracks.length === 1 && doc.scenes[0].items.filter(i => i.t === 'music').length === 2), 'tracks and Music blocks load from the story');
  assert(await p.evaluate(() => fetch('/api/validate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(doc) }).then(r => r.json()).then(r => (r.problems || []).length === 0)), 'and the story has no problems');
  assert((await p.locator('.node .chip').allTextContents()).some(t => t.includes('Calm Hall')), 'a scene shows the music it starts');
  await p.click('.node');
  assert(await p.locator('.blk.music select').first().inputValue() === 'calm', 'the Music block picks from the tracks');

  // ---- the Sound view
  await p.click('#soundBtn');
  await p.waitForSelector('[data-speaker]');
  assert(await p.locator('[data-speaker]').count() === 3, 'the narrator and both characters are speakers');
  assert((await p.locator('[data-speaker="ada"] .chip').first().textContent()) === '0 of 2 lines recorded', 'lines are counted per speaker');
  assert(await p.locator('[data-track="calm"]').count() === 1, 'the track is listed');
  assert((await p.locator('#sound .bar').first().textContent()).includes('account key is set'), 'the page knows a partner key is set (and never sees it)');
  assert(!(await p.content()).includes('test-key-not-real'), 'the key itself is not sent to the page');
  await p.waitForFunction(() => document.querySelectorAll('[data-speaker="ada"] select').length >= 2);
  const stock = await p.locator('[data-speaker="ada"] select').nth(1).locator('option').allTextContents();
  assert(stock.length > 2, `stock voices are read from ComfyUI (${stock.length - 1})`);

  // music
  await p.click('[data-track="calm"] button.primary');
  let d = await sent(1); await jobsIdle();
  const m = d.prompts[0];
  assert(classes(m).some(c => c.startsWith('SaveAudio')) && classes(m).includes('EmptyLatentAudio'), 'the track goes through the music workflow');
  assert(Object.values(m).some(n => n.class_type === 'EmptyLatentAudio' && n.inputs.seconds === 12) && JSON.stringify(m).includes('soft strings'), 'with its own length and description');
  assert(files().some(f => f.startsWith('music_calm.')), 'the music file is saved beside the story: ' + files().join(', '));
  await p.waitForSelector('[data-track="calm"] audio');
  const played = await p.evaluate(() => fetch(document.querySelector('[data-track="calm"] audio').src).then(r => [r.status, r.headers.get('content-type')]));
  assert(played[0] === 200 && String(played[1]).startsWith('audio/'), 'and can be played back in the page');

  // a stock voice for Ada, then her missing lines
  await p.locator('[data-speaker="ada"] select').nth(1).selectOption({ index: 2 });
  const chosen = await p.evaluate(() => prefs.voices.ada.voice);
  await p.waitForSelector('[data-speaker="ada"] button.primary');
  await p.click('[data-speaker="ada"] button.primary');
  d = await sent(3); await jobsIdle();
  const v = d.prompts.slice(1, 3);
  assert(v.every(w => classes(w).includes('ElevenLabsTextToSpeech')), "Ada's two lines go through the voice workflow");
  assert(v.every(w => JSON.stringify(w).includes(JSON.stringify(chosen).slice(1, -1))), `in the voice chosen for her (${chosen})`);
  assert(v.some(w => JSON.stringify(w).includes('Good evening, sir.')) && v.some(w => JSON.stringify(w).includes('It is now.')), 'each with its own words');
  assert(d.extra.slice(1, 3).every(e => e && e.api_key_comfy_org === 'test-key-not-real'), 'the account key travels with partner jobs in extra_data');
  assert(!JSON.stringify(m).includes('test-key-not-real') && !fs.readFileSync(path.join(tmp, 'assets', 'art.json'), 'utf8').includes('test-key'), 'and is in no workflow or file');
  assert(files().filter(f => f.startsWith('voice_ada_')).length === 2, 'two recordings are saved');
  await p.waitForFunction(() => document.querySelector('[data-speaker="ada"] .chip').textContent === '2 of 2 lines recorded');
  assert(true, 'the card follows');
  assert(JSON.parse(fs.readFileSync(path.join(tmp, 'assets', 'art.json'), 'utf8')).voices.ada.voice === chosen, 'the voice choice is remembered with the story');

  // a cloned voice for Bo from a sample
  const wav = path.join(tmp, 'bo.wav');
  const pcm = Buffer.alloc(8000), head = Buffer.alloc(44);
  head.write('RIFF', 0); head.writeUInt32LE(36 + pcm.length, 4); head.write('WAVEfmt ', 8); head.writeUInt32LE(16, 16); head.writeUInt16LE(1, 20); head.writeUInt16LE(1, 22);
  head.writeUInt32LE(8000, 24); head.writeUInt32LE(8000, 28); head.writeUInt16LE(1, 32); head.writeUInt16LE(8, 34); head.write('data', 36); head.writeUInt32LE(pcm.length, 40);
  fs.writeFileSync(wav, Buffer.concat([head, pcm]));
  await p.locator('[data-speaker="bo"] select').first().selectOption('voice_f5_clone.json');
  await p.waitForSelector('[data-speaker="bo"] input[type=file]');
  await p.setInputFiles('[data-speaker="bo"] input[type=file]', wav);
  await p.waitForSelector('[data-speaker="bo"] audio');
  assert(fs.existsSync(path.join(audioDir, 'samples', 'bo.wav')), 'a voice sample is stored beside the story');
  await p.fill('[data-speaker="bo"] input[placeholder^="Exactly"]', 'This is how I speak.');
  await p.locator('[data-speaker="bo"] input[placeholder^="Exactly"]').blur();
  await sleep(200);
  await p.click('[data-speaker="bo"] button.primary');
  d = await sent(4); await jobsIdle();
  const c = d.prompts[3];
  assert(classes(c).includes('F5TTSAudioInputs') && classes(c).includes('LoadAudio'), "Bo's line goes through the cloning workflow");
  assert(JSON.stringify(c).includes('This is how I speak.') && JSON.stringify(c).includes('Is it?'), 'with the sample transcript and the line');
  assert((d.files || []).some(f => /bo/.test(f)), 'the sample was uploaded to ComfyUI: ' + (d.files || []).join(', '));
  assert(files().filter(f => f.startsWith('voice_bo_')).length === 1, "Bo's line is recorded");
  assert((await jobs()).every(j => j.state === 'done'), 'every job finished: ' + (await jobs()).filter(j => j.state !== 'done').map(j => j.message).join('; '));

  // lines list, and rewording
  await p.locator('[data-speaker="ada"] button', { hasText: 'Show lines' }).click();
  await p.waitForSelector('[data-speaker="ada"] .lrow');
  assert(await p.locator('[data-speaker="ada"] .lrow').count() === 2 && await p.locator('[data-speaker="ada"] .lrow audio').count() === 2, 'each line can be played back');
  if (out !== tmp) { fs.mkdirSync(out, { recursive: true }); await p.screenshot({ path: path.join(out, 'editor_sound.png') }); }
  await p.evaluate(() => change(() => { doc.scenes[0].items.find(i => i.t === 'say' && i.text === 'It is now.').text = 'It is, now you are here.'; }));
  await p.keyboard.press('Control+s');
  await p.waitForFunction(() => !dirty);
  await p.waitForFunction(() => document.querySelector('[data-speaker="ada"] .chip').textContent === '1 of 2 lines recorded');
  assert(true, 'a reworded line needs recording again');
  await p.locator('#sound button', { hasText: 'Tidy up' }).click();
  await sleep(500);
  assert(files().filter(f => f.startsWith('voice_ada_')).length === 1, 'tidying sets the stale recording aside');

  // a new track from the page
  await p.locator('#sound button', { hasText: '+ Track' }).click();
  assert(await p.evaluate(() => doc.tracks.length === 2 && dirty), 'a track can be added from the Sound view');
  await p.keyboard.press('Control+z');
  assert(await p.evaluate(() => doc.tracks.length === 1), 'and undone');
  await p.keyboard.press('Control+s'); await sleep(300);

  // ---- the game
  const text = fs.readFileSync(story, 'utf8');
  assert(text.includes('track calm | Calm Hall | 12 | soft strings, slow, warm') && text.includes('music calm') && text.includes('music off'), 'tracks and music survive saving');
  const run = spawnSync(exe, ['--story', story, '--autoplay', ''], { encoding: 'utf8' });
  const line = (run.stdout + run.stderr).split('\n').find(l => l.startsWith('sound:')) || '';
  assert(/sound: 2 voiced line/.test(line) && line.includes('calm'), 'the game reads the recorded lines and plays the track: ' + line);
  const silent = spawnSync(exe, ['--story', story, '--autoplay', '', '--silent'], { encoding: 'utf8' });
  assert(!(silent.stdout + silent.stderr).includes('sound:'), '--silent turns it all off');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
