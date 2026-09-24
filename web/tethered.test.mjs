import test from 'node:test';
import assert from 'node:assert/strict';
import {command, supportsCapture, objectInfo, Ptp, installTethered} from './tethered.mjs';

const packet = (type, code, id, payload = new Uint8Array()) => {
  const bytes = new Uint8Array(12 + payload.length), view = new DataView(bytes.buffer);
  view.setUint32(0, bytes.length, true); view.setUint16(4, type, true);
  view.setUint16(6, code, true); view.setUint32(8, id, true); bytes.set(payload, 12);
  return bytes;
};
const info = Uint8Array.from([0,0,0,0,0,0,0,0,0,0,0,2,0,0,0,1,16,14,16]);

test('only advertised standard capture is supported; malformed datasets are rejected', () => {
  assert.equal(supportsCapture(info), true);
  for (let end = 0; end < info.length; end++) assert.equal(supportsCapture(info.subarray(0, end)), false);
  const huge = info.slice(); huge.fill(255, 11, 15); assert.equal(supportsCapture(huge), false);
  assert.deepEqual([...command(0x100e, 7, [0,0])], [20,0,0,0,1,0,14,16,7,0,0,0,0,0,0,0,0,0,0,0]);
});
test('object filenames cannot escape storage and declared lengths must be complete', () => {
  const name = '../photo.CR3', bytes = new Uint8Array(53 + (name.length + 1) * 2), view = new DataView(bytes.buffer);
  view.setUint32(8, 1024, true); bytes[52] = name.length + 1;
  [...name].forEach((c, i) => view.setUint16(53 + i * 2, c.charCodeAt(0), true));
  assert.deepEqual(objectInfo(bytes), {size: 1024, extension: 'cr3'});
  assert.throws(() => objectInfo(bytes.subarray(0, bytes.length - 1)), /tethered.no_download/);
  view.setUint32(8, 0xffffffff, true); assert.throws(() => objectInfo(bytes), /tethered.no_download/);
});
test('USB packet boundaries and response transaction IDs are checked', async () => {
  const data = packet(2, 0x1001, 0, info), response = packet(3, 0x2001, 0);
  const all = new Uint8Array(data.length + response.length); all.set(data); all.set(response, data.length);
  let offset = 0;
  const device = {
    transferOut: async (_, bytes) => ({status: 'ok', bytesWritten: bytes.length}),
    transferIn: async () => { const part = all.slice(offset, offset += 5); return {status: 'ok', data: new DataView(part.buffer)}; },
  };
  const camera = new Ptp(device); camera.input = 1; camera.output = 2;
  assert.deepEqual(await camera.exchange(0x1001), info);
  offset = data.length; camera.transaction = 4;
  await assert.rejects(camera.exchange(0x1001), /tethered.no_download/);
});
test('oversized download is rejected before allocating it', async () => {
  const header = packet(2, 0x1009, 0); new DataView(header.buffer).setUint32(0, 0xfffffffe, true);
  const camera = new Ptp({transferOut: async (_, b) => ({status: 'ok', bytesWritten: b.length}), transferIn: async () => ({status: 'ok', data: new DataView(header.buffer)})});
  await assert.rejects(camera.exchange(0x1009, [1], 1024), /tethered.no_download/);
});
test('cancel closes USB and blocks further protocol operations', async () => {
  let closed = 0; const camera = new Ptp({close: async () => {closed++;}});
  await camera.cancel(); assert.equal(closed, 1);
  await assert.rejects(camera.exchange(0x100e), /common.cancelled/);
});
test('permission is requested synchronously and stale permission cannot select a camera', async () => {
  let requested = false, resolve;
  const host = {navigator: {usb: {requestDevice() {requested = true; return new Promise(r => {resolve = r;});}}}};
  installTethered(host);
  const pending = host.__schistTethered('choose'); assert.equal(requested, true);
  await host.__schistTethered('cancel'); resolve({productName: 'Camera'});
  await assert.rejects(pending, /common.cancelled/);
});
test('browsers without WebUSB fail explicitly', async () => {
  const host = {navigator: {}}; installTethered(host);
  await assert.rejects(host.__schistTethered('choose'), /common.not_available/);
});

test('one exposure downloads a RAW+JPEG pair, closes its session and never deletes objects', async () => {
  const operations = []; let queue = [], fired = false, closed = false;
  const u32s = values => { const data = new Uint8Array(values.length * 4), view = new DataView(data.buffer); values.forEach((v,i) => view.setUint32(i*4,v,true)); return data; };
  const fileInfo = (name, size) => {
    const bytes = new Uint8Array(53 + (name.length + 1) * 2), view = new DataView(bytes.buffer);
    view.setUint32(8, size, true); bytes[52] = name.length + 1;
    [...name].forEach((c,i) => view.setUint16(53+i*2,c.charCodeAt(0),true)); return bytes;
  };
  const device = {
    opened: false,
    configuration: {interfaces: [{interfaceNumber: 0, alternates: [{interfaceClass: 6, interfaceSubclass: 1, interfaceProtocol: 1, alternateSetting: 0,
      endpoints: [{type: 'bulk', direction: 'in', endpointNumber: 1}, {type: 'bulk', direction: 'out', endpointNumber: 2}]}]}]},
    async open() { this.opened = true; }, async close() { this.opened = false; closed = true; },
    async claimInterface() {}, async selectAlternateInterface() {},
    async transferOut(_, bytes) {
      const view = new DataView(bytes.buffer), op = view.getUint16(6,true), id = view.getUint32(8,true);
      operations.push(op); let data;
      if (op === 0x1001) data = info;
      else if (op === 0x1007) data = fired ? u32s([2,11,12]) : u32s([0]);
      else if (op === 0x100e) fired = true;
      else if (op === 0x1008) data = fileInfo(view.getUint32(12,true) === 11 ? 'photo.jpg' : 'photo.cr3', 3);
      else if (op === 0x1009) data = new Uint8Array([1,2,3]);
      if (data) queue.push(packet(2,op,id,data));
      queue.push(packet(3,0x2001,id)); return {status: 'ok', bytesWritten: bytes.length};
    },
    async transferIn() { const bytes = queue.shift(); assert.ok(bytes, 'unexpected USB read'); return {status: 'ok', data: new DataView(bytes.buffer)}; },
  };
  const camera = new Ptp(device);
  try {
    await camera.open();
    const files = await camera.capture();
    assert.deepEqual(files.map(file => file.extension), ['jpg', 'cr3']);
    assert.ok(files.every(file => file.bytes.length === 3));
  } finally { await camera.close(); }
  assert.equal(operations.filter(op => op === 0x100e).length, 1);
  assert.equal(operations.at(-1), 0x1003);
  assert.equal(operations.includes(0x100b), false);
  assert.equal(closed, true);
});
