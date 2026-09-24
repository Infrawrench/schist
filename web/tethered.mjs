// Standard PTP over WebUSB. No camera deletion or vendor mode changes.
const fail = (key) => { throw new Error(key); };
export function command(operation, transaction, parameters = []) {
  const bytes = new Uint8Array(12 + parameters.length * 4), view = new DataView(bytes.buffer);
  view.setUint32(0, bytes.length, true); view.setUint16(4, 1, true);
  view.setUint16(6, operation, true); view.setUint32(8, transaction, true);
  parameters.forEach((p, i) => view.setUint32(12 + i * 4, p, true));
  return bytes;
}
export function supportsCapture(bytes) {
  if (bytes.length < 9) return false;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const offset = 9 + bytes[8] * 2 + 2;
  if (offset + 4 > bytes.length) return false;
  const count = view.getUint32(offset, true);
  if (count > (bytes.length - offset - 4) / 2) return false;
  for (let i = 0; i < count; i++) if (view.getUint16(offset + 4 + i * 2, true) === 0x100e) return true;
  return false;
}
export function objectInfo(bytes) {
  if (bytes.length < 53) fail('tethered.no_download');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint16(4, true) === 0x3001) return null;
  const chars = bytes[52], size = view.getUint32(8, true);
  if (chars < 2 || 53 + chars * 2 > bytes.length || !size || size === 0xffffffff) fail('tethered.no_download');
  if (view.getUint16(53 + (chars - 1) * 2, true) !== 0) fail('tethered.no_download');
  const name = new TextDecoder('utf-16le', {fatal: true}).decode(bytes.subarray(53, 53 + (chars - 1) * 2));
  const extension = name.split('.').pop();
  if (!name.includes('.') || !/^[a-z0-9]{1,12}$/i.test(extension)) fail('tethered.no_download');
  return {size, extension: extension.toLowerCase()};
}
export class Ptp {
  constructor(device) { this.device = device; this.transaction = 0; this.buffer = new Uint8Array(); this.cancelled = false; }
  check() { if (this.cancelled) fail('common.cancelled'); }
  async open() {
    await this.device.open(); this.check();
    if (!this.device.configuration) await this.device.selectConfiguration(this.device.configurations[0].configurationValue);
    for (const face of this.device.configuration.interfaces) {
      const alt = face.alternates.find(a => a.interfaceClass === 6 && a.interfaceSubclass === 1 && a.interfaceProtocol === 1);
      if (!alt) continue;
      this.face = face.interfaceNumber;
      await this.device.claimInterface(this.face);
      await this.device.selectAlternateInterface(this.face, alt.alternateSetting);
      this.input = alt.endpoints.find(e => e.type === 'bulk' && e.direction === 'in')?.endpointNumber;
      this.output = alt.endpoints.find(e => e.type === 'bulk' && e.direction === 'out')?.endpointNumber;
      break;
    }
    if (this.input === undefined || this.output === undefined) fail('tethered.unsupported');
    await this.exchange(0x1002, [1]); this.session = true;
    if (!supportsCapture(await this.exchange(0x1001))) fail('tethered.unsupported');
  }
  async read(count) {
    const result = new Uint8Array(count); let offset = 0;
    while (offset < count) {
      this.check();
      if (!this.buffer.length) {
        const packet = await this.device.transferIn(this.input, 16384); this.check();
        if (packet.status !== 'ok' || !packet.data) fail('library.import.camera_disconnected');
        this.buffer = new Uint8Array(packet.data.buffer, packet.data.byteOffset, packet.data.byteLength);
        if (!this.buffer.length) continue;
      }
      const n = Math.min(count - offset, this.buffer.length);
      result.set(this.buffer.subarray(0, n), offset);
      this.buffer = this.buffer.subarray(n); offset += n;
    }
    return result;
  }
  async exchange(operation, parameters = [], maxData = 8 * 1024 * 1024) {
    this.check(); const id = this.transaction++, bytes = command(operation, id, parameters);
    const sent = await this.device.transferOut(this.output, bytes); this.check();
    if (sent.status !== 'ok' || sent.bytesWritten !== bytes.length) fail('library.import.camera_disconnected');
    let data;
    for (;;) {
      const header = new DataView((await this.read(12)).buffer);
      const length = header.getUint32(0, true), type = header.getUint16(4, true), code = header.getUint16(6, true);
      if (length < 12 || header.getUint32(8, true) !== id) fail('tethered.no_download');
      if (type === 3) {
        if (length > 32) fail('tethered.no_download');
        await this.read(length - 12);
        if (code !== 0x2001) fail(`PTP 0x${code.toString(16)}`);
        return data ?? new Uint8Array();
      }
      if (type !== 2 || data || code !== operation || length - 12 > maxData) fail('tethered.no_download');
      data = await this.read(length - 12);
    }
  }
  async handles() {
    const data = await this.exchange(0x1007, [0xffffffff, 0, 0]);
    if (data.length < 4) fail('tethered.no_download');
    const view = new DataView(data.buffer);
    if (view.getUint32(0, true) * 4 !== data.length - 4) fail('tethered.no_download');
    const values = new Set(); for (let p = 4; p < data.length; p += 4) values.add(view.getUint32(p, true));
    return values;
  }
  async capture() {
    const before = await this.handles(), added = new Set();
    await this.exchange(0x100e, [0, 0]);
    let last = Date.now();
    while (!added.size || Date.now() - last < 2000) {
      this.check(); await new Promise(resolve => setTimeout(resolve, 200));
      let current;
      try { current = await this.handles(); }
      catch (error) { if (error.message === 'PTP 0x2019') continue; throw error; }
      for (const id of current) if (!before.has(id) && !added.has(id)) { added.add(id); last = Date.now(); }
    }
    const files = []; let total = 0;
    for (const id of added) {
      const info = objectInfo(await this.exchange(0x1008, [id])); if (!info) continue;
      // Browser originals are held in memory; reject before allocating too much.
      total += info.size; if (total > 512 * 1024 * 1024) fail('tethered.no_download');
      const bytes = await this.exchange(0x1009, [id], info.size);
      if (bytes.length !== info.size) fail('tethered.no_download');
      files.push({extension: info.extension, bytes});
    }
    if (!files.length) fail('tethered.no_download');
    return files;
  }
  async close() {
    try { if (this.session && !this.cancelled) await this.exchange(0x1003); }
    finally { if (this.device.opened) await this.device.close(); }
  }
  cancel() { this.cancelled = true; return this.device.close().catch(() => {}); }
}

export function installTethered(host = window) {
  let device, active, sequence = 0, generation = 0;
  host.__schistTethered = async (operation) => {
    if (operation === 'cancel') { generation++; await active?.cancel(); return {}; }
    if (active) fail('library.import.already_running');
    if (!host.navigator.usb) fail('common.not_available');
    if (operation === 'choose') {
      // Invoke requestDevice synchronously in the click's activation scope.
      const current = ++generation;
      const chosen = await host.navigator.usb.requestDevice({filters: [{classCode: 6, subclassCode: 1, protocolCode: 1}]});
      if (current !== generation) fail('common.cancelled');
      device = chosen; return {model: device.productName || 'USB'};
    }
    if (!device) fail('tethered.unsupported');
    const camera = active = new Ptp(device);
    let timeout = false;
    const timer = setTimeout(() => { timeout = true; void camera.cancel(); }, 120000);
    try {
      await camera.open(); const files = await camera.capture(); camera.check();
      const counts = new Map(), number = ++sequence;
      return {files: files.map(file => {
        const n = (counts.get(file.extension) || 0) + 1; counts.set(file.extension, n);
        return {name: `capture-${String(number).padStart(6, '0')}${n > 1 ? `-${n}` : ''}.${file.extension}`, bytes: file.bytes};
      })};
    } catch (error) { if (timeout) fail('tethered.timeout'); throw error; }
    finally { try { await camera.close(); } finally { clearTimeout(timer); active = undefined; } }
  };
}
