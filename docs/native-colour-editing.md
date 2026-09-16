# Native CMYK and Lab editing

PSD/PSB imports retain their native channel samples at 8, 16, or 32 bits
per channel. CMYK tiles hold C, M, Y, K **and separate alpha**. Lab tiles
hold D50 L*, a*, b* and alpha. RGB display pixels are derived from those
samples; they are not the editable source of truth.

In the Color panel, use **Channels** to choose an ink or Lab component.
The value slider controls what Brush/Pencil paint into that channel;
**Fill** fills that channel on the active raster layer through the selection.
These edits preserve the other colour components and transparency. The
CMYK/Lab entry returns to composite painting. CMYK and L* use 0–100;
a* and b* use −128–127. Clone and History Brush also respect the selected
channel. Eraser edits transparency independently.

## Storage, history and files

- `NativePixel` describes channel meanings independently of RGBA. CMYK
  values are ink coverage (0 means no ink). Lab samples are encoded as
  L*/100 and (a*+128)/255, (b*+128)/255. PSD's inverted CMYK encoding is
  handled only by the codec.
- `TileBuf::Native` stores interleaved native samples at the document's
  depth. Existing RGB accessors explicitly convert for RGB consumers;
  identity and alpha-only writes preserve the original colour samples.
- COW snapshots, undo/redo, cancelled edits, translations, resampling and
  mode conversion carry the native samples. Filter previews retain tile
  snapshots so cancelling restores the exact original channels.
- PSD layers and the merged image write native planes directly. Invisible
  colour samples under zero alpha are retained in layer storage and export.
  The [PSD file-format specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
  defines the channel IDs, depths and section layout used by the codec.
- Shared-document/recovery tiles have an explicit native discriminator,
  mode and depth. Existing RGB encodings still read unchanged. Older
  clients that do not understand native tiles reject that encoding.

## Compositing and processing boundaries

Normal compositing, masks, opacity, groups, clipping stacks, Merge Down,
Merge Visible and Flatten retain native channels. CMYK separable blend
functions operate on the ink complements, including K. Native documents
use the CPU compositor; RGBA GPU shaders are not used for their layer
compositing.

Embedded native ICC profiles are used for the final conversion to sRGB
and the in-process filter adapter where the CMS can evaluate them. Alpha
never enters the colour transform. Missing, invalid or unsupported
profiles use the simple CMYK conversion or D50 Lab conversion. Source ICC
bytes survive editing and PSD save/reopen. Changing mode converts samples
and profile metadata in one undo entry; assigning an incompatible RGB
profile to native samples is rejected.

The plugin API provides `NativeFilterBuffer` and
`FilterPlugin::apply_native_with`. Native filters can change individual
components without RGB. Existing filters use the default RGB adapter:
only changed colours are converted back, while identity and alpha-only
results retain the source channels. Selection blending happens in native
samples. Desktop and MCP filter hosts use this contract.

Some operations intentionally remain RGB processing boundaries: existing
RGB filters/adjustments, retouch algorithms other than clone/history,
layer effects, Lab non-Normal blends and nonseparable blend modes. Their
changed pixels can be re-separated or gamut-clipped; they are not
independent-ink operations. External/WASM filter ABIs, clipboard images,
RGB-only importers and RGB-only export formats also remain RGB. Affinity
import/export has not been upgraded to native channel interchange.
These boundaries do not overwrite the source tiles merely to display or
save a native PSD.

## Verification

`make check-native-color` tests independent inks that render as the same
RGB, native channel edits and alpha, COW/undo/redo/cancel, translation and
resampling, RGB identity filters, native plugin dispatch, ICC Lab conversion,
recovery serialization, and PSD/PSB save/reopen. Codec fixtures are built
independently of the writer and cover 8/16/32-bit data plus raw, RLE, ZIP
and ZIP-with-prediction input.

`make check-native-color-app` checks application integration;
`make lint-native-color` runs the relevant lint checks.
