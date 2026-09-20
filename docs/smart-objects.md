# Editable and linked smart objects

The Layer menu provides **Place Embedded Smart Object**, **Place Linked Smart
Object**, **Edit Contents**, **Replace Contents**, **Relink Smart Object**, and
**Update Linked Instances**. PSD/PSB and raster formats whose dimensions can be
read before decoding are supported. Source files and embedded PSDs are limited to
64 MiB; the source canvas is limited to 32 million pixels and 30,000 pixels per
side. A source rejected by these limits leaves the current document unchanged.

Embedded objects retain a layered PSD source. Edit Contents opens that document
in a regular tab: layers, text, painting, and the other editing tools work there.
Save updates every instance with the same source identity in the parent document,
closes the contents tab and returns to the parent. Duplicate Layer creates another
instance sharing the source. Existing raster smart objects acquire an editable
source when first opened. Source origins, including negative coordinates, retain
their original placement. Up to 16 contents tabs can be open, including nested
sources. Saving a nested source updates its immediate parent; save that parent to
propagate the next level.

Every instance keeps its placement transform, mask, opacity, blending and layer
style. Filter stacks are rerun against the new unfiltered source. If an instance
is locked or a filter cannot render, the entire update is refused. Replace
Contents embeds a replacement and detaches any existing file link; Relink binds
the source family to another file. Replacement retains each instance’s scale, rotation and source anchor, adjusting
for a changed source origin. Artwork of different dimensions can change the
displayed size.

Linked objects retain an embedded snapshot so missing files never blank the
canvas. The Layers panel shows the source path, missing links and changes in file
size/modification time. Update Linked Instances explicitly reloads the file and
updates every instance referring to that source identity or path **in the current
document**. Other open documents can be updated with the same command. Files are
never read automatically merely because a document was opened. Use Relink after
moving an asset. Links use absolute native paths; moving a project to another
computer may require relinking.

Edit Contents on a linked object opens its current file. Save writes that file
using its existing format and updates instances in the parent. It refuses to
replace a linked file that changed after editing began. To keep conflicting edits,
use Save As and then Relink. Use PSD/PSB source files when source layers must remain
editable in the external file: raster exports flatten them. Undo/Redo restore the
parent's cached pixels, editable source and link metadata together; they do not
rewind external source files.

RGB profile conversions happen before placement. Native CMYK/Lab artwork retains
native samples when the parent uses the same mode/profile; conversion is explicit
when they differ. Sources keep float precision independently of the parent depth.

PSD saves, recovery snapshots, and shared-document checkpoints retain nested
sources, source identities, and links. A recovered contents tab is an independent
document: save it separately and use Replace Contents or Relink to apply it. If
the parent is closed during editing, the contents tab becomes an independent
document rather than discarding its edits. Only one contents editor per source
family opens in each parent, and stale sources are refused on Save.

The browser supports embedded placement, replacement and contents editing through
its file picker. It can display linked snapshots, but filesystem link updates and
linked editing require the desktop app. A browser document can use Replace
Contents to turn a linked snapshot into an embedded source.

Schist stores source documents in a versioned `ScSd` private layer block, bounded
before parsing. Nested PSDs are decoded only on Edit Contents; no recursive source
loading occurs during document import. The `ScSo` v3 rendered-source payload uses
bounded sparse native tiles; v1/v2 remain readable. Other PSD readers display the
rendered pixels and may ignore Schist's editable source. This does not author
Photoshop's proprietary linked-object graph.

The 32 smart-object interface strings use `schist-i18n` and are translated across
all 150 shipped locale catalogs. Generic Norwegian and Serbo-Croatian catalogs
are generated from Norwegian Bokmål and Croatian respectively.
