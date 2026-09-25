# Named workspaces

Use **View → Workspaces** to switch to Painting, Photo Development or
Retouching, or to a layout you saved. These entries also appear in Spotlight
(**Ctrl/Cmd+Shift+P**) under Actions, including when no document is open.
The submenu also offers Save As, Update, Rename, Delete and Reset commands;
each opens the manager with that operation as its Enter action, so its target
and effect can be reviewed first.
The starter layouts emphasize color, navigator/history, and layers/history,
respectively; they do not select tools or modify image processing settings.

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
Reorder panels using their headers and resize them using their lower edges in
the editor, then save or update a preset. The color panel can show Info or
Character depending on the document/tool; that contextual tab choice is not
part of a preset. The Notes panel still needs notes in the current document.

Keyboard controls in the manager: **Alt+Up/Down** selects a saved layout;
**Ctrl/Cmd+N** focuses the name field; **Ctrl/Cmd+S** saves a new copy;
**Ctrl/Cmd+Enter** applies; **Ctrl/Cmd+U** updates; **Ctrl/Cmd+R** renames;
**Ctrl/Cmd+D** deletes; **Ctrl/Cmd+0** resets. **Escape** closes the dialog.
Layout changes are immediate, including when the dialog is closed with Escape.

A preset contains panel order, individual visibility, saved heights, optional
width, dock visibility and editor AI sidebar visibility. It does not capture
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
