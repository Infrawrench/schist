const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { Schist } = require('../../dist/library/node/schist.js');
const call = (app, value) => JSON.parse(app.request(JSON.stringify(value)));
const a = new Schist();
const b = new Schist();
try {
  assert.deepEqual(call(a, { op: 'create', width: 8, height: 8 }), { session: 1 });
  assert.deepEqual(call(b, { op: 'sessions' }), []);
  call(a, { op: 'call', session: 1, name: 'adjust_invert' });
  const png = a.exportFile(1, 'png', 8);
  assert.equal(Buffer.from(png).subarray(1, 4).toString(), 'PNG');
  assert.equal(b.importFile('image.png', png), 1);
  for (const [id, file] of [['face', 'detector'], ['face-embed', 'recogniser']]) {
    a.loadModel(id, fs.readFileSync(path.join(__dirname, `../../crates/library/tests/fixtures/${file}.onnx`)));
  }
  const request = { op: 'people', width: 320, height: 240, rgb: Array(320 * 240 * 3).fill(128) };
  const faces = call(a, request);
  assert.equal(faces.length, 1);
  assert.equal(faces[0].embedding.length, 128);
  assert.throws(() => call(b, request));
  console.log('WASM editing and People smoke passed');
} finally {
  a.free();
  b.free();
}
