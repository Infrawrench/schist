# Named workspaces

Use **View → Workspaces** to switch to Painting, Photo Development or
Retouching, to Design, or to a layout you saved. These entries also appear in Spotlight
(**Ctrl/Cmd+Shift+P**) under Actions, including when no document is open.
The submenu also offers Save As, Update, Rename, Delete and Reset commands;
each opens the manager with that operation as its Enter action, so its target
and effect can be reviewed first.
Painting emphasizes color, and Retouching emphasizes layers and history.
Photo Development uses an Aperture-inspired layout: a left inspector with
Photos, Info and Adjustments tabs, a charcoal viewer, and a bottom filmstrip
of open documents. Grid, Photos + Preview and Preview buttons switch between
the browser, split view and viewer layouts.
Click a thumbnail to edit that document; double-click a browser thumbnail to
return to the split view. Middle-click closes it through the normal save prompt.
The inspector provides an RGB thumbnail histogram, adjustment-layer commands,
the existing layers/history controls, and photo metadata. Adjustments open
the existing parameter dialogs. The tool-options bar starts hidden in Photo
Development. Choosing an editing tool reveals its controls; Hand and Zoom
hide them again, and Grid keeps them hidden.

In Photos, **Buckets** lists the gallery's existing local and Schist Cloud
buckets. Smart buckets keep their rules and star marker. Clicking a bucket
opens that bucket in the gallery, where its membership and rules are managed;
opening a photo for editing returns to the photo workspace.
**Open Documents** lists the same open editor tabs as the filmstrip, in the
same order. Each entry is one editable document with its own layers, history
and unsaved state. Opening a bucket does not open all its photos as documents.
Closing a document does not remove its photo from a bucket. Gallery photos
retain their original-file association and save edits to their existing
sidecar; ordinary opened files retain the normal save behavior.

The Design starter is listed only when the `design-mode` feature is on, and
choosing it switches the mode as well as the dock, because a layout with a
Pages panel in a photo editor is a blank dock section.
The starters do not select tools or modify image processing settings.

Open **Manage Workspaces…** to edit the current dock and save it:

- **Save As…** saves a new named copy of the current layout.
- Select a saved name, then **Apply** to switch to it. Selecting its name alone
  does not change the dock, so **Update** can replace that saved layout with
  the current dock arrangement. If the live layout no longer matches a saved
  one, choose the update target explicitly; Schist does not guess which preset
  should be overwritten.
- **Rename** changes the selected preset's name to the text in the name field.
- **Delete** removes the selected saved preset; the current dock stays as it is.
- **Reset** restores the current dock to the application's default layout.
  It leaves all saved presets, documents, and unrelated preferences intact.
  Reapply a saved preset to discard unsaved changes to that layout.

The manager also controls dock visibility, individual panels and dock width.
Photo Development keeps its inspector tabs fixed; its Layers and History
sections can be hidden in the manager. In Painting and Retouching, reorder
panels using their headers and resize them using their lower edges, then save
or update a preset. The color panel can show Info or
Character depending on the document/tool; that contextual tab choice is not
part of a preset. The Notes panel still needs notes in the current document.

Keyboard controls in the manager: **Alt+Up/Down** selects a saved layout;
**Ctrl/Cmd+N** focuses the name field; **Ctrl/Cmd+S** saves a new copy;
**Ctrl/Cmd+Enter** applies; **Ctrl/Cmd+U** updates; **Ctrl/Cmd+R** renames;
**Ctrl/Cmd+D** deletes; **Ctrl/Cmd+0** resets. **Escape** closes the dialog.
Layout changes are immediate, including when the dialog is closed with Escape.

A preset contains panel order, individual visibility, saved heights, optional
width, dock visibility, editor AI sidebar visibility, and the photo workspace's
inspector tab and browser/split/viewer choice. It does not capture
theme, telemetry, update preferences, author information, AI credentials or
models, gallery settings, canvas overlays, documents, or undo history.
On compact windows the existing panel/canvas page toggle remains in charge;
the saved dock width applies when the window has room for a side column.
The browser has no agent sidebar; the stored AI visibility is harmless there.

Existing preferences migrate without replacing the current arrangement.
Unknown panel IDs and duplicate IDs are discarded; missing panels are appended
in default order. Heights are limited to 60–1200 pixels and custom width to
180–600 pixels. Names are trimmed, must contain 1–80 characters without control
characters, and are unique ignoring letter case. Up to 64 presets can be saved.
Malformed saved presets are skipped without discarding unrelated preferences.

Native saves stage and sync a new file, then atomically replace
`preferences.json`; Unix also syncs its containing directory. Browser saves use
one localStorage transaction. A failed save reports an error and does not
commit the change to the live layout or saved preset list. Browser storage is
local to the browser/profile and can be cleared by its settings. Simultaneous
application instances retain the existing last-writer-wins preference behavior.

Workspace strings are available in all 150 supported locales, with English
source and AI translations for the other 149 locales; human review is pending.
Norwegian and Serbo-Croatian follow the shared Bokmål and Croatian catalogs.
Existing common button/panel labels stay localized.

## Photo Development screenshots

The Photos inspector separates Buckets from Open Documents. The tool-options bar starts hidden:

![Photo Development showing Buckets and Open Documents](https://agent-assets.infrawrench.com/schist/photo-workspace/buckets-20261003-ee3fa102/photos.png)

The light theme with the Adjustments inspector, charcoal viewer and document filmstrip:

![Photo Development with the Adjustments inspector and filmstrip](https://agent-assets.infrawrench.com/schist/photo-workspace/albums-20261003-b62fe5e3/split-view.png)

The Info inspector shows the document preview, zoom and image dimensions:

![Photo Development with the Info inspector](https://agent-assets.infrawrench.com/schist/photo-workspace/albums-20261003-b62fe5e3/info-inspector.png)

Choosing Exposure opens the existing adjustment controls:

![Exposure adjustment controls in Photo Development](https://agent-assets.infrawrench.com/schist/photo-workspace/albums-20261003-b62fe5e3/exposure-controls.png)
