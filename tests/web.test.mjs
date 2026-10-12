import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { Emulator, AudioPlayer, gamepadMask, cleanDirections, romKey, encodeSave, decodeSave } from '../web/engine.mjs';

const wasm = await readFile(new URL('../dist/semulate.wasm', import.meta.url));
const module = await WebAssembly.compile(wasm);
const player = async () => new Emulator(await WebAssembly.instantiate(module));
function fixture(code = [0x4c, 0, 0x80], battery = true, mapper = 0) {
  const rom = new Uint8Array(16 + 16384);
  rom.set([0x4e, 0x45, 0x53, 0x1a, 1, 0, (mapper << 4) | (battery ? 2 : 0), mapper & 0xf0]);
  rom.fill(0xea, 16); rom.set(code, 16);
  for (const n of [6, 4, 2]) rom.set([0, 0x80], rom.length - n);
  return rom;
}

test('the exact release WebAssembly is standalone and creates an empty player', async () => {
  assert.deepEqual(WebAssembly.Module.imports(module), []);
  const core = await Emulator.fromBytes(wasm);
  assert.throws(() => core.frame(), /Choose a ROM/);
  assert.equal(core.saveSize, 0);
  assert.equal(core.sampleRate, 48000);
  assert.equal(core.pixels().length, 256 * 240 * 4);
});

test('ROM validation is transactional; failed loads keep the previous save and game', async () => {
  const core = await player(); core.load(fixture());
  const save = new Uint8Array(8192).fill(0x5a); core.importSave(save);
  for (const bad of [new Uint8Array([1, 2, 3]), fixture([], true, 5), fixture().subarray(0, 30)]) {
    assert.throws(() => core.load(bad));
    assert.equal(core.mapper, 0); assert.deepEqual(core.save(), save); core.frame();
  }
  assert.throws(() => core.load(new Uint8Array()), /1 byte/);
  assert.equal(core.api.semulate_input(0xffffffff), 0);
  assert.match(core.error(), /16 MiB/);
});

test('battery saves round-trip, reject the wrong size, and survive reset', async () => {
  const core = await player(); core.load(fixture());
  const save = Uint8Array.from({ length: 8192 }, (_, i) => (i * 37) & 255);
  core.importSave(save); assert.deepEqual(core.save(), save);
  assert.throws(() => core.importSave(new Uint8Array(8191)), /exactly 8192/);
  assert.deepEqual(core.save(), save);
  core.reset(); assert.deepEqual(core.save(), save);
  core.load(fixture(undefined, false)); assert.equal(core.saveSize, 0);
  assert.throws(() => core.importSave(save), /no battery/);
});

test('controller buttons traverse the actual CPU serial protocol and release cleanly', async () => {
  const core = await player();
  // Strobe controller 1; write all eight serial bits into battery RAM, then repeat.
  core.load(fixture([0xa9, 1, 0x8d, 0x16, 0x40, 0xa9, 0, 0x8d, 0x16, 0x40, 0xa2, 0,
    0xad, 0x16, 0x40, 0x29, 1, 0x9d, 0, 0x60, 0xe8, 0xe0, 8, 0xd0, 0xf3, 0x4c, 0, 0x80]));
  core.buttons(0b10100101); core.frame();
  assert.deepEqual([...core.save().slice(0, 8)], [1, 0, 1, 0, 0, 1, 0, 1]);
  core.buttons(0); core.frame(); assert.deepEqual([...core.save().slice(0, 8)], Array(8).fill(0));
  core.buttons(255, 99); // Out-of-range ports must not trap.
});

test('fresh memory views survive a large failed upload and repeated cartridge replacement', async () => {
  const core = await player(); core.load(fixture()); core.frame();
  assert.throws(() => core.load(new Uint8Array(4 * 1024 * 1024)), /iNES/);
  for (let i = 0; i < 20; i++) { core.load(fixture()); core.frame(); }
  assert.equal(core.pixels().length, 245760);
  assert.equal(core.pixels()[3], 255);
  assert.ok(core.api.memory.buffer.byteLength < 32 * 1024 * 1024);
});

for (const file of ['MANTLE1_BETA1.nes', 'RooftopNinja.nes']) {
  test(`${file}: ten seconds of actual WASM emulation with finite audio and changing video`, async () => {
    const core = await player(); core.load(await readFile(new URL(`../roms/${file}`, import.meta.url)));
    let samples = 0, loudest = 0; const screens = new Set();
    for (let frame = 0; frame < 600; frame++) {
      core.buttons(frame >= 120 && frame < 130 ? 8 : frame >= 240 && frame < 300 ? 128 : 0);
      core.frame();
      const audio = core.audio();
      assert.ok(audio.length >= 700 && audio.length <= 900, `bounded per-frame audio: ${audio.length}`);
      for (const sample of audio) { assert.ok(Number.isFinite(sample) && Math.abs(sample) <= 1); loudest = Math.max(loudest, Math.abs(sample)); }
      samples += audio.length;
      if (frame % 100 === 0) {
        const pixels = core.pixels(); let hash = 0;
        for (let i = 0; i < pixels.length; i++) hash = ((hash * 31) ^ pixels[i]) >>> 0;
        screens.add(hash);
      }
    }
    assert.ok(samples > 470000 && samples < 490000);
    assert.ok(loudest > 0.001, 'game generated audible samples');
    assert.ok(screens.size > 1, 'frames are not a frozen placeholder');
  });
}

test('save identities follow ROM contents and binary saves preserve every byte', async () => {
  const a = fixture(); const b = a.slice(); b[50] ^= 1;
  assert.equal(await romKey(a), await romKey(a.slice()));
  assert.notEqual(await romKey(a), await romKey(b));
  const bytes = Uint8Array.from({ length: 32768 }, (_, i) => i & 255);
  assert.deepEqual(decodeSave(encodeSave(bytes), bytes.length), bytes);
  assert.throws(() => decodeSave(encodeSave(bytes), 8192), /size/);
  assert.throws(() => decodeSave('invalid %%', 8192));
});

test('standard gamepads and opposite directions map consistently', () => {
  const pad = { connected: true, mapping: 'standard', buttons: Array.from({ length: 16 }, () => ({ pressed: false })), axes: [0.8, -0.8] };
  pad.buttons[0].pressed = true; pad.buttons[9].pressed = true;
  assert.equal(gamepadMask(pad), 1 | 8 | 16 | 128);
  assert.equal(gamepadMask({ ...pad, mapping: '' }), 0);
  assert.equal(gamepadMask(null), 0);
  assert.equal(cleanDirections(255), 15);
});

test('audio schedules 48 kHz samples independently of device rate, bounds latency and flushes', async () => {
  const starts = [], buffers = [], sources = [];
  const context = {
    sampleRate: 44100, currentTime: 1, state: 'suspended', destination: {},
    async resume() { this.state = 'running'; },
    createGain() { return { connect() {}, gain: { setTargetAtTime() {} } }; },
    createBuffer(channels, length, rate) { buffers.push({ channels, length, rate }); return { copyToChannel() {} }; },
    createBufferSource() {
      const source = { connect() {}, disconnect() {}, start(t) { starts.push(t); }, stop() { this.stopped = true; } };
      sources.push(source); return source;
    },
  };
  const audio = new AudioPlayer(() => context); await audio.enable();
  for (let i = 0; i < 100; i++) audio.queue(new Float32Array(800), 48000);
  assert.ok(starts.length > 1 && starts.length < 15);
  assert.ok(starts.every((v, i) => !i || v > starts[i - 1]));
  assert.ok(buffers.every(b => b.channels === 1 && b.rate === 48000));
  audio.flush(); assert.ok(sources.every(s => s.stopped)); assert.equal(audio.sources.size, 0);
  audio.setMuted(true); audio.queue(new Float32Array(800), 48000); assert.equal(sources.length, starts.length);
});
