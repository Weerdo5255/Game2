// End-to-end test of making characters and places with a language model,
// in a real browser, against a stand-in for the Claude API and Ollama
// (tools/mock_ai_server.py: no key, no network, no GPU).
//
//   cargo build && node tools/editor_ai_e2e.js [screenshot-dir]
//
// Needs Node with Playwright and Chromium, and Python 3. Everything happens
// in a temporary folder; the repository's stories are not touched.
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-ai-'));
const out = process.argv[2] || null;
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets'));
  const story = path.join(tmp, 'assets', 'story.txt');
  fs.copyFileSync(path.join(root, 'assets', 'story.txt'), story);
  const log = path.join(tmp, 'ai.json');
  const mock = spawn('python3', [path.join(root, 'tools', 'mock_ai_server.py'), log], { stdio: ['ignore', 'pipe', 'inherit'] });
  procs.push(mock);
  const mockUrl = await new Promise(r => mock.stdout.once('data', d => r(String(d).trim())));
  const PORT = 7941;
  const env = { ...process.env, ANTHROPIC_BASE_URL: mockUrl, ANTHROPIC_API_KEY: 'test-key-not-real', OLLAMA_HOST: mockUrl };
  delete env.ANTHROPIC_AUTH_TOKEN;
  procs.push(spawn(exe, ['--editor', '--story', story, '--stories', path.join(tmp, 'stories'), '--port', String(PORT), '--no-browser'], { stdio: 'ignore', env }));
  await sleep(800);
  const sent = () => JSON.parse(fs.readFileSync(log, 'utf8'));

  const b = browser = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('dialog', d => d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');

  // ---- what the page is told: whether there is a key (never the key), and Ollama's models
  const status = await p.evaluate(() => fetch('/api/ai').then(r => r.json()));
  assert(status.claude.ready === true && status.ollama.ok && status.ollama.models.includes('qwen3:32b'), 'the page learns that Claude has a key and which models Ollama has');
  assert(!JSON.stringify(status).includes('test-key-not-real'), 'the key itself is never sent to the page');

  // ---- a character, from Claude
  await p.click('.tabs button[data-tab="cast"]');
  const before = await p.evaluate(() => doc.characters.length);
  await p.click('#aiCharBtn');
  await p.waitForSelector('#aiDlg.show #aiProvider');
  await p.waitForFunction(() => ai.status);
  assert(await p.inputValue('#aiProvider') === 'claude', 'Claude is offered first when it has a key');
  await p.fill('#aiBrief', 'a sharp-eyed housekeeper who keeps every key');
  await p.click('#aiGo');
  await p.waitForSelector('#aiMade img.portrait');
  const req = sent().find(r => r.path === '/v1/messages');
  assert(req.keyed && req.headers['anthropic-version'] === '2026-10-21', 'the request carries the key and the API version');
  assert(req.body.model === 'claude-opus-5-5' && req.body.stream === true && req.body.output_config.format.type === 'json_schema' && req.body.output_config.effort === 'medium', 'Claude is asked by default, streamed, for JSON of a fixed shape');
  assert(req.body.fallbacks === 'default' && (req.headers['anthropic-beta'] || '').includes('server-side-fallback-2026-07-01'), 'a declined request may be handed to another model');
  assert(!('temperature' in req.body) && !('thinking' in req.body), 'no settings the API version refuses');
  assert(req.body.messages[0].content.includes('Below Stairs') && req.body.messages[0].content.includes('housekeeper'), 'the ask carries the story and the brief');
  if (out) await p.screenshot({ path: path.join(out, 'ai_character.png') });
  await p.click('#aiAdd');
  await p.waitForFunction(n => doc.characters.length === n + 1, before);
  const hobb = await p.evaluate(() => doc.characters[doc.characters.length - 1]);
  assert(hobb.id === 'mrs_hobb' && hobb.sprite === 'c' && hobb.look.age === '0.8' && hobb.look.coat === '#2a2a3a', 'the character joins the cast with the look that was made');
  assert(await p.evaluate(() => openChar === 'mrs_hobb' && tab === 'cast'), 'and is opened in the Cast list');
  await p.waitForFunction(() => prefs.prompts.spr_mrs_hobb);
  await sleep(300);
  const art = JSON.parse(fs.readFileSync(path.join(tmp, 'assets', 'art.json'), 'utf8'));
  assert((art.prompts.spr_mrs_hobb || '').includes('housekeeper'), "the description is kept as the character's ComfyUI prompt");

  // ---- refusal and a busy API are told plainly
  await p.click('#aiCharBtn');
  await p.fill('#aiBrief', 'please refuse this');
  await p.click('#aiGo');
  await p.waitForSelector('#aiError');
  assert((await p.textContent('#aiError')).includes('declined'), 'a refusal is told as one');
  await p.fill('#aiBrief', 'a busy footman');
  await p.click('#aiGo');
  await p.waitForSelector('#aiMade');
  assert(sent().filter(r => r.path === '/v1/messages').length === 4, 'a busy answer is tried again (after the time it asks for)');
  await p.click('#aiDlg button.ghost');

  // ---- a place, from Ollama, with a 3D room
  await p.click('.tabs button[data-tab="scenes"]');
  await p.click('#aiPlaceBtn');
  await p.waitForFunction(() => ai.status);
  await p.selectOption('#aiProvider', 'ollama');
  await p.selectOption('#aiModel', 'qwen3:32b');
  await p.fill('#aiBrief', "the servants' stair at night, one candle");
  await p.check('#aiWant3d');
  await p.click('#aiGo');
  await p.waitForSelector('#aiMade img.paint');
  const chat = sent().find(r => r.path === '/api/chat');
  assert(chat.body.model === 'qwen3:32b' && chat.body.format.properties.room && chat.body.stream === false, 'Ollama is asked with its model and the shape of a place with a room');
  assert(await p.evaluate(() => ai.made.painting.layers.length === 5 && ai.made.room.parts.length === 2), 'the painting comes back cleaned (a shape the game does not know is left out), with its room');
  if (out) await p.screenshot({ path: path.join(out, 'ai_place.png') });
  await p.click('#aiAdd');
  await p.waitForFunction(() => meta.bgs.includes('servants_stair'));
  const paintFile = path.join(tmp, 'assets', 'paintings', 'servants_stair.json');
  assert(fs.existsSync(paintFile) && fs.existsSync(path.join(tmp, 'assets', 'world', 'servants_stair.json')), 'the painting and the room are saved beside the story');
  const img = await p.evaluate(() => fetch(bgUrl('servants_stair')).then(r => r.status + ' ' + r.headers.get('content-type')));
  assert(img === '200 image/png', 'the new place is painted by the editor like the game\'s own');
  const world = await p.evaluate(() => fetch(forStory('/api/world?place=servants_stair')).then(r => r.json()));
  assert(world.custom && world.places.some(x => x.name === 'servants_stair'), 'and its room opens in the World editor');

  // ---- a story can use it: in a Background block, then saved without problems
  await p.evaluate(() => { change(() => { doc.scenes[0].items.unshift({ t: 'bg', name: 'servants_stair' }); }); });
  await p.evaluate(() => save());
  await p.waitForFunction(() => !dirty);
  const checked = await p.evaluate(() => fetch(forStory('/api/story')).then(r => r.json()));
  assert(!checked.problems.some(x => x.level === 'error' && x.msg.includes('servants_stair')), 'a scene may show the new place');
  const exported = await p.evaluate(() => fetch(forStory('/api/export?format=json')).then(r => r.json()));
  assert(exported.files && exported.files['paintings/servants_stair.json'], 'the painting travels with an exported story');
  const imported = await p.evaluate(e => fetch('/api/import', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(e) }).then(r => r.json()), exported);
  assert(imported.id && fs.existsSync(path.join(tmp, 'stories', imported.id, 'paintings', 'servants_stair.json')) && !(imported.problems || []).some(x => x.level === 'error'), 'and arrives with it when imported');
  const taken = await p.evaluate(() => fetch('/api/ai/place/save', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ story: storyId, name: 'pantry', painting: { layers: [] } }) }).then(r => r.status));
  assert(taken === 400, "the game's own places cannot be painted over");

  // ---- no key: said plainly, nothing sent
  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
