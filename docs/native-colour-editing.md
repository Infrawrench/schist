# Native CMYK and Lab editing: unresolved README item

The editor still edits RGB pixels in CMYK and Lab documents. This item
cannot be completed as an isolated channel-panel or colour-conversion
change: the current storage and editing contracts do not retain the native
channels needed for it.

The constraints in the current implementation are concrete:

- `crates/core/src/tile.rs` stores four interleaved RGBA components in
  `TileBuf`, and its pixel API returns `Rgba`. CMYK with transparency needs
  five independent components. Lab also needs its own channel meanings
  and ranges, rather than RGB components bearing different labels.
- `crates/codec-psd/src/reader/layers.rs` converts CMYK/Lab planes to RGB
  before populating the editable tiles. The writer converts edited RGB
  back into the requested file mode. Different CMYK separations can
  produce the same RGB colour, so that conversion cannot recover the
  original individual inks.
- Painting, adjustments, filters, blending, GPU shaders, plugin buffers
  and display transforms consume RGB/RGBA. Reinterpreting existing tiles
  as native channels would change their behaviour throughout the editor.

A correct implementation needs native channel storage and colour-space
metadata, matching edit/undo and serialization support, and an explicit
boundary for tools and plugins that operate in RGB. Native channels must
remain authoritative through import, edits, compositing and export;
rendered RGB is a display or processing representation. It also needs
channel-selection UI and regression fixtures for independent CMYK ink
edits, Lab channel edits, alpha, profiles, undo and save/reopen at each
supported depth.

This migration has not been implemented. The README retains the limitation
instead of claiming that RGB-derived channel controls provide native edits.
