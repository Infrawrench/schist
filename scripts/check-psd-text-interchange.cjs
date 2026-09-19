// Independently inspect Schist outputs with ag-psd; this does not call Schist's reader.
// SCHIST_INTERCHANGE_ARTIFACT_DIR=/tmp/schist-interchange make check-psd-interchange
// node scripts/check-psd-text-interchange.cjs /tmp/schist-psd-independent/node_modules/ag-psd /tmp/schist-interchange
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const psd = require(process.argv[2] || 'ag-psd');
for (const name of ['native-text.psd', 'native-text.psb']) {
  const doc = psd.readPsd(fs.readFileSync(path.join(process.argv[3], name)), {
    skipLayerImageData: true, skipCompositeImageData: true, skipThumbnail: true,
  });
  const text = doc.children[0].text;
  assert.equal(text.text, 'A😀 Bé\nNext');
  assert.equal(text.orientation, 'horizontal');
  assert.equal(text.paragraphStyle.justification, 'center');
  assert.deepEqual(text.style.fillColor, {r:51, g:102, b:153});
  assert.equal(text.styleRuns[0].length, 4); // UTF-16, not UTF-8 byte length.
  assert.equal(text.styleRuns[1].style.font.name, 'DejaVuSans-Bold');
  assert.equal(text.style.tracking, 100);
  assert.equal(text.style.ligatures, false); // Untouched Schist text uses the unligated layout.
  assert.equal(text.transform[0], 1);
  assert.equal(text.transform[3], 1);
  console.log(`${name}: independent ag-psd text, font runs, color, paragraph and transform verified`);
}
