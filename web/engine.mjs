export const WIDTH = 256;
export const HEIGHT = 240;
export const MAX_ROM_BYTES = 16 * 1024 * 1024;
export const FRAME_MS = 1000 / 60.0988;
export const BUTTONS = { A: 1, B: 2, Select: 4, Start: 8, Up: 16, Down: 32, Left: 64, Right: 128 };
export const KEYS = { KeyZ: 1, KeyX: 2, ShiftRight: 4, ShiftLeft: 4, Enter: 8, ArrowUp: 16, ArrowDown: 32, ArrowLeft: 64, ArrowRight: 128 };

export class Emulator {
  constructor(instance) { this.api = instance.exports; }
  static async fromBytes(bytes) {
    const { instance } = await WebAssembly.instantiate(bytes, {});
    return new Emulator(instance);
  }
  static async fetch(url) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Could not load the emulator (${response.status}). Refresh to retry.`);
    return Emulator.fromBytes(await response.arrayBuffer());
  }
  error() {
    const ptr = this.api.semulate_error();
    const len = this.api.semulate_error_len();
    return new TextDecoder().decode(new Uint8Array(this.api.memory.buffer, ptr, len));
  }
  checked(result) { if (!result) throw new Error(this.error() || 'The emulator could not complete that action.'); }
  input(bytes) {
    if (!(bytes instanceof Uint8Array)) bytes = new Uint8Array(bytes);
    if (!bytes.length || bytes.length > MAX_ROM_BYTES) throw new Error('Choose a file between 1 byte and 16 MiB.');
    const ptr = this.api.semulate_input(bytes.length);
    if (!ptr) throw new Error(this.error());
    // Allocation may grow memory; always obtain a fresh view afterwards.
    new Uint8Array(this.api.memory.buffer, ptr, bytes.length).set(bytes);
  }
  load(bytes) { this.input(bytes); this.checked(this.api.semulate_load()); }
  reset() { this.checked(this.api.semulate_reset()); }
  buttons(mask, port = 0) { this.api.semulate_buttons(port, mask); }
  frame() { this.checked(this.api.semulate_frame()); }
  pixels() {
    const ptr = this.api.semulate_pixels(), len = this.api.semulate_pixels_len();
    return new Uint8ClampedArray(this.api.memory.buffer, ptr, len);
  }
  audio() {
    const ptr = this.api.semulate_audio(), len = this.api.semulate_audio_len();
    return new Float32Array(this.api.memory.buffer, ptr, len).slice();
  }
  get mapper() { return this.api.semulate_mapper(); }
  get sampleRate() { return this.api.semulate_sample_rate(); }
  get saveSize() { return this.api.semulate_save_len(); }
  save() {
    const ptr = this.api.semulate_save(), len = this.saveSize;
    return new Uint8Array(this.api.memory.buffer, ptr, len).slice();
  }
  importSave(bytes) { this.input(bytes); this.checked(this.api.semulate_import_save()); }
}

export function gamepadMask(pad) {
  if (!pad || pad.connected === false || pad.mapping !== 'standard') return 0;
  const down = n => Boolean(pad.buttons[n]?.pressed);
  let mask = (down(0) ? 1 : 0) | (down(1) || down(2) ? 2 : 0) | (down(8) ? 4 : 0) | (down(9) ? 8 : 0);
  if (down(12) || pad.axes[1] < -0.5) mask |= 16;
  if (down(13) || pad.axes[1] > 0.5) mask |= 32;
  if (down(14) || pad.axes[0] < -0.5) mask |= 64;
  if (down(15) || pad.axes[0] > 0.5) mask |= 128;
  return mask;
}

export function cleanDirections(mask) {
  if ((mask & 48) === 48) mask &= ~48;
  if ((mask & 192) === 192) mask &= ~192;
  return mask;
}

export class AudioPlayer {
  constructor(factory = () => new (globalThis.AudioContext || globalThis.webkitAudioContext)()) {
    this.factory = factory; this.context = null; this.gain = null;
    this.nextTime = 0; this.sources = new Set(); this.muted = false; this.volume = 0.65;
  }
  async enable() {
    if (!this.context) {
      this.context = this.factory(); this.gain = this.context.createGain();
      this.gain.connect(this.context.destination); this.setVolume(this.volume);
    }
    if (this.context.state !== 'running') await this.context.resume();
  }
  setVolume(value) {
    this.volume = Math.max(0, Math.min(1, Number(value)));
    if (this.gain) this.gain.gain.setTargetAtTime(this.muted ? 0 : this.volume, this.context.currentTime, 0.015);
  }
  setMuted(muted) { this.muted = muted; this.setVolume(this.volume); }
  queue(samples, sampleRate) {
    if (!samples.length || this.muted || this.context?.state !== 'running') return;
    const now = this.context.currentTime;
    // Bound latency after background tabs, slow frames, or a sleeping device.
    if (this.nextTime > now + 0.18) return;
    if (this.nextTime < now + 0.008) this.nextTime = now + 0.035;
    const buffer = this.context.createBuffer(1, samples.length, sampleRate);
    buffer.copyToChannel(samples, 0);
    const source = this.context.createBufferSource();
    source.buffer = buffer; source.connect(this.gain);
    this.sources.add(source);
    source.onended = () => { this.sources.delete(source); source.disconnect(); };
    source.start(this.nextTime);
    this.nextTime += samples.length / sampleRate;
  }
  flush() {
    for (const source of this.sources) { try { source.stop(); } catch {} source.disconnect(); }
    this.sources.clear(); this.nextTime = 0;
  }
}

export async function romKey(bytes) {
  const digest = await globalThis.crypto.subtle.digest('SHA-256', bytes);
  return 'semulate:save:v1:' + [...new Uint8Array(digest)].map(n => n.toString(16).padStart(2, '0')).join('');
}
export function encodeSave(bytes) {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}
export function decodeSave(text, size) {
  if (text.length > Math.ceil(size / 3) * 4 + 4) throw new Error('Stored save has an unexpected size.');
  const binary = atob(text);
  if (binary.length !== size) throw new Error(`Stored save must be ${size} bytes.`);
  return Uint8Array.from(binary, c => c.charCodeAt(0));
}
