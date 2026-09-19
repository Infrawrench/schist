// Generate fixtures with an independent writer; no Schist codec is involved.
// npm install --prefix /tmp/schist-psd-independent --ignore-scripts ag-psd@31.0.2
// node scripts/psd-text-fixtures.cjs /tmp/schist-psd-independent/node_modules/ag-psd
const fs = require('node:fs');
const path = require('node:path');
const psd = require(process.argv[2] || 'ag-psd');
psd.initializeCanvas(() => { throw new Error('Canvas is unnecessary for these metadata fixtures'); }, undefined,
  (width, height) => ({ width, height, data: new Uint8ClampedArray(width * height * 4) }));
const output = path.join(__dirname, '../crates/codec-psd/tests/fixtures');
const style = { font: { name: 'DejaVuSans' }, fontSize: 20, fillColor: { r: 51, g: 102, b: 153 }, tracking: 100 };
const content = 'A😀 Bé\nNext';
const text = {
  text: content, transform: [1, 0, 0, 1, 12, 40], orientation: 'horizontal',
  style, paragraphStyle: { justification: 'center', autoHyphenate: false },
  styleRuns: [{ length: 4, style }, { length: content.length - 4, style: { ...style, font: { name: 'DejaVuSans-Bold' }, fauxBold: true } }],
};
for (const rotated of [false, true]) {
  const doc = { width: 128, height: 96, children: [{ name: 'Independent native type',
    imageData: { width: 1, height: 1, data: new Uint8ClampedArray([51,102,153,255]) },
    text: rotated ? { ...text, transform: [0, 1, -1, 0, 12, 40] } : text,
  }] };
  fs.writeFileSync(path.join(output, rotated ? 'ag-psd-type-rotated.psd' : 'ag-psd-type.psd'), Buffer.from(psd.writePsd(doc)));
}
for (const [name, extra] of [
  ['ag-psd-type-auto-leading.psd', { paragraphStyle: { justification: 'center', autoLeading: 2.4, autoHyphenate: false } }],
  ['ag-psd-type-overset.psd', { shapeType: 'box', boxBounds: [0, 0, 100, 5] }],
]) {
  const doc = { width: 128, height: 96, children: [{ name,
    imageData: { width: 1, height: 1, data: new Uint8ClampedArray([51,102,153,255]) }, text: { ...text, ...extra },
  }] };
  fs.writeFileSync(path.join(output, name), Buffer.from(psd.writePsd(doc)));
}
