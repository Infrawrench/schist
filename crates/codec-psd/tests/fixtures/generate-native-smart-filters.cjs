// Reproduce the independently authored fixture with ag-psd 31.0.0 (MIT):
// NODE_PATH=/path/to/node_modules node generate-native-smart-filters.cjs
const fs = require('node:fs');
const path = require('node:path');
const { writePsdBuffer } = require('ag-psd');
const width = 3, height = 2;
const sourceImage = { width, height, data: new Uint8ClampedArray([
  255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255,
  64, 128, 192, 255, 0, 0, 0, 0, 255, 255, 255, 128,
]) };
const source = writePsdBuffer({ width, height, imageData: sourceImage,
  children: [{ name: 'independent source', imageData: sourceImage }] });
const id = 'bb89d850-c2e6-4a24-82b4-3e9de1837b08';
const base = { opacity: 1, blendMode: 'normal', enabled: true, hasOptions: true,
  foregroundColor: { r: 0, g: 0, b: 0 }, backgroundColor: { r: 255, g: 255, b: 255 } };
const pixels = { width, height, data: new Uint8ClampedArray(width * height * 4).fill(180) };
const placedLayer = { id, type: 'raster', width, height,
  transform: [2, 1, 5, 1, 5, 3, 2, 3],
  warp: { style: 'none', rotate: 'horizontal', bounds: {
    top: { units: 'Pixels', value: 0 }, left: { units: 'Pixels', value: 0 },
    right: { units: 'Pixels', value: width }, bottom: { units: 'Pixels', value: height },
  } },
  filter: { enabled: true, validAtPosition: true, maskEnabled: false,
    maskLinked: false, maskExtendWithWhite: true, list: [
      { ...base, name: 'High Pass', type: 'high pass',
        filter: { radius: { units: 'Pixels', value: 4.5 } } },
      { ...base, name: 'Median', type: 'median',
        filter: { radius: { units: 'Pixels', value: 2 } } },
      { ...base, name: 'Motion Blur', type: 'motion blur',
        filter: { distance: { units: 'Pixels', value: 12 }, angle: -30 } },
      { ...base, name: 'Unsharp Mask', type: 'unsharp mask', enabled: false,
        filter: { radius: { units: 'Pixels', value: 1.5 }, amount: 1.25, threshold: 7 } },
      { ...base, name: 'Gaussian Blur', type: 'gaussian blur',
        filter: { radius: { units: 'Pixels', value: 2.25 } } },
    ] },
};
const psd = writePsdBuffer({ width: 7, height: 5,
  children: [{ name: 'Independent smart filters', left: 2, top: 1,
    imageData: pixels, placedLayer }],
  linkedFiles: [{ id, name: 'independent-source.psd', type: '8BPS', data: source }],
});
fs.writeFileSync(path.join(__dirname, 'native-smart-filters-ag-psd.psd'), psd);
