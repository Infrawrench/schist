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
  assert.deepEqual(text.styleRuns.map(run => run.length), [4, 7]); // UTF-16 lengths.
  // ag-psd lifts shared properties into text.style. Resolve inheritance before
  // checking runs, including when DejaVu Sans is unavailable on the host.
  const [regular, bold] = text.styleRuns.map(run => ({...text.style, ...run.style}));
  assert.ok(['DejaVuSans', 'DejaVu Sans'].includes(regular.font?.name));
  assert.equal(regular.fauxBold, false);
  if (bold.font?.name === 'DejaVuSans-Bold') {
    assert.equal(bold.fauxBold, false);
  } else {
    // Without a bold face, the writer retains the regular/family name and
    // uses FauxBold. Merely retaining the font without bold styling must fail.
    assert.equal(bold.font?.name, regular.font.name);
    assert.equal(bold.fauxBold, true);
  }
  assert.equal(text.style.tracking, 100);
  assert.equal(text.style.ligatures, false); // Untouched Schist text uses the unligated layout.
  assert.equal(text.transform[0], 1);
  assert.equal(text.transform[3], 1);
  console.log(`${name}: independent ag-psd text, font runs, color, paragraph and transform verified`);
}
