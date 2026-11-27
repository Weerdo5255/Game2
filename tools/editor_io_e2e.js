// End-to-end test of exporting and importing stories, in a real browser.
//
//   cargo build && node tools/editor_io_e2e.js
//
// Everything happens in a temporary folder; the repository is not touched.
const { chromium } = require('playwright');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const root = path.join(__dirname, '..');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'story-io-'));
const exe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'below-stairs.exe' : 'below-stairs');
const assert = (c, m) => { if (!c) { console.log('FAIL: ' + m); process.exitCode = 1; } else console.log('ok: ' + m); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
let browser = null;
// the start of a PNG of the given size (enough for the import's checks; never decoded here)
const png = (w, h) => { const b = Buffer.alloc(40); Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]).copy(b); b.write('IHDR', 12); b.writeUInt32BE(w, 16); b.writeUInt32BE(h, 20); return b; };
const PNG = png(16, 16);
const tree = dir => fs.existsSync(dir) ? fs.readdirSync(dir, { recursive: true }).map(String).sort() : [];

(async () => {
  fs.mkdirSync(path.join(tmp, 'assets', 'enhanced'), { recursive: true });
  fs.mkdirSync(path.join(tmp, 'assets', 'audio', 'samples'), { recursive: true });
  const main = path.join(tmp, 'assets', 'story.txt');
  fs.copyFileSync(path.join(root, 'assets', 'story.txt'), main);
  fs.writeFileSync(path.join(tmp, 'assets', 'enhanced', 'bg_vista.png'), PNG);
  fs.writeFileSync(path.join(tmp, 'assets', 'audio', 'music_estate.wav'), 'RIFF-pretend');
  fs.writeFileSync(path.join(tmp, 'assets', 'audio', 'samples', 'p.wav'), 'RIFF-sample');
  fs.writeFileSync(path.join(tmp, 'assets', 'audio', 'notes.exe'), 'not a recording');
  fs.writeFileSync(path.join(tmp, 'assets', 'art.json'), JSON.stringify({ prompts: { bg_vista: 'a wide green valley' }, workflows: { music: 'music_ace_step.json' }, voices: { p: { workflow: 'voice_f5_clone.json', sample: 'p.wav', sample_text: 'Hello there.', speed: 1.1 } } }));
  const PORT = 7938, stories = path.join(tmp, 'stories');
  procs.push(spawn(exe, ['--editor', '--story', main, '--stories', stories, '--comfy-dir', path.join(root, 'comfyui'), '--port', String(PORT), '--no-browser'], { stdio: 'ignore' }));
  await sleep(800);
  const b = browser = await chromium.launch();
  const ctx = await b.newContext({ viewport: { width: 1500, height: 900 }, acceptDownloads: true });
  const p = await ctx.newPage();
  p.setDefaultTimeout(15000);
  const errs = [];
  p.on('pageerror', e => errs.push('pageerror: ' + e.message));
  p.on('dialog', d => d.accept());
  await p.goto(`http://127.0.0.1:${PORT}/`);
  await p.waitForSelector('.node');
  const post = body => p.evaluate(body => fetch('/api/import', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).then(async r => [r.status, await r.json()]), body);
  const library = () => p.evaluate(() => fetch('/api/stories').then(r => r.json()).then(r => r.stories.map(s => s.id)));

  // ---- export
  const menu = await p.locator('#storySel option').allTextContents();
  assert(menu.some(t => t.includes('Import a story')) && menu.filter(t => t.includes('Export')).length === 3, 'the story menu offers import and three kinds of export');
  let [dl] = await Promise.all([p.waitForEvent('download'), p.selectOption('#storySel', { label: '⇩ Export this story (.json)' })]);
  const plainFile = path.join(tmp, 'plain.json');
  await dl.saveAs(plainFile);
  const plain = JSON.parse(fs.readFileSync(plainFile, 'utf8'));
  assert(dl.suggestedFilename() === 'below-stairs-above-stars.story.json', 'the download is named after the story: ' + dl.suggestedFilename());
  assert(plain.format === 'below-stairs-story' && plain.version === 1 && plain.story.scenes.length === 47 && plain.story.tracks.length === 4 && !plain.files, 'a plain export holds the whole story and no files');
  assert(plain.prefs.voices.p.sample === 'p.wav' && plain.prefs.prompts.bg_vista === 'a wide green valley', 'with its art and sound choices');
  assert(await p.evaluate(() => storyId === 'main' && document.querySelector('#storySel').value === 'main'), 'exporting leaves the open story open');
  [dl] = await Promise.all([p.waitForEvent('download'), p.selectOption('#storySel', { label: '⇩ Export with its art and sound (.json)' })]);
  const bundleFile = path.join(tmp, 'bundle.json');
  await dl.saveAs(bundleFile);
  const bundle = JSON.parse(fs.readFileSync(bundleFile, 'utf8'));
  assert(JSON.stringify(Object.keys(bundle.files)) === JSON.stringify(['enhanced/bg_vista.png', 'audio/music_estate.wav', 'audio/samples/p.wav']), 'a full export carries pictures, recordings and samples, and nothing else: ' + Object.keys(bundle.files));
  assert(Buffer.from(bundle.files['enhanced/bg_vista.png'], 'base64').equals(PNG), 'byte for byte');
  [dl] = await Promise.all([p.waitForEvent('download'), p.selectOption('#storySel', { label: '⇩ Export as a story text file (.txt)' })]);
  const textFile = path.join(tmp, 'tale.txt');
  await dl.saveAs(textFile);
  assert(fs.readFileSync(textFile, 'utf8') === fs.readFileSync(main, 'utf8') && dl.suggestedFilename().endsWith('.story.txt'), 'the text export is the story file itself');
  // unsaved changes are saved first, so the export is what is on screen
  await p.evaluate(() => change(() => { doc.subtitle = 'Exported edition'; }));
  [dl] = await Promise.all([p.waitForEvent('download'), p.selectOption('#storySel', { label: '⇩ Export this story (.json)' })]);
  assert(JSON.parse(fs.readFileSync(await dl.path(), 'utf8')).story.subtitle === 'Exported edition' && await p.evaluate(() => !dirty), 'exporting saves unsaved changes first');

  // ---- import through the menu
  const pick = async file => { const [chooser] = await Promise.all([p.waitForEvent('filechooser'), p.selectOption('#storySel', { label: '⇧ Import a story from a file…' })]); await chooser.setFiles(file); };
  await pick(bundleFile);
  await p.waitForFunction(() => storyId !== 'main');
  const id = await p.evaluate(() => storyId);
  await p.waitForFunction(() => doc.scenes.length === 47);
  assert(id === 'below-stairs-above-stars', 'an imported story becomes a new story in the library: ' + id);
  const folder = path.join(stories, id);
  assert(JSON.stringify(tree(folder)) === JSON.stringify(['art.json', 'audio', 'audio/music_estate.wav', 'audio/samples', 'audio/samples/p.wav', 'enhanced', 'enhanced/bg_vista.png', 'story.txt']), 'with its files in place: ' + tree(folder).join(' '));
  assert(fs.readFileSync(path.join(folder, 'enhanced', 'bg_vista.png')).equals(PNG) && fs.readFileSync(path.join(folder, 'story.txt'), 'utf8') === fs.readFileSync(textFile, 'utf8'), 'the pictures and the story are the same');
  assert(JSON.parse(fs.readFileSync(path.join(folder, 'art.json'), 'utf8')).voices.p.sample_text === 'Hello there.', 'and so are its voice settings');
  const again = await p.evaluate(() => fetch('/api/export?format=bundle&story=' + storyId).then(r => r.json()));
  assert(JSON.stringify(again.story) === JSON.stringify(bundle.story) && JSON.stringify(again.files) === JSON.stringify(bundle.files) && JSON.stringify(again.prefs) === JSON.stringify(bundle.prefs), 'exporting it again gives the same file');
  assert(fs.readFileSync(main, 'utf8').includes('Below Stairs') && (await library()).length === 2, 'the story it came from is untouched');
  const ending = f => (spawnSync(exe, ['--story', f, '--autoplay', '0,0,1,0,0,2,3'], { encoding: 'utf8' }).stdout.split('\n').find(l => l.startsWith('ending:')) || '');
  assert(ending(path.join(folder, 'story.txt')) === ending(main) && ending(main).includes('ending:'), 'the game plays the imported story the same way: ' + ending(main));

  // a story text file, and a bare story in JSON
  await pick(textFile);
  await p.waitForFunction(() => storyId === 'below-stairs-above-stars-2' && doc.scenes.length === 47 && doc.subtitle !== 'Exported edition');
  assert(await p.evaluate(() => doc.scenes.length === 47 && doc.characters.length === 8), 'a story text file imports too, under a name of its own');
  const bare = path.join(tmp, 'my_little_tale.json');
  const small = JSON.parse(JSON.stringify(plain.story)); small.title = ''; small.scenes = small.scenes.slice(0, 1); small.chapters = []; small.scenes[0].pos = [10.5, 20.25]; small.text = 'a note of my own'; small.characters[0].color = 'red;position:fixed;inset:0;background:url(http://example.invalid/x)';
  small.scenes[0].items = [{ t: 'say', who: 'n', text: 'Only this.' }, { t: 'end', title: 'Done', text: '' }];
  fs.writeFileSync(bare, JSON.stringify(small));
  await pick(bare);
  await p.waitForFunction(() => storyId === 'my-little-tale' && doc.scenes.length === 1);
  assert(await p.evaluate(() => doc.title === '' && doc.scenes.length === 1), 'a bare story in JSON imports; untitled, it stays untitled and its folder is named after the file');

  assert(await p.evaluate(() => JSON.stringify(doc.scenes[0].pos) === '[11,20]' || JSON.stringify(doc.scenes[0].pos) === '[10,20]'), 'positions with fractions are rounded, not refused');
  await p.evaluate(() => { tab = 'cast'; renderLeft(); });
  const styles = await p.evaluate(() => [...document.querySelectorAll('[style]')].map(e => e.getAttribute('style')).join('\n'));
  assert(!styles.includes('position:fixed') && !styles.includes('example.invalid'), "a character's colour from a file cannot carry other styling into the page");
  // unsaved work is never dropped by an import, and a damaged file is refused
  await p.selectOption('#storySel', 'main');
  await p.waitForFunction(() => storyId === 'main' && doc.scenes.length === 47);
  await p.evaluate(() => change(() => { doc.subtitle = 'Still being written'; }));
  await pick(plainFile);
  await p.waitForFunction(() => stories.length === 5);
  assert(await p.evaluate(() => storyId === 'main' && dirty && doc.subtitle === 'Still being written' && document.querySelector('#storySel').value === 'main'), 'importing while the open story has unsaved changes adds the story but stays put');
  await p.keyboard.press('Control+s'); await p.waitForFunction(() => !dirty);
  const cut = path.join(tmp, 'cut.bundle.json');
  fs.writeFileSync(cut, fs.readFileSync(bundleFile, 'utf8').slice(0, 5000));
  const n = (await library()).length;
  await pick(cut);
  await p.waitForFunction(() => /damaged or incomplete/.test(document.querySelector('#toast').textContent));
  assert((await library()).length === n, 'a cut-off export file is refused, not imported as nonsense');
  let [st2, r2] = await post({ text: '{"format": "below-stairs-story", "story": {"title": "x' });
  assert(st2 === 400 && /damaged/.test(r2.error), 'by the server too');
  const big = await p.evaluate(() => fetch('/api/validate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '"' + 'x'.repeat(9 << 20) + '"' }).then(r => r.status, () => 0));
  assert(big === 413 || big === 0, 'only an import may be large: ' + big);

  // ---- what an import may not do
  const before = await library(), outside = tree(tmp).filter(f => !f.startsWith('stories'));
  let [status, r] = await post({ hello: 1 });
  assert(status === 400 && /no story/.test(r.error), 'a file that is not a story is refused: ' + r.error);
  [status, r] = await post({ format: 'below-stairs-story', version: 99, story: plain.story });
  assert(status === 400 && /newer version/.test(r.error), 'so is one from a newer version');
  [status, r] = await post({ format: 'something-else', story: plain.story });
  assert(status === 400, 'and one of another kind');
  [status, r] = await post({ text: 'this is not\na story at all\n' });
  assert(status === 400, 'and text with no scenes: ' + r.error);
  assert(JSON.stringify(await library()) === JSON.stringify(before), 'none of them left anything in the library');
  const evil = { format: 'below-stairs-story', version: 1, story: { ...plain.story, title: 'Evil' }, prefs: { workflows: { voice: '../../x.json', music: 'music_ace_step.json' }, voices: { p: { sample: '../../../story.txt', workflow: '/etc/passwd' }, '../x': { voice: 'a' } } },
    files: { '../evil.png': PNG.toString('base64'), 'enhanced/../../evil.png': PNG.toString('base64'), '/tmp/evil.png': PNG.toString('base64'), 'audio/samples/../../../evil.wav': 'UklGRg==', 'enhanced/.hidden.png': PNG.toString('base64'),
      'enhanced/notpng.png': Buffer.from('<html>').toString('base64'), 'audio/run.exe': 'TVo=', 'story.txt': 'eA==', 'art.json': 'eA==', 'enhanced/sub/x.png': PNG.toString('base64'), 'enhanced/ok.png': PNG.toString('base64'), 'audio/bad.wav': '***', 'enhanced/huge.png': png(20000, 20000).toString('base64'), 'enhanced/con.png': PNG.toString('base64'), 'audio/nul.wav': 'UklGRg==', 'audio/COM1.x.wav': 'UklGRg==' }, junk: new Array(20000).fill(0) };
  [status, r] = await post(evil);
  assert(status === 200 && r.files === 1 && r.skipped.length === 15, `files that are not a story's pictures or recordings are left out (${r.files} kept, ${r.skipped.length} skipped)`);
  assert(JSON.stringify(tree(path.join(stories, r.id))) === JSON.stringify(['art.json', 'enhanced', 'enhanced/ok.png', 'story.txt']), 'only the good one is written: ' + tree(path.join(stories, r.id)).join(' '));
  assert(JSON.stringify(tree(tmp).filter(f => !f.startsWith('stories'))) === JSON.stringify(outside) && !fs.existsSync('/tmp/evil.png') && tree(stories).filter(f => f.includes('evil.')).length === 0, 'and nothing is written outside the new story');
  const ep = JSON.parse(fs.readFileSync(path.join(stories, r.id, 'art.json'), 'utf8'));
  assert(ep.voices.p.sample === '' && ep.voices.p.workflow === '' && !ep.voices['../x'] && !ep.workflows.voice && ep.workflows.music === 'music_ace_step.json', 'its settings cannot point outside either');
  const cross = await p.evaluate(() => fetch('/api/import', { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: '{"text":"*start\\nend X"}' }).then(r => r.status));
  assert(cross === 403, 'imports are only accepted from the editor page itself');

  console.log('page errors:', JSON.stringify(errs));
  if (errs.length) process.exitCode = 1;
  await b.close();
})().catch(e => { console.log('CRASH', e.message); process.exitCode = 1; }).finally(async () => { if (browser) await browser.close().catch(() => {}); procs.forEach(c => c.kill()); fs.rmSync(tmp, { recursive: true, force: true }); });
