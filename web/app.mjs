import { Emulator, AudioPlayer, FRAME_MS, KEYS, MAX_ROM_BYTES, gamepadMask, cleanDirections, romKey, encodeSave, decodeSave } from './engine.mjs';

const $ = id => document.getElementById(id);
const canvas = $('screen');
const context = canvas.getContext('2d', { alpha: false });
const frameImage = context.createImageData(256, 240);
const sound = new AudioPlayer();
const heldKeys = new Set();
const heldPointers = new Map();
let core = null, current = null, running = false, loading = false, faulted = false;
let loadSerial = 0, lastTick = 0, elapsed = 0, lastSave = 0, fpsStart = 0, frames = 0, lastPadScan = 0;
let catalogue = [], gameButtons = [], audioWarningShown = false;

function notice(message, error = false) {
  $('notice-text').textContent = message;
  $('notice').classList.toggle('error', error);
  $('notice').hidden = false;
}
$('dismiss-notice').addEventListener('click', () => { $('notice').hidden = true; });

function clearInput() {
  heldKeys.clear(); heldPointers.clear();
  document.querySelectorAll('[data-button]').forEach(button => button.classList.remove('pressed'));
  if (core) { core.buttons(0); core.buttons(0, 1); }
}
function paint() { frameImage.data.set(core.pixels()); context.putImageData(frameImage, 0, 0); }
function update() {
  $('play-pause').textContent = running ? 'Pause' : 'Play';
  $('play-pause').disabled = !current || loading || faulted;
  $('resume').disabled = !current || loading || faulted;
  $('reset').disabled = !current || loading;
  $('import-rom').disabled = !core || loading;
  $('pause-screen').hidden = !current || running || loading || faulted;
  $('export-save').disabled = !current || !core?.saveSize || loading;
  $('import-save').disabled = !current || !core?.saveSize || loading;
  gameButtons.forEach((button, i) => {
    button.disabled = !core || loading || !catalogue[i].supported;
    button.setAttribute('aria-current', String(current?.file === catalogue[i].file && current?.bundled));
  });
}
function updateSaveInfo() {
  if (!current) return;
  $('save-info').textContent = !core.saveSize
    ? 'This cartridge does not use battery saves. Progress lasts until you reset or close the game.'
    : current.saveBlocked
      ? 'The existing browser save could not be restored and has been left untouched. Import a valid .sav to replace it, or export your current progress.'
      : !current.key || current.storageFailed
        ? 'Automatic saves are unavailable. Export a .sav file to keep your progress.'
        : 'Battery saves are stored in this browser every five seconds and when you pause. Export a .sav to take your progress with you.';
}
function persist() {
  if (!current?.key || !core.saveSize || current.saveBlocked) return;
  const encoded = encodeSave(core.save());
  if (encoded === current.lastSaved) return;
  try {
    localStorage.setItem(current.key, encoded); current.lastSaved = encoded; current.storageFailed = false;
  } catch {
    if (!current.storageFailed) notice('Browser storage is unavailable or full. Export a .sav file to keep your progress.');
    current.storageFailed = true;
  }
  updateSaveInfo();
}
function setRunning(value) {
  running = Boolean(value && current && !faulted);
  clearInput(); sound.flush(); lastTick = 0; elapsed = 0; frames = 0; fpsStart = 0;
  if (!running) persist();
  update();
}
async function enableSound() {
  try { await sound.enable(); }
  catch {
    sound.setMuted(true); updateSound();
    if (!audioWarningShown) notice('Audio could not start in this browser. You can still play silently; use Sound to retry.');
    audioWarningShown = true;
  }
}
function resume() {
  if (!current || loading || faulted) return;
  void enableSound(); setRunning(true); canvas.focus({ preventScroll: true });
}
function togglePause() { if (running) setRunning(false); else resume(); }
function updateSound() {
  $('sound').setAttribute('aria-pressed', String(!sound.muted));
  $('sound').querySelector('span').textContent = sound.muted ? 'Sound off' : 'Sound on';
}
function toggleSound() { sound.setMuted(!sound.muted); sound.flush(); if (!sound.muted) void enableSound(); updateSound(); }

async function openRom(readBytes, file, title, bundled = false) {
  if (!core) { notice('The emulator is still starting. Try again in a moment.'); return; }
  const ticket = ++loadSerial;
  setRunning(false); loading = true; update();
  // Unlock Web Audio in the gesture, before asynchronous file loading.
  void enableSound();
  let installed = false;
  try {
    const bytes = new Uint8Array(await readBytes());
    if (ticket !== loadSerial) return;
    if (bytes.length > MAX_ROM_BYTES) throw new Error('ROM is too large. The limit is 16 MiB.');
    let key = null;
    try { key = await romKey(bytes); } catch { /* Gameplay and manual saves still work without Web Crypto. */ }
    if (ticket !== loadSerial) return;
    // A failed ROM load preserves the previous core and its save RAM.
    core.load(bytes); installed = true; faulted = false;
    current = { file, title, bundled, key, saveBlocked: false, storageFailed: false, lastSaved: null };
    $('notice').hidden = true;
    if (core.saveSize && key) {
      try {
        const stored = localStorage.getItem(key);
        if (stored !== null) {
          try { core.importSave(decodeSave(stored, core.saveSize)); current.lastSaved = stored; }
          catch (error) { current.saveBlocked = true; notice(`Saved progress could not be restored: ${error.message} The stored copy has been kept.`, true); }
        }
      } catch { current.storageFailed = true; notice('Browser storage is unavailable. Use .sav import/export to keep progress.'); }
    }
    $('game-title').textContent = title;
    $('idle-screen').hidden = true;
    updateSaveInfo();
    core.frame(); paint();
    loading = false; setRunning(!document.hidden);
    canvas.focus({ preventScroll: true });
  } catch (error) {
    if (ticket !== loadSerial) return;
    if (installed) faulted = true;
    running = false;
    notice(error.message || 'That ROM could not be opened.', true);
  } finally {
    if (ticket === loadSerial) { loading = false; update(); }
  }
}
function openLocal(file) {
  if (!file) return;
  if (!file.name.toLowerCase().endsWith('.nes')) { notice('Choose an uncompressed .nes ROM file.', true); return; }
  if (!file.size || file.size > MAX_ROM_BYTES) { notice('Choose a ROM between 1 byte and 16 MiB.', true); return; }
  return openRom(() => file.arrayBuffer(), file.name, file.name.replace(/\.nes$/i, '').replaceAll('_', ' '));
}
async function readBundled(game) {
  const response = await fetch(new URL(game.url, import.meta.url));
  if (!response.ok) throw new Error(`Could not load ${game.title}. You can import a local ROM instead.`);
  const size = Number(response.headers.get('content-length'));
  if (size > MAX_ROM_BYTES) throw new Error('This ROM exceeds the 16 MiB limit.');
  return response.arrayBuffer();
}
function renderCatalogue() {
  const list = $('game-list'); list.replaceChildren(); gameButtons = [];
  if (!catalogue.length) {
    const empty = document.createElement('p'); empty.className = 'loading';
    empty.textContent = 'No homebrews supplied. Import a ROM to play.'; list.append(empty);
  }
  for (const game of catalogue) {
    const button = document.createElement('button'); button.className = 'game-card';
    button.setAttribute('aria-label', `Play ${game.title}`);
    button.textContent = game.title;
    if (!game.supported) button.title = `Mapper ${game.mapper} unsupported`;
    button.addEventListener('click', () => openRom(() => readBundled(game), game.file, game.title, true));
    list.append(button); gameButtons.push(button);
  }
  update();
}

$('import-rom').addEventListener('click', () => $('rom-file').click());
$('rom-file').addEventListener('change', event => { void openLocal(event.target.files[0]); event.target.value = ''; });
$('play-pause').addEventListener('click', togglePause);
$('resume').addEventListener('click', resume);
$('reset').addEventListener('click', reset);
function reset() {
  if (!current || loading) return;
  const wasRunning = running;
  persist(); setRunning(false);
  try { core.reset(); faulted = false; core.frame(); paint(); setRunning(wasRunning); canvas.focus({ preventScroll: true }); }
  catch (error) { faulted = true; notice(error.message, true); update(); }
}
$('sound').addEventListener('click', toggleSound);
$('volume').addEventListener('input', event => { sound.setVolume(Number(event.target.value) / 100); });
$('fullscreen').addEventListener('click', fullscreen);
async function fullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await document.querySelector('.console').requestFullscreen();
  } catch { notice('Fullscreen is unavailable in this browser. You can keep playing here.'); }
}
if (!document.querySelector('.console').requestFullscreen) $('fullscreen').disabled = true;

document.addEventListener('keydown', event => {
  if (event.target.closest('input,textarea,select,[contenteditable=true]')) return;
  if ((event.code === 'Space' || event.code === 'Enter') && event.target.closest('button,summary,a')) return;
  if (current && !loading && event.code in KEYS) {
    event.preventDefault(); if (running) heldKeys.add(event.code);
  } else if (!event.repeat) {
    if (event.code === 'Space' || event.code === 'KeyP') { if (current) { event.preventDefault(); togglePause(); } }
    if (event.code === 'KeyR') reset();
    if (event.code === 'KeyM') toggleSound();
    if (event.code === 'KeyF') void fullscreen();
  }
});
document.addEventListener('keyup', event => { heldKeys.delete(event.code); });
window.addEventListener('blur', () => { if (running) setRunning(false); else clearInput(); });
document.addEventListener('visibilitychange', () => { if (document.hidden) setRunning(false); });
window.addEventListener('pagehide', () => { persist(); clearInput(); sound.flush(); });

const touchToggle = $('touch-toggle');
touchToggle.checked = matchMedia('(pointer: coarse)').matches;
function updateTouch() { $('touch-controls').hidden = !touchToggle.checked; clearInput(); }
touchToggle.addEventListener('change', updateTouch); updateTouch();
for (const button of document.querySelectorAll('[data-button]')) {
  button.addEventListener('pointerdown', event => {
    if (!running) return;
    event.preventDefault(); button.setPointerCapture(event.pointerId);
    heldPointers.set(event.pointerId, Number(button.dataset.button)); button.classList.add('pressed');
  });
  const release = event => {
    heldPointers.delete(event.pointerId);
    if (![...heldPointers.values()].includes(Number(button.dataset.button))) button.classList.remove('pressed');
  };
  button.addEventListener('pointerup', release); button.addEventListener('pointercancel', release); button.addEventListener('lostpointercapture', release);
  button.addEventListener('contextmenu', event => event.preventDefault());
}

let dragDepth = 0;
window.addEventListener('dragenter', event => {
  if (!Array.from(event.dataTransfer?.types || []).includes('Files')) return;
  event.preventDefault(); dragDepth++; $('drop-cover').hidden = false; $('import-card').classList.add('dragging');
});
window.addEventListener('dragover', event => {
  if (Array.from(event.dataTransfer?.types || []).includes('Files')) { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; }
});
window.addEventListener('dragleave', () => { if (--dragDepth <= 0) hideDrop(); });
function hideDrop() { dragDepth = 0; $('drop-cover').hidden = true; $('import-card').classList.remove('dragging'); }
window.addEventListener('drop', event => {
  event.preventDefault(); hideDrop();
  const files = event.dataTransfer?.files;
  if (files?.length) { if (files.length !== 1) notice('Drop one ROM at a time.', true); else void openLocal(files[0]); }
});

function download(bytes, filename) {
  const url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
  const anchor = document.createElement('a'); anchor.href = url; anchor.download = filename;
  document.body.append(anchor); anchor.click(); anchor.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
}
$('export-save').addEventListener('click', () => { if (current && core.saveSize) download(core.save(), current.file.replace(/\.nes$/i, '') + '.sav'); });
$('import-save').addEventListener('click', () => $('save-file').click());
$('save-file').addEventListener('change', async event => {
  const file = event.target.files[0]; event.target.value = '';
  if (!file || !current || !core.saveSize) return;
  const target = current;
  setRunning(false);
  try {
    if (file.size !== core.saveSize) throw new Error(`This cartridge needs a ${core.saveSize}-byte .sav file.`);
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (current !== target || loading) throw new Error('The game changed while reading the save. Choose the save again.');
    core.importSave(bytes); core.reset(); faulted = false;
    current.saveBlocked = false; current.lastSaved = null; persist(); updateSaveInfo(); update();
    notice('Save imported. Press Play to start the game with your restored progress.');
  } catch (error) { notice(error.message, true); }
});

function pads() { try { return Array.from(navigator.getGamepads?.() || []).filter(p => p?.connected); } catch { return []; } }
function updatePads(connected) {
  const standard = connected.filter(p => p.mapping === 'standard');
  $('gamepad-status').textContent = standard.length ? `● ${standard.length} gamepad${standard.length === 1 ? '' : 's'} connected` : connected.length ? '○ This gamepad needs a standard mapping' : '○ No gamepad connected';
}
window.addEventListener('gamepadconnected', () => updatePads(pads()));
window.addEventListener('gamepaddisconnected', () => { clearInput(); updatePads(pads()); });
function loop(time) {
  requestAnimationFrame(loop);
  const connected = pads();
  if (time - lastPadScan > 1000) { updatePads(connected); lastPadScan = time; }
  if (!running || !current || loading) return;
  if (!lastTick) { lastTick = time; fpsStart = time; return; }
  elapsed += Math.min(time - lastTick, 100); lastTick = time;
  let mask = 0;
  for (const code of heldKeys) mask |= KEYS[code];
  for (const value of heldPointers.values()) mask |= value;
  const standard = connected.filter(p => p.mapping === 'standard');
  core.buttons(cleanDirections(mask | gamepadMask(standard[0])));
  core.buttons(cleanDirections(gamepadMask(standard[1])), 1);
  try {
    let steps = 0;
    while (elapsed >= FRAME_MS && steps < 3) {
      core.frame(); sound.queue(core.audio(), core.sampleRate);
      elapsed -= FRAME_MS; steps++; frames++;
    }
    if (steps) paint();
    if (elapsed >= FRAME_MS) elapsed %= FRAME_MS;
    if (time - fpsStart >= 1000) {
      frames = 0; fpsStart = time;
    }
    if (time - lastSave >= 5000) { persist(); lastSave = time; }
  } catch (error) {
    faulted = true; setRunning(false); notice(`Emulation stopped: ${error.message} Try Reset or choose another ROM.`, true);
  }
}
requestAnimationFrame(loop);

async function boot() {
  const enginePromise = Emulator.fetch(new URL('./semulate.wasm', import.meta.url));
  const cataloguePromise = fetch(new URL('./roms.json', import.meta.url)).then(response => {
    if (!response.ok) throw new Error('Game catalogue unavailable. Import your own ROM to play.');
    return response.json();
  });
  const results = await Promise.allSettled([enginePromise, cataloguePromise]);
  if (results[0].status === 'fulfilled') {
    core = results[0].value; $('ready-hint').textContent = 'Choose a homebrew below or import a ROM.';
  } else {
    $('ready-hint').textContent = 'Could not start the emulator.';
    notice(`Could not start WebAssembly: ${results[0].reason.message} Serve this site over HTTP or HTTPS, then refresh.`, true);
  }
  if (results[1].status === 'fulfilled' && Array.isArray(results[1].value)) catalogue = results[1].value;
  else if (core) notice('Game catalogue unavailable. You can still import your own ROM.');
  renderCatalogue(); update();
}
void boot();
