// Independently inspect Schist exports with ag-psd 31.0.x (MIT):
// NODE_PATH=/path/to/node_modules node validate-native-smart-filters.cjs /tmp/exports
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { readPsd } = require('ag-psd');
for (const ext of ['psd', 'psb']) {
  const doc = readPsd(fs.readFileSync(path.join(process.argv[2], `native-filters.${ext}`)), {
    skipLayerImageData: true, skipCompositeImageData: true, skipThumbnail: true,
    throwForMissingFeatures: true,
  });
  const layer = doc.children[0];
  assert.equal(layer.placedLayer.type, 'raster');
  assert.deepEqual(layer.placedLayer.transform, [0, 0, 4, 0, 4, 3, 0, 3]);
  assert.equal(layer.placedLayer.filter.list[0].type, 'gaussian blur');
  assert.deepEqual(layer.placedLayer.filter.list[0].filter.radius, { units: 'Pixels', value: 2.75 });
  assert.equal(layer.placedLayer.filter.list[0].enabled, true);
  assert.equal(layer.placedLayer.filter.list[0].opacity, 1);
  assert.equal(layer.placedLayer.filter.maskEnabled, false);
  const linked = doc.linkedFiles.find(file => file.id === layer.placedLayer.id);
  assert.ok(linked && linked.data, 'placed object resolves to embedded source bytes');
  const source = readPsd(linked.data, { useImageData: true, skipCompositeImageData: true });
  assert.equal(source.width, 4);
  assert.equal(source.height, 3);
  assert.equal(source.bitsPerChannel, 16);
  const rgba = source.children[0].imageData.data;
  assert.ok(rgba instanceof Uint16Array);
  assert.ok(Math.abs(rgba[0] / 65535 - 0.12345) < 0.0001);
  assert.ok(Math.abs(rgba[1] / 65535 - 0.45678) < 0.0001);
  assert.equal(rgba[3], 65535);
  console.log(`ag-psd validated native filter recipe and embedded source: ${ext}`);
}
for (const kind of ['smart', 'raster']) {
  const doc = readPsd(fs.readFileSync(path.join(process.argv[2], `${kind}-affine-filters.psd`)), {
    skipLayerImageData: true, skipCompositeImageData: true, skipThumbnail: true,
  });
  assert.deepEqual(doc.children[0].placedLayer.transform, [7, -3, 11, -1, 10.25, 5, 6.25, 3]);
  assert.equal(doc.children[0].placedLayer.width, 4);
  assert.equal(doc.children[0].placedLayer.height, 3);
  assert.equal(doc.children[0].placedLayer.filter.list[0].filter.radius.value, 2.75);
  console.log(`ag-psd validated ${kind} affine placement with unchanged source and radius`);
}
const more = readPsd(fs.readFileSync(path.join(process.argv[2], 'more-native-filters.psd')), {
  skipLayerImageData: true, skipCompositeImageData: true, skipThumbnail: true,
});
const list = more.children[0].placedLayer.filter.list;
assert.deepEqual(list.map(f => f.type), ['unsharp mask', 'high pass', 'median', 'motion blur', 'gaussian blur']);
assert.equal(list[0].filter.amount, 1.75);
assert.equal(list[0].filter.radius.value, 1);
assert.equal(list[1].filter.radius.value, 4.5);
assert.equal(list[2].filter.radius.value, 2);
assert.equal(list[3].filter.distance.value, 12);
assert.equal(list[3].filter.angle, -30);
console.log('ag-psd validated adjustable Sharpen, High Pass, Median, and Motion Blur');
