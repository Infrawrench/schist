//! Type tool (T): editable text layers.
//!
//! A text layer is a raster layer plus a `PsTx` block in its preserved-PSD
//! extras holding the JSON [`TextSpec`] it was rendered from, character
//! runs and all: select a word and pick a font, and only that word
//! changes. That block
//! rides through save/load untouched (the PSD writer re-emits unknown
//! blocks verbatim), so text stays re-editable across sessions while
//! Photoshop still sees ordinary pixels.

use schist_color::Rgba;
use schist_core::{
    Document, IntRect, Layer, LayerId, LayerPath, RawBlock, TileCoord, TileMap, TILE_SIZE,
};
use schist_i18n::{choices, t};
use schist_plugin_api::{
    EditorState, Modifiers, OptionValue, Overlay, PluginManifest, PluginRegistry, PointerInput,
    ToolCtx, ToolOption, ToolPlugin,
};
use schist_text_engine::{
    line_spans, rasterize, Align, CaretAffinity, CaretMovement, CaretPosition, ParagraphDirection,
    StyleRun, TextPath, TextSpec, WritingMode,
};

/// Additional-layer-info key under which the text spec is preserved.
pub const TEXT_BLOCK_KEY: [u8; 4] = *b"PsTx";

/// What a text layer stores alongside the spec.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredText {
    spec: TextSpec,
    /// Document-space origin of the layout box.
    origin: (i32, i32),
    /// Fill colour as RGBA bytes.
    color: [u8; 4],
}

fn read_stored(layer: &Layer) -> Option<StoredText> {
    let block = layer.extras.iter().find(|b| b.key == TEXT_BLOCK_KEY)?;
    match serde_json::from_slice(&block.data) {
        Ok(v) => Some(v),
        Err(err) => {
            log::warn!("text layer {:?} has unreadable spec: {err}", layer.name);
            None
        }
    }
}

fn write_stored(layer: &mut Layer, stored: &StoredText) {
    let data = match serde_json::to_vec(stored) {
        Ok(d) => d,
        Err(err) => {
            log::error!("cannot serialize text spec: {err}");
            return;
        }
    };
    layer.extras.retain(|b| b.key != TEXT_BLOCK_KEY);
    layer.extras.push(RawBlock {
        key: TEXT_BLOCK_KEY,
        data,
    });
}

/// Render a text spec into a fresh tile map at `origin`.
fn render_tiles(doc: &Document, stored: &StoredText) -> (TileMap, IntRect) {
    let mut tiles = TileMap::new();
    let Some(raster) = rasterize(&stored.spec) else {
        return (tiles, IntRect::EMPTY);
    };
    if raster.is_empty() {
        return (tiles, IntRect::EMPTY);
    }
    let bounds = raster.bounds.translated(stored.origin.0, stored.origin.1);
    let w = raster.bounds.width() as usize;
    let color = Rgba::from_u8(
        stored.color[0],
        stored.color[1],
        stored.color[2],
        stored.color[3],
    );
    let depth = doc.depth;
    for coord in TileCoord::covering(&bounds) {
        let trect = coord.rect();
        let clip = trect.intersect(&bounds);
        if clip.is_empty() {
            continue;
        }
        let buf = tiles.get_mut_or_insert(coord, depth);
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let cov =
                    raster.coverage[(y - bounds.top) as usize * w + (x - bounds.left) as usize];
                if cov == 0 {
                    continue;
                }
                let ix = ((y - trect.top) * TILE_SIZE + (x - trect.left)) as usize;
                buf.set(
                    ix,
                    Rgba {
                        a: color.a * (cov as f32 / 255.0),
                        ..color
                    },
                );
            }
        }
    }
    tiles.prune_blank();
    (tiles, bounds)
}

/// The typographic box occupied by all of `stored`'s laid-out lines.
///
/// Raster bounds only cover non-transparent glyph pixels: a lowercase
/// word therefore starts below the font's ascender and may stop above its
/// descender. Editing chrome belongs to the complete line box instead, so
/// the insertion caret cannot protrude through its outline.
fn layout_bounds(stored: &StoredText) -> IntRect {
    if stored.spec.path.is_some() || stored.spec.writing_mode.is_vertical() {
        return schist_text_engine::carets(&stored.spec)
            .into_iter()
            .fold(IntRect::EMPTY, |bounds, (_, c)| {
                bounds.union(&caret_rect(c))
            })
            .translated(stored.origin.0, stored.origin.1);
    }
    let mut spans = line_spans(&stored.spec).into_iter();
    let Some(first) = spans.next() else {
        return IntRect::EMPTY;
    };
    let first_end = first.x + first.width;
    let (mut left, mut right) = (first.x.min(first_end), first.x.max(first_end));
    let (mut top, mut bottom) = (first.top, first.top + first.height);
    for span in spans {
        let end = span.x + span.width;
        left = left.min(span.x.min(end));
        right = right.max(span.x.max(end));
        top = top.min(span.top);
        bottom = bottom.max(span.top + span.height);
    }
    let ox = stored.origin.0 as f32;
    let oy = stored.origin.1 as f32;
    let left = (ox + left).floor() as i32;
    let mut right = (ox + right).ceil() as i32;
    let top = (oy + top).floor() as i32;
    let mut bottom = (oy + bottom).ceil() as i32;
    // Keep whitespace-only lines representable as rectangles too.
    right = right.max(left + 1);
    bottom = bottom.max(top + 1);
    IntRect::new(left, top, right, bottom)
}

fn caret_rect(c: schist_text_engine::Caret) -> IntRect {
    let end_x = c.x - c.angle.sin() * c.height;
    let end_y = c.top + c.angle.cos() * c.height;
    IntRect::new(
        c.x.min(end_x).floor() as i32,
        c.top.min(end_y).floor() as i32,
        c.x.max(end_x).ceil() as i32 + 1,
        c.top.max(end_y).ceil() as i32 + 1,
    )
}

/// Every font family the document's text layers ask for, in the order
/// first seen and without repeats.
pub fn families_used(doc: &Document) -> Vec<String> {
    fn walk(layers: &[Layer], out: &mut Vec<String>) {
        for layer in layers {
            if let Some(children) = layer.children() {
                walk(children, out);
            }
            let Some(stored) = read_stored(layer) else {
                continue;
            };
            for family in stored.spec.families() {
                let family = family.trim();
                if !family.is_empty() && !out.iter().any(|f| f == family) {
                    out.push(family.to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.tree.layers, &mut out);
    out
}

/// Re-set every text layer in `family` and repaint it.
///
/// A layer set in a font that was missing was rasterized in whatever the
/// engine substituted; once the real font arrives those pixels are stale
/// and only a re-render fixes them. Returns how many layers changed.
pub fn rerender_family(doc: &mut Document, family: &str) -> usize {
    fn collect(layers: &[Layer], family: &str, out: &mut Vec<schist_core::LayerId>) {
        for layer in layers {
            if let Some(children) = layer.children() {
                collect(children, family, out);
            }
            let uses = |s: &StoredText| {
                s.spec
                    .families()
                    .iter()
                    .any(|f| f.trim().eq_ignore_ascii_case(family))
            };
            if read_stored(layer).is_some_and(|s| uses(&s)) {
                out.push(layer.id);
            }
        }
    }
    let mut ids = Vec::new();
    collect(&doc.tree.layers, family, &mut ids);
    let mut changed = 0;
    for id in ids {
        let Some(stored) = doc.tree.find(id).and_then(read_stored) else {
            continue;
        };
        let before = doc
            .tree
            .find(id)
            .map(|l| l.content_bounds())
            .unwrap_or(IntRect::EMPTY);
        let (tiles, bounds) = render_tiles(doc, &stored);
        if let Some(layer) = doc.tree.find_mut(id) {
            if let Some(raster) = layer.as_raster_mut() {
                raster.tiles = tiles;
            }
            // The style cache was built from the old glyphs.
            layer.styled = None;
            changed += 1;
        }
        doc.add_damage(before.union(&bounds));
    }
    changed
}

/// An editing session over one text layer.
struct Editing {
    layer: LayerId,
    stored: StoredText,
    /// Pixels before this session, for undo capture on commit.
    original: TileMap,
    /// The layer's preserved blocks and name before this session, so the
    /// commit can record them changing alongside the pixels and a
    /// cancel can put them back.
    original_extras: Vec<RawBlock>,
    original_name: String,
    /// True once the layer was created by this session (so cancelling
    /// removes it entirely).
    created: bool,
    dirty: bool,
    /// Byte offset of the caret in `stored.spec.text`.
    caret: usize,
    affinity: CaretAffinity,
    /// The other end of the selection. Equal to `caret` when nothing is
    /// selected, so the two together describe both states.
    anchor: usize,
}

static STYLES: &[&str] = &[
    "tool.type.choice.regular",
    "tool.type.choice.bold",
    "tool.type.choice.italic",
    "tool.type.choice.bold_italic",
];
static ALIGNMENTS: &[&str] = &["common.left", "common.center", "common.right"];
static VERTICAL_ALIGNMENTS: &[&str] = &["common.top", "common.center", "common.bottom"];

#[derive(Default)]
pub struct TypeTool {
    editing: Option<Editing>,
    /// A canvas drag is extending the editing session's selection.
    selecting: bool,
    /// What new text starts as, and what the options bar shows when
    /// nothing is being edited. Editing a layer adopts its spec, so the
    /// bar always describes the text you are looking at.
    spec: TextSpec,
    use_path: bool,
}

impl TypeTool {
    /// Live-render the session's text into its layer without touching
    /// history (the whole session commits as one edit).
    fn refresh(&mut self, doc: &mut Document) {
        let Some(session) = &mut self.editing else {
            return;
        };
        let (tiles, bounds) = render_tiles(doc, &session.stored);
        let before = doc
            .tree
            .find(session.layer)
            .map(|l| l.content_bounds())
            .unwrap_or(IntRect::EMPTY);
        if let Some(layer) = doc.tree.find_mut(session.layer) {
            if let Some(raster) = layer.as_raster_mut() {
                raster.tiles = tiles;
            }
            layer.name = display_name(&session.stored.spec.text);
            write_stored(layer, &session.stored);
        }
        doc.add_damage(before.union(&bounds));
    }

    fn start_new(&mut self, ctx: &mut ToolCtx, x: f32, y: f32) {
        let mut stored = StoredText {
            spec: TextSpec {
                text: String::new(),
                path: None,
                ..self.spec.clone()
            },
            origin: (x.round() as i32, y.round() as i32),
            color: ctx.state.foreground.to_u8(),
        };
        if self.use_path {
            stored.spec.path = active_text_path(ctx.doc, stored.origin);
        }
        self.spec.path = stored.spec.path.clone();
        self.use_path = self.spec.path.is_some();
        let mut layer = Layer::new_raster(t("common.text"));
        write_stored(&mut layer, &stored);
        let id = layer.id;
        let original_extras = layer.extras.clone();
        let original_name = layer.name.clone();
        let path = match ctx.doc.active_layer.and_then(|a| ctx.doc.tree.path_of(a)) {
            Some(mut p) => {
                *p.0.last_mut().unwrap() += 1;
                p
            }
            None => LayerPath(vec![ctx.doc.tree.layers.len()]),
        };
        let mut edit = ctx.doc.begin_edit(t("tool.type.history.new_layer"));
        edit.insert_layer(path, layer);
        edit.commit();
        ctx.doc.active_layer = Some(id);
        self.editing = Some(Editing {
            layer: id,
            stored,
            original: TileMap::new(),
            original_extras,
            original_name,
            created: true,
            dirty: false,
            caret: 0,
            affinity: CaretAffinity::Downstream,
            anchor: 0,
        });
    }

    /// Keep the session's fill on the foreground swatch.
    ///
    /// The eyedropper and the colour panel both write the foreground, and
    /// text follows it the way a brush stroke would. Without this the
    /// colour was read once when the layer was created and never again,
    /// so picking a new colour and clicking back into the text changed
    /// nothing. Returns true when the fill actually changed, so callers
    /// know a re-render is due.
    fn adopt_foreground(&mut self, state: &EditorState) -> bool {
        let Some(session) = &mut self.editing else {
            return false;
        };
        let fg = state.foreground.to_u8();
        if session.stored.color == fg {
            return false;
        }
        session.stored.color = fg;
        session.dirty = true;
        true
    }

    /// Show the font at the caret in the options bar: the selection's
    /// first character, or the character before an insertion point, the
    /// way every editor's font menu follows the cursor.
    fn sync_bar(&mut self) {
        let Some(session) = &self.editing else {
            return;
        };
        let style = session.caret_style();
        self.spec.family = style.family;
        self.spec.bold = style.bold;
        self.spec.italic = style.italic;
        self.spec.size = style.size;
    }

    /// Whether a point is in the layout box of `stored`.
    ///
    /// Ink bounds omit spaces and empty lines, but both are valid places
    /// to put a text caret, so text hit-testing uses the layout rather than
    /// pixels alone.
    fn contains_text(stored: &StoredText, x: f32, y: f32) -> bool {
        const SLOP: f32 = 4.0;
        if stored.spec.path.is_some() || stored.spec.writing_mode.is_vertical() {
            return layout_bounds(stored)
                .inflated(SLOP as i32)
                .contains(x as i32, y as i32);
        }
        let x = x - stored.origin.0 as f32;
        let y = y - stored.origin.1 as f32;
        line_spans(&stored.spec).iter().any(|line| {
            x >= line.x - SLOP
                && x <= line.x + line.width + SLOP
                && y >= line.top - SLOP
                && y <= line.top + line.height + SLOP
        })
    }

    /// Move the current session's caret to a document-space point.
    fn point_caret(&mut self, x: f32, y: f32, extend: bool) {
        let Some(session) = &mut self.editing else {
            return;
        };
        let local_x = x - session.stored.origin.0 as f32;
        let local_y = y - session.stored.origin.1 as f32;
        let Some(position) =
            schist_text_engine::hit_test_position(&session.stored.spec, local_x, local_y)
        else {
            return;
        };
        session.caret = position.byte;
        session.affinity = position.affinity;
        if !extend {
            session.anchor = position.byte;
        }
        self.sync_bar();
    }

    /// Pick an existing text layer under the cursor, if any.
    fn text_layer_at(doc: &Document, x: f32, y: f32) -> Option<(LayerId, StoredText)> {
        let (px, py) = (x.round() as i32, y.round() as i32);
        let mut hit = None;
        for layer in doc.tree.iter() {
            let Some(stored) = read_stored(layer) else {
                continue;
            };
            if layer.tight_bounds().inflated(4).contains(px, py)
                || Self::contains_text(&stored, x, y)
            {
                hit = Some((layer.id, stored));
            }
        }
        hit
    }
}

fn display_name(text: &str) -> String {
    let first: String = text.lines().next().unwrap_or("").chars().take(24).collect();
    if first.trim().is_empty() {
        t("common.text").to_string()
    } else {
        first
    }
}

fn active_text_path(doc: &Document, origin: (i32, i32)) -> Option<TextPath> {
    // Match Path Selection: the active live shape takes precedence over
    // a separately stored path.
    let path = doc
        .active_layer
        .and_then(|id| doc.tree.find(id))
        .and_then(|layer| layer.shape.as_deref())
        .map(|shape| &shape.path)
        .or_else(|| doc.active_path.and_then(|i| doc.paths.get(i)))?;
    let mut curve = path.subpaths.iter().find(|s| s.anchors.len() >= 2)?.clone();
    for anchor in &mut curve.anchors {
        *anchor = anchor.translated(-(origin.0 as f32), -(origin.1 as f32));
    }
    Some(TextPath { curve, offset: 0.0 })
}

impl ToolPlugin for TypeTool {
    fn id(&self) -> &'static str {
        "type"
    }
    fn name(&self) -> &'static str {
        t("tool.type.name")
    }
    fn description(&self) -> &'static str {
        t("tool.type.description")
    }
    fn icon(&self) -> &'static str {
        "type"
    }
    fn shortcut(&self) -> Option<&'static str> {
        Some("t")
    }

    fn captures_keys(&self) -> bool {
        self.editing.is_some()
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.selecting = false;

        // A click in the text already being edited places the caret there;
        // dragging from it selects text. Previously every click committed
        // and reopened the layer with its caret at the end, so issue #99's
        // per-selection font controls were not reachable with a mouse.
        let in_current = self
            .editing
            .as_ref()
            .is_some_and(|session| Self::contains_text(&session.stored, input.x, input.y));
        if in_current {
            self.point_caret(input.x, input.y, input.modifiers.shift);
            self.selecting = true;
            return;
        }

        // Clicking away from the current text commits it first.
        if self.editing.is_some() {
            self.on_commit(ctx);
        }
        match Self::text_layer_at(ctx.doc, input.x, input.y) {
            Some((layer, stored)) => {
                let found = ctx.doc.tree.find(layer);
                let original = found
                    .and_then(|l| l.as_raster())
                    .map(|r| r.tiles.clone())
                    .unwrap_or_default();
                let original_extras = found.map(|l| l.extras.clone()).unwrap_or_default();
                let original_name = found.map(|l| l.name.clone()).unwrap_or_default();
                ctx.doc.active_layer = Some(layer);
                // Show this layer's own type settings in the bar. Its
                // runs stay with it: the bar describes one font at a
                // time, the one at the caret.
                self.spec = TextSpec {
                    text: String::new(),
                    runs: Vec::new(),
                    ..stored.spec.clone()
                };
                self.use_path = stored.spec.path.is_some();
                let local_x = input.x - stored.origin.0 as f32;
                let local_y = input.y - stored.origin.1 as f32;
                let position =
                    schist_text_engine::hit_test_position(&stored.spec, local_x, local_y)
                        .unwrap_or_else(|| stored.spec.text.len().into());
                self.editing = Some(Editing {
                    layer,
                    stored,
                    original,
                    original_extras,
                    original_name,
                    created: false,
                    dirty: false,
                    caret: position.byte,
                    affinity: position.affinity,
                    anchor: position.byte,
                });
                self.selecting = true;
                self.sync_bar();
                // A colour picked since this text was set applies to it now,
                // so the eyedropper works on text like on anything else.
                if self.adopt_foreground(ctx.state) {
                    self.refresh(ctx.doc);
                }
            }
            None => {
                self.start_new(ctx, input.x, input.y);
                self.selecting = true;
            }
        }
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        if self.selecting {
            self.point_caret(input.x, input.y, true);
        }
    }

    fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        if self.selecting {
            self.point_caret(input.x, input.y, true);
            self.selecting = false;
        }
    }

    fn on_key(
        &mut self,
        ctx: &mut ToolCtx,
        key: &str,
        text: Option<&str>,
        modifiers: Modifiers,
    ) -> bool {
        if self.editing.is_none() {
            return false;
        }
        self.selecting = false;
        // The colour panel can be clicked mid-session; the next keystroke
        // is where its choice reaches the text.
        let recolored = self.adopt_foreground(ctx.state);
        let shift = modifiers.shift;
        let word = modifiers.ctrl_or_cmd;
        let mut changed = false;
        let mut handled = true;
        {
            let Some(session) = &mut self.editing else {
                return false;
            };
            match key {
                // Ctrl+A selects the text being edited, not the canvas.
                "a" if modifiers.ctrl_or_cmd => {
                    session.anchor = 0;
                    session.caret = session.stored.spec.text.len();
                }
                "left" | "right" | "up" | "down" | "home" | "end" => {
                    let to = match key {
                        "left" if word => session.move_word_visual(false),
                        "right" if word => session.move_word_visual(true),
                        // An unshifted arrow with a selection collapses to
                        // its edge rather than stepping past it.
                        "left" if session.has_selection() && !shift => {
                            session.collapse_visual(false, false)
                        }
                        "right" if session.has_selection() && !shift => {
                            session.collapse_visual(true, false)
                        }
                        "left" => session.move_visual(CaretMovement::Left),
                        "right" => session.move_visual(CaretMovement::Right),
                        "up" if session.has_selection()
                            && !shift
                            && session.stored.spec.writing_mode.is_vertical() =>
                        {
                            session.collapse_visual(false, true)
                        }
                        "up" => session.move_visual(CaretMovement::Up),
                        "down"
                            if session.has_selection()
                                && !shift
                                && session.stored.spec.writing_mode.is_vertical() =>
                        {
                            session.collapse_visual(true, true)
                        }
                        "down" => session.move_visual(CaretMovement::Down),
                        "home" => session.move_visual(CaretMovement::Home),
                        _ => session.move_visual(CaretMovement::End),
                    };
                    session.caret = to;
                    if !shift {
                        session.anchor = to;
                    }
                }
                "backspace" => {
                    if !session.delete_selection() {
                        let to = if word {
                            session.prev_word(session.caret)
                        } else {
                            session.prev_boundary(session.caret)
                        };
                        if to != session.caret {
                            session.replace(to..session.caret, "");
                            session.caret = to;
                            session.anchor = to;
                        }
                    }
                    changed = true;
                }
                "delete" => {
                    if !session.delete_selection() {
                        let to = if word {
                            session.next_word(session.caret)
                        } else {
                            session.next_boundary(session.caret)
                        };
                        if to != session.caret {
                            session.replace(session.caret..to, "");
                        }
                    }
                    changed = true;
                }
                // Anything else with a modifier is a shortcut, not typing.
                _ if modifiers.ctrl_or_cmd => return false,
                "enter" => {
                    session.insert("\n");
                    changed = true;
                }
                "tab" => {
                    session.insert("    ");
                    changed = true;
                }
                "space" => {
                    session.insert(" ");
                    changed = true;
                }
                _ => match text {
                    Some(t) if !t.is_empty() && !t.chars().any(|c| c.is_control()) => {
                        session.insert(t);
                        changed = true;
                    }
                    _ => handled = false,
                },
            }
            if changed {
                session.affinity = CaretAffinity::Downstream;
                session.dirty = true;
            }
        }
        if changed || recolored {
            self.refresh(ctx.doc);
        }
        self.sync_bar();
        if !handled {
            return false;
        }
        // Swallow every plain keystroke while typing so letters don't
        // switch tools mid-word.
        true
    }

    fn options(&self) -> Vec<ToolOption> {
        let families = schist_text_engine::family_names();
        let family = families
            .iter()
            .position(|f| *f == self.spec.family)
            .unwrap_or(0);
        vec![
            ToolOption::choice("type-family", t("common.font"), families, family),
            ToolOption::choice(
                "type-style",
                t("common.style"),
                choices!(STYLES),
                usize::from(self.spec.bold) | (usize::from(self.spec.italic) << 1),
            ),
            ToolOption::slider(
                "type-size",
                t("common.size"),
                self.spec.size,
                6.0,
                400.0,
                t("common.unit.px_suffix"),
            ),
            ToolOption::choice(
                "type-align",
                t("tool.type.option.align"),
                if self.spec.writing_mode.is_vertical() {
                    choices!(VERTICAL_ALIGNMENTS)
                } else {
                    choices!(ALIGNMENTS)
                },
                match self.spec.align {
                    Align::Left => 0,
                    Align::Center => 1,
                    Align::Right => 2,
                },
            ),
            ToolOption::choice(
                "type-direction",
                t("tool.type.option.direction"),
                vec![
                    t("common.automatic"),
                    t("tool.type.choice.ltr"),
                    t("tool.type.choice.rtl"),
                ],
                match self.spec.direction {
                    ParagraphDirection::Auto => 0,
                    ParagraphDirection::LeftToRight => 1,
                    ParagraphDirection::RightToLeft => 2,
                },
            ),
            ToolOption::choice(
                "type-writing-mode",
                t("tool.type.option.writing_mode"),
                vec![
                    t("common.horizontal"),
                    t("tool.type.choice.vertical_rl"),
                    t("tool.type.choice.vertical_lr"),
                ],
                match self.spec.writing_mode {
                    WritingMode::Horizontal => 0,
                    WritingMode::VerticalRl => 1,
                    WritingMode::VerticalLr => 2,
                },
            ),
            ToolOption::slider(
                "type-leading",
                t("tool.type.option.leading"),
                self.spec.line_height,
                0.5,
                3.0,
                "\u{d7}",
            ),
            ToolOption::slider(
                "type-tracking",
                t("tool.type.option.tracking"),
                self.spec.tracking,
                -20.0,
                80.0,
                t("common.unit.px_suffix"),
            ),
            ToolOption::toggle(
                "type-kern",
                t("tool.type.option.kerning"),
                self.spec.feature("kern", true),
            ),
            ToolOption::toggle(
                "type-liga",
                t("tool.type.option.ligatures"),
                self.spec.feature("liga", false),
            ),
            ToolOption::toggle(
                "type-dlig",
                t("tool.type.option.discretionary_ligatures"),
                self.spec.feature("dlig", false),
            ),
            ToolOption::toggle(
                "type-smcp",
                t("tool.type.option.small_caps"),
                self.spec.feature("smcp", false),
            ),
            ToolOption::toggle("type-path", t("tool.type.option.on_path"), self.use_path),
            ToolOption::slider(
                "type-path-offset",
                t("tool.type.option.path_offset"),
                self.spec.path.as_ref().map_or(0.0, |p| p.offset),
                -2000.0,
                2000.0,
                t("common.unit.px_suffix"),
            ),
        ]
    }

    fn set_option(&mut self, key: &str, value: OptionValue) {
        match key {
            "type-family" => {
                if let Some(name) = schist_text_engine::family_names().get(value.index()) {
                    self.spec.family = (*name).to_string();
                }
            }
            "type-style" => {
                let i = value.index();
                self.spec.bold = i & 1 != 0;
                self.spec.italic = i & 2 != 0;
            }
            "type-size" => self.spec.size = value.num().clamp(6.0, 400.0),
            "type-align" => {
                self.spec.align = match value.index() {
                    1 => Align::Center,
                    2 => Align::Right,
                    _ => Align::Left,
                }
            }
            "type-direction" => {
                self.spec.direction = match value.index() {
                    1 => ParagraphDirection::LeftToRight,
                    2 => ParagraphDirection::RightToLeft,
                    _ => ParagraphDirection::Auto,
                }
            }
            "type-writing-mode" => {
                self.spec.writing_mode = match value.index() {
                    1 => WritingMode::VerticalRl,
                    2 => WritingMode::VerticalLr,
                    _ => WritingMode::Horizontal,
                };
                if self.spec.writing_mode.is_vertical() {
                    self.spec.path = None;
                    self.use_path = false;
                }
            }
            "type-leading" => self.spec.line_height = value.num().clamp(0.5, 3.0),
            "type-tracking" => self.spec.tracking = value.num(),
            "type-kern" | "type-liga" | "type-dlig" | "type-smcp" => {
                self.spec.set_feature(&key[5..], value.bool());
            }
            "type-path" => {
                self.use_path = value.bool();
                if self.use_path {
                    self.spec.writing_mode = WritingMode::Horizontal;
                }
            }
            "type-path-offset" => {
                if let Some(path) = &mut self.spec.path {
                    path.offset = value.num();
                }
            }
            _ => {}
        }
    }

    /// Push the bar's setting onto the text being edited, so a font or
    /// size change shows up immediately rather than on the next click.
    ///
    /// Font, style and size are character settings: with a selection
    /// they apply to just those characters, so one layer can mix
    /// families (issue #99); with none they apply to the whole layer.
    /// Alignment, leading and tracking belong to the layer either way.
    fn on_option_changed(&mut self, ctx: &mut ToolCtx, key: &str) {
        let Some(session) = &mut self.editing else {
            return;
        };
        if key == "type-path" {
            session.stored.spec.path = if self.use_path {
                active_text_path(ctx.doc, session.stored.origin)
            } else {
                None
            };
            self.spec.path = session.stored.spec.path.clone();
            self.use_path = self.spec.path.is_some();
        }
        let over = match key {
            "type-family" => StyleRun {
                family: Some(self.spec.family.clone()),
                ..Default::default()
            },
            "type-style" => StyleRun {
                bold: Some(self.spec.bold),
                italic: Some(self.spec.italic),
                ..Default::default()
            },
            "type-size" => StyleRun {
                size: Some(self.spec.size),
                ..Default::default()
            },
            _ => {
                let spec = &mut session.stored.spec;
                spec.align = self.spec.align;
                spec.direction = self.spec.direction;
                spec.writing_mode = self.spec.writing_mode;
                if spec.writing_mode.is_vertical() {
                    spec.path = None;
                }
                spec.line_height = self.spec.line_height;
                spec.tracking = self.spec.tracking;
                spec.features = self.spec.features.clone();
                if key == "type-path-offset" {
                    spec.path = self.spec.path.clone();
                }
                StyleRun::default()
            }
        };
        if !over.is_plain() {
            let range = if session.has_selection() {
                session.selection()
            } else {
                0..session.stored.spec.text.len()
            };
            session.stored.spec.apply_style(range, &over);
        }
        session.dirty = true;
        self.adopt_foreground(ctx.state);
        self.refresh(ctx.doc);
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        self.selecting = false;
        let Some(session) = self.editing.take() else {
            return;
        };
        if !session.dirty {
            // Nothing typed: drop an empty layer we created.
            if session.created {
                let mut edit = ctx.doc.begin_edit(t("tool.type.history.discard_empty"));
                edit.remove_layer(session.layer);
                edit.commit();
                // The insert and this removal cancel out, so collapse the
                // pair rather than leaving two no-op steps in the panel.
                //
                // This used to call `undo()` twice and then `pop_redo()`
                // twice. Both were wrong: `undo()` unwinds whatever is on
                // top, which is only this layer's insert when nothing else
                // was committed in between, and `pop_redo()` is the redo
                // primitive, so it pushed the junk straight back onto the
                // undo stack. Clicking with the type tool and not typing
                // therefore reverted the user's last two real edits, with
                // the History panel still listing them as applied.
                ctx.doc.history.drop_cancelling_pair(
                    t("tool.type.history.discard_empty"),
                    t("tool.type.history.new_layer"),
                );
            }
            return;
        }
        // Put the layer back as it stood before the session, then
        // re-apply everything through the edit builder so one undo
        // restores the pre-edit pixels, spec and name together. Undoing
        // the pixels alone left the layer's text disagreeing with its
        // glyphs, so the next click into it edited the wrong words.
        let (tiles, _) = render_tiles(ctx.doc, &session.stored);
        let name = display_name(&session.stored.spec.text);
        let Some(layer) = ctx.doc.tree.find_mut(session.layer) else {
            return;
        };
        if let Some(raster) = layer.as_raster_mut() {
            raster.tiles = session.original.clone();
        }
        layer.name = session.original_name.clone();
        let extras = std::mem::replace(&mut layer.extras, session.original_extras.clone());
        let mut edit = ctx.doc.begin_edit(t("tool.type.history.edit"));
        edit.replace_layer_tiles(session.layer, tiles);
        edit.set_extras(session.layer, extras);
        edit.change_props(session.layer, |l| l.name = name);
        edit.commit();
    }

    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        self.selecting = false;
        let Some(session) = self.editing.take() else {
            return;
        };
        let before = ctx
            .doc
            .tree
            .find(session.layer)
            .map(|l| l.content_bounds())
            .unwrap_or(IntRect::EMPTY);
        if let Some(layer) = ctx.doc.tree.find_mut(session.layer) {
            if let Some(raster) = layer.as_raster_mut() {
                raster.tiles = session.original.clone();
            }
            layer.extras = session.original_extras.clone();
            layer.name = session.original_name.clone();
        }
        ctx.doc.add_damage(before);
        if session.created {
            let mut edit = ctx.doc.begin_edit(t("tool.type.history.discard"));
            edit.remove_layer(session.layer);
            edit.commit();
        }
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_commit(ctx);
    }

    fn overlays(&self, doc: &Document, _state: &EditorState) -> Vec<Overlay> {
        let Some(session) = &self.editing else {
            return Vec::new();
        };
        let ink_bounds = doc
            .tree
            .find(session.layer)
            .map(|l| l.tight_bounds())
            .unwrap_or(IntRect::EMPTY);
        // Layout coordinates are relative to the layout box's top-left,
        // which `render_tiles` places at `origin`, so the same offset maps
        // a caret onto the canvas. The old overlay measured x from the ink
        // bounds and stepped y by `size * line_height`, neither of which
        // is what the engine actually laid out.
        let (ox, oy) = session.origin_f32();
        let spec = &session.stored.spec;
        let mut out = Vec::new();
        let outline_bounds = ink_bounds.union(&layout_bounds(&session.stored));
        if !ink_bounds.is_empty() {
            out.push(Overlay::Rect(outline_bounds.inflated(2)));
        }

        if session.has_selection() {
            out.extend(
                schist_text_engine::selection_rects(spec, session.selection())
                    .into_iter()
                    .filter_map(|rect| {
                        let rect = rect.translated(ox as i32, oy as i32);
                        let rect = if spec.path.is_none() && !spec.writing_mode.is_vertical() {
                            rect.intersect(&ink_bounds)
                        } else {
                            rect
                        };
                        (!rect.is_empty()).then_some(Overlay::Highlight(rect))
                    }),
            );
        }

        if let Some(caret) = schist_text_engine::caret_at_position(spec, session.position()) {
            let x = ox + caret.x;
            let y = oy + caret.top;
            out.push(Overlay::Caret {
                x1: x,
                y1: y,
                x2: x - caret.angle.sin() * caret.height,
                y2: y + caret.angle.cos() * caret.height,
                color: Rgba::from_u8(
                    session.stored.color[0],
                    session.stored.color[1],
                    session.stored.color[2],
                    255,
                ),
            });
        }
        out
    }
}

impl Editing {
    /// The selected byte range, empty when the caret is a plain insertion
    /// point.
    fn selection(&self) -> std::ops::Range<usize> {
        let (a, b) = (self.caret.min(self.anchor), self.caret.max(self.anchor));
        a..b
    }

    fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    fn text(&self) -> &str {
        &self.stored.spec.text
    }

    /// Replace `range` of the text with `s`, keeping the style runs in
    /// step so a word typed after a bold one stays bold.
    fn replace(&mut self, range: std::ops::Range<usize>, s: &str) {
        self.stored.spec.text.replace_range(range.clone(), s);
        self.stored.spec.splice_runs(range, s.len());
    }

    /// The font at the caret: the selection's first character, or the
    /// character before an insertion point.
    fn caret_style(&self) -> schist_text_engine::CharStyle {
        let at = if self.has_selection() {
            self.selection().start
        } else if self.caret > 0 {
            self.prev_boundary(self.caret)
        } else {
            0
        };
        self.stored.spec.style_at(at)
    }

    /// Collapse the selection, returning true if anything was removed.
    fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.replace(range.clone(), "");
        self.caret = range.start;
        self.anchor = range.start;
        true
    }

    /// Insert at the caret, replacing the selection first.
    fn insert(&mut self, s: &str) {
        self.delete_selection();
        let at = self.caret.min(self.stored.spec.text.len());
        self.replace(at..at, s);
        self.caret = at + s.len();
        self.anchor = self.caret;
    }

    /// Byte offset one grapheme before `at`.
    ///
    /// Whole-grapheme rather than whole-`char`, so backspacing an accented
    /// letter or an emoji removes what looks like one character instead of
    /// peeling off a combining mark at a time.
    fn prev_boundary(&self, at: usize) -> usize {
        schist_text_engine::grapheme_boundaries(self.text())
            .rev()
            .find(|i| *i < at)
            .unwrap_or(0)
    }

    fn next_boundary(&self, at: usize) -> usize {
        schist_text_engine::grapheme_boundaries(self.text())
            .find(|i| *i > at)
            .unwrap_or(self.text().len())
    }

    fn position(&self) -> CaretPosition {
        CaretPosition {
            byte: self.caret,
            affinity: self.affinity,
        }
    }

    fn collapse_visual(&mut self, forward: bool, vertical: bool) -> usize {
        let range = self.selection();
        let candidates = [
            CaretPosition::from(range.start),
            CaretPosition {
                byte: range.end,
                affinity: CaretAffinity::Upstream,
            },
        ];
        let position = candidates
            .into_iter()
            .filter_map(|p| {
                schist_text_engine::caret_at_position(&self.stored.spec, p)
                    .map(|c| (p, if vertical { c.top } else { c.x }))
            })
            .min_by(|a, b| {
                if forward {
                    b.1.total_cmp(&a.1)
                } else {
                    a.1.total_cmp(&b.1)
                }
            })
            .map_or_else(|| range.start.into(), |(p, _)| p);
        self.affinity = position.affinity;
        position.byte
    }

    fn move_word_visual(&mut self, right: bool) -> usize {
        let movement = if right {
            CaretMovement::Right
        } else {
            CaretMovement::Left
        };
        let mut position = self.position();
        let mut next = schist_text_engine::move_caret(&self.stored.spec, position, movement);
        // A soft-wrap boundary has two visual locations for one byte. Cross
        // that affinity transition before deciding which logical word to skip.
        if next.byte == position.byte && next != position {
            position = next;
            next = schist_text_engine::move_caret(&self.stored.spec, position, movement);
        }
        if next.byte == self.caret {
            self.affinity = next.affinity;
            return self.caret;
        }
        if next.byte > self.caret {
            self.affinity = CaretAffinity::Upstream;
            self.next_word(self.caret)
        } else {
            self.affinity = CaretAffinity::Downstream;
            self.prev_word(self.caret)
        }
    }

    fn move_visual(&mut self, movement: CaretMovement) -> usize {
        let position = schist_text_engine::move_caret(&self.stored.spec, self.position(), movement);
        self.affinity = position.affinity;
        position.byte
    }

    /// Start of the word at or before `at`.
    fn prev_word(&self, at: usize) -> usize {
        let text = self.text();
        let mut i = at;
        while i > 0 {
            let p = self.prev_boundary(i);
            let ch = text[p..].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            i = p;
        }
        while i > 0 {
            let p = self.prev_boundary(i);
            let ch = text[p..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            i = p;
        }
        i
    }

    /// End of the word at or after `at`.
    fn next_word(&self, at: usize) -> usize {
        let text = self.text();
        let mut i = at;
        while i < text.len() {
            let ch = text[i..].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            i = self.next_boundary(i);
        }
        while i < text.len() {
            let ch = text[i..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            i = self.next_boundary(i);
        }
        i
    }

    fn origin_f32(&self) -> (f32, f32) {
        (self.stored.origin.0 as f32, self.stored.origin.1 as f32)
    }
}

/// Set the alignment of the layer currently being edited (used by the tool
/// options bar).
pub fn set_align(tool: &mut TypeTool, doc: &mut Document, align: Align) {
    if let Some(session) = &mut tool.editing {
        session.stored.spec.align = align;
        session.dirty = true;
    }
    tool.refresh(doc);
}

/// Set the font size of the layer currently being edited.
pub fn set_size(tool: &mut TypeTool, doc: &mut Document, size: f32) {
    if let Some(session) = &mut tool.editing {
        let len = session.stored.spec.text.len();
        session.stored.spec.apply_style(
            0..len,
            &StyleRun {
                size: Some(size.clamp(4.0, 800.0)),
                ..Default::default()
            },
        );
        session.dirty = true;
    }
    tool.refresh(doc);
}

/// Family of the text layer being edited, if any.
pub fn editing_family(tool: &TypeTool) -> Option<String> {
    tool.editing.as_ref().map(|e| e.stored.spec.family.clone())
}

/// Set the font family of the layer currently being edited.
pub fn set_family(tool: &mut TypeTool, doc: &mut Document, family: String) {
    if let Some(session) = &mut tool.editing {
        let len = session.stored.spec.text.len();
        session.stored.spec.apply_style(
            0..len,
            &StyleRun {
                family: Some(family),
                ..Default::default()
            },
        );
        session.dirty = true;
    }
    tool.refresh(doc);
}

/// True while a text layer is open for editing.
pub fn is_editing(tool: &TypeTool) -> bool {
    tool.editing.is_some()
}

pub struct TypeToolsPlugin;

impl PluginManifest for TypeToolsPlugin {
    fn id(&self) -> &'static str {
        "schist.tools-type"
    }

    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_tool(Box::new(TypeTool::default()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_color::Depth;

    fn doc() -> Document {
        let mut d = Document::new("t", 300, 200, Depth::Eight);
        d.push_layer(Layer::new_raster("bg"));
        d
    }

    fn input(x: f32, y: f32) -> PointerInput {
        PointerInput {
            x,
            y,
            pressure: 1.0,
            modifiers: Modifiers::default(),
        }
    }

    fn type_text(tool: &mut TypeTool, ctx: &mut ToolCtx, s: &str) {
        for ch in s.chars() {
            let text = ch.to_string();
            let key = if ch == ' ' { "space" } else { &text };
            tool.on_key(ctx, key, Some(&text), Modifiers::default());
        }
    }

    #[test]
    fn rtl_typing_arrows_selection_and_grapheme_deletion_use_logical_text() {
        let mut d = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.set_option("type-direction", OptionValue::Choice(2));
        tool.on_pointer_down(&mut ctx, input(20.0, 40.0));
        type_text(&mut tool, &mut ctx, "אבג");
        let end = tool.editing.as_ref().unwrap().caret;
        tool.on_key(
            &mut ctx,
            "right",
            None,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(tool.editing.as_ref().unwrap().caret, end - 'ג'.len_utf8());
        assert!(tool
            .overlays(ctx.doc, ctx.state)
            .iter()
            .any(|o| matches!(o, Overlay::Highlight(_))));
        type_text(&mut tool, &mut ctx, "שָ");
        assert_eq!(tool.editing.as_ref().unwrap().text(), "אבשָ");
        tool.on_key(&mut ctx, "backspace", None, Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().text(), "אב");
        // Backspace also keeps a ZWJ emoji sequence intact.
        type_text(&mut tool, &mut ctx, "👩‍👩‍👧‍👦");
        tool.on_key(&mut ctx, "backspace", None, Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().text(), "אב");
        tool.on_commit(&mut ctx);
        assert_eq!(
            ctx.doc
                .tree
                .iter()
                .find_map(read_stored)
                .unwrap()
                .spec
                .direction,
            ParagraphDirection::RightToLeft
        );
    }

    #[test]
    fn vertical_editing_pointer_selection_save_and_undo_preserve_writing_mode() {
        for mode in [1, 2] {
            let mut d = doc();
            let mut state = EditorState::default();
            let mut tool = TypeTool::default();
            let mut ctx = ToolCtx {
                doc: &mut d,
                state: &mut state,
            };
            tool.set_option("type-writing-mode", OptionValue::Choice(mode));
            tool.on_pointer_down(&mut ctx, input(40.0, 40.0));
            type_text(&mut tool, &mut ctx, "日本語");
            tool.on_key(&mut ctx, "enter", None, Modifiers::default());
            type_text(&mut tool, &mut ctx, "東京");
            let stored = tool.editing.as_ref().unwrap().stored.clone();
            let at = schist_text_engine::caret_at(&stored.spec, '日'.len_utf8()).unwrap();
            let x = stored.origin.0 as f32 + at.x - at.height / 2.0;
            let y = stored.origin.1 as f32 + at.top;
            tool.on_pointer_down(&mut ctx, input(x, y));
            tool.on_pointer_up(&mut ctx, input(x, y));
            assert_eq!(tool.editing.as_ref().unwrap().caret, '日'.len_utf8());
            tool.on_key(
                &mut ctx,
                "down",
                None,
                Modifiers {
                    shift: true,
                    ..Default::default()
                },
            );
            assert_eq!(tool.editing.as_ref().unwrap().selection(), 3..6);
            assert!(tool
                .overlays(ctx.doc, ctx.state)
                .iter()
                .any(|o| matches!(o, Overlay::Highlight(_))));
            tool.on_commit(&mut ctx);
            let before = ctx.doc.tree.iter().find_map(read_stored).unwrap();
            for psb in [false, true] {
                let bytes = schist_codec_psd::write_psd_with(ctx.doc, psb).unwrap();
                let reopened = schist_codec_psd::read_psd(&bytes).unwrap();
                let back = reopened.tree.iter().find_map(read_stored).unwrap();
                assert_eq!(back.spec, before.spec);
                assert_eq!(
                    render_tiles(&reopened, &back).1,
                    render_tiles(ctx.doc, &before).1
                );
            }
            tool.on_pointer_down(&mut ctx, input(x, y));
            tool.set_option("type-writing-mode", OptionValue::Choice(0));
            tool.on_option_changed(&mut ctx, "type-writing-mode");
            tool.on_commit(&mut ctx);
            assert_eq!(
                ctx.doc
                    .tree
                    .iter()
                    .find_map(read_stored)
                    .unwrap()
                    .spec
                    .writing_mode,
                WritingMode::Horizontal
            );
            ctx.doc.undo();
            assert_eq!(
                ctx.doc.tree.iter().find_map(read_stored).unwrap().spec,
                before.spec
            );
            ctx.doc.redo();
            assert_eq!(
                ctx.doc
                    .tree
                    .iter()
                    .find_map(read_stored)
                    .unwrap()
                    .spec
                    .writing_mode,
                WritingMode::Horizontal
            );
        }
    }

    #[test]
    fn path_and_features_survive_psd_reopen_and_undo() {
        let mut d = doc();
        let mut path = schist_core::path::VectorPath::new("Baseline");
        path.push_open_anchors(vec![
            schist_core::path::Anchor::corner(100.0, 40.0),
            schist_core::path::Anchor::corner(100.0, 190.0),
        ]);
        d.paths.push(path);
        d.active_path = Some(0);
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.set_option("type-path", OptionValue::Bool(true));
        tool.on_pointer_down(&mut ctx, input(100.0, 40.0));
        type_text(&mut tool, &mut ctx, "office");
        for (key, value) in [
            ("type-liga", OptionValue::Bool(true)),
            ("type-path-offset", OptionValue::Num(12.0)),
        ] {
            tool.set_option(key, value);
            tool.on_option_changed(&mut ctx, key);
        }
        let stored = tool.editing.as_ref().unwrap().stored.clone();
        assert_eq!(stored.spec.path.as_ref().unwrap().offset, 12.0);
        assert!(stored.spec.feature("liga", false));
        let caret = tool
            .overlays(ctx.doc, ctx.state)
            .into_iter()
            .find_map(|o| match o {
                Overlay::Caret { x1, y1, x2, y2, .. } => Some((x1, y1, x2, y2)),
                _ => None,
            })
            .unwrap();
        assert!(
            (caret.1 - caret.3).abs() < 0.001,
            "vertical path needs a horizontal caret"
        );
        assert!((caret.0 - caret.2).abs() > 10.0);
        tool.on_commit(&mut ctx);
        for psb in [false, true] {
            let bytes = schist_codec_psd::write_psd_with(ctx.doc, psb).unwrap();
            let reopened = schist_codec_psd::read_psd(&bytes).unwrap();
            let back = reopened.tree.iter().find_map(read_stored).unwrap();
            assert_eq!(back.spec, stored.spec);
            let before = render_tiles(ctx.doc, &stored).1;
            assert_eq!(render_tiles(&reopened, &back).1, before);
        }
        // Editing the same layer is a single undoable change to metadata
        // and pixels, including toggling the baseline back off.
        let c = schist_text_engine::caret_at(&stored.spec, 0).unwrap();
        tool.on_pointer_down(
            &mut ctx,
            input(c.x + stored.origin.0 as f32, c.top + stored.origin.1 as f32),
        );
        tool.set_option("type-path", OptionValue::Bool(false));
        tool.on_option_changed(&mut ctx, "type-path");
        tool.on_commit(&mut ctx);
        assert!(ctx
            .doc
            .tree
            .iter()
            .find_map(read_stored)
            .unwrap()
            .spec
            .path
            .is_none());
        ctx.doc.undo();
        assert_eq!(
            ctx.doc.tree.iter().find_map(read_stored).unwrap().spec,
            stored.spec
        );
    }

    #[test]
    fn moving_a_text_layer_moves_its_editable_origin_and_undo_restores_bytes() {
        let mut d = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 40.0));
        type_text(&mut tool, &mut ctx, "Move me");
        tool.on_commit(&mut ctx);
        let id = ctx.doc.active_layer.unwrap();
        let before = read_stored(ctx.doc.tree.find(id).unwrap()).unwrap();
        let extras = ctx.doc.tree.find(id).unwrap().extras.clone();
        let mut edit = ctx.doc.begin_edit("Move text");
        edit.translate_layer(id, 31, -13);
        edit.commit();
        let moved = read_stored(ctx.doc.tree.find(id).unwrap()).unwrap();
        assert_eq!(moved.origin, (before.origin.0 + 31, before.origin.1 - 13));
        let (rendered, _) = render_tiles(ctx.doc, &moved);
        let pixels = &ctx.doc.tree.find(id).unwrap().as_raster().unwrap().tiles;
        assert_eq!(rendered.content_bounds(), pixels.content_bounds());
        for coord in TileCoord::covering(&rendered.content_bounds()) {
            assert_eq!(rendered.get(coord), pixels.get(coord));
        }
        ctx.doc.undo();
        assert_eq!(ctx.doc.tree.find(id).unwrap().extras, extras);
        ctx.doc.redo();
        assert_eq!(
            read_stored(ctx.doc.tree.find(id).unwrap()).unwrap().origin,
            moved.origin
        );
    }

    #[test]
    fn text_uses_the_same_live_shape_as_path_selection() {
        let mut d = doc();
        let mut path = schist_core::VectorPath::new("Shape baseline");
        path.push_open_anchors(vec![
            schist_core::Anchor::corner(20.0, 30.0),
            schist_core::Anchor::corner(120.0, 80.0),
        ]);
        d.tree.find_mut(d.active_layer.unwrap()).unwrap().shape = Some(Box::new(
            schist_core::VectorShape::new(path.clone(), Rgba::BLACK),
        ));
        d.paths.push(schist_core::VectorPath::new("Unrelated path"));
        d.active_path = Some(0);
        let copied = active_text_path(&d, (10, 15)).unwrap();
        assert_eq!(copied.curve.anchors[0].point, (10.0, 15.0));
        assert_eq!(copied.curve.anchors[1].point, (110.0, 65.0));
        assert_eq!(
            d.tree
                .find(d.active_layer.unwrap())
                .unwrap()
                .shape
                .as_ref()
                .unwrap()
                .path,
            path
        );
    }

    #[test]
    fn the_options_bar_settings_reach_the_text() {
        let mut d = doc();
        let mut state = schist_plugin_api::EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "Hi");

        // Size, alignment and style all land on the live session.
        tool.set_option("type-size", OptionValue::Num(96.0));
        tool.on_option_changed(&mut ctx, "type-size");
        tool.set_option("type-align", OptionValue::Choice(2));
        tool.on_option_changed(&mut ctx, "type-align");
        tool.set_option("type-style", OptionValue::Choice(3));
        tool.on_option_changed(&mut ctx, "type-style");

        let session = tool.editing.as_ref().expect("still editing");
        assert_eq!(session.stored.spec.size, 96.0);
        assert_eq!(session.stored.spec.align, Align::Right);
        assert!(session.stored.spec.bold && session.stored.spec.italic);
        assert_eq!(session.stored.spec.text, "Hi", "the typing survives");
    }

    fn select(tool: &mut TypeTool, anchor: usize, caret: usize) {
        let session = tool.editing.as_mut().expect("editing");
        session.anchor = anchor;
        session.caret = caret;
        tool.sync_bar();
    }

    fn shown(tool: &TypeTool, key: &str) -> OptionValue {
        tool.options()
            .into_iter()
            .find(|o| o.key == key)
            .expect("an option")
            .value
    }

    #[test]
    fn a_selected_word_takes_its_own_size_and_typing_after_it_keeps_it() {
        let mut d = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "Hi there");
        let plain = d.tree.layers[1].tight_bounds();

        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        select(&mut tool, 3, 8);
        tool.set_option("type-size", OptionValue::Num(96.0));
        tool.on_option_changed(&mut ctx, "type-size");
        {
            let spec = &tool.editing.as_ref().unwrap().stored.spec;
            assert_eq!(spec.size, 48.0, "the layer's own size is untouched");
            assert_eq!(spec.runs.len(), 1);
            assert_eq!((spec.runs[0].start, spec.runs[0].end), (3, 8));
            assert_eq!(spec.runs[0].size, Some(96.0));
        }
        assert!(
            d.tree.layers[1].tight_bounds().height() > plain.height() + 10,
            "the big word shows on the canvas"
        );

        // Typing after the big word continues in it, and the bar says so.
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        select(&mut tool, 8, 8);
        tool.on_key(&mut ctx, "!", Some("!"), Modifiers::default());
        let spec = &tool.editing.as_ref().unwrap().stored.spec;
        assert_eq!(spec.text, "Hi there!");
        assert_eq!((spec.runs[0].start, spec.runs[0].end), (3, 9));
        assert_eq!(shown(&tool, "type-size").num(), 96.0);
        // The caret back in the small text shows the small size.
        select(&mut tool, 1, 1);
        assert_eq!(shown(&tool, "type-size").num(), 48.0);
    }

    #[test]
    fn dragging_over_a_word_lets_it_take_its_own_font() {
        let mut d = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "Hi there");

        let session = tool.editing.as_ref().unwrap();
        let (ox, oy) = session.origin_f32();
        let from = schist_text_engine::caret_at(&session.stored.spec, 3).unwrap();
        let to = schist_text_engine::caret_at(&session.stored.spec, 8).unwrap();
        let y = oy + from.top + from.height / 2.0;
        tool.on_pointer_down(&mut ctx, input(ox + from.x, y));
        tool.on_pointer_move(&mut ctx, input(ox + to.x, y));
        tool.on_pointer_up(&mut ctx, input(ox + to.x, y));

        assert_eq!(tool.editing.as_ref().unwrap().selection(), 3..8);
        let bounds = ctx
            .doc
            .tree
            .find(ctx.doc.active_layer.unwrap())
            .unwrap()
            .tight_bounds();
        let highlights: Vec<_> = tool
            .overlays(ctx.doc, ctx.state)
            .into_iter()
            .filter_map(|overlay| match overlay {
                Overlay::Highlight(rect) => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0], highlights[0].intersect(&bounds));

        let base = tool.editing.as_ref().unwrap().stored.spec.family.clone();
        tool.spec.family = "A different family".into();
        tool.on_option_changed(&mut ctx, "type-family");

        let spec = &tool.editing.as_ref().unwrap().stored.spec;
        assert_eq!(spec.family, base, "the layer's base font stays put");
        assert_eq!(spec.runs.len(), 1);
        assert_eq!((spec.runs[0].start, spec.runs[0].end), (3, 8));
        assert_eq!(spec.runs[0].family.as_deref(), Some("A different family"));
    }

    #[test]
    fn with_nothing_selected_the_bar_restyles_the_whole_layer() {
        let mut d = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "Hi there");
        select(&mut tool, 3, 8);
        tool.set_option("type-style", OptionValue::Choice(1));
        tool.on_option_changed(&mut ctx, "type-style");
        select(&mut tool, 8, 8);
        tool.set_option("type-size", OptionValue::Num(20.0));
        tool.on_option_changed(&mut ctx, "type-size");
        let spec = &tool.editing.as_ref().unwrap().stored.spec;
        assert_eq!(spec.size, 20.0);
        assert!(!spec.bold, "the layer's own style is unchanged");
        assert_eq!(spec.runs.len(), 1, "the bold word keeps its bold");
        assert_eq!(spec.runs[0].bold, Some(true));
        assert_eq!(spec.runs[0].size, None);
    }

    #[test]
    fn undo_restores_the_text_and_name_with_the_pixels() {
        let mut d = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "Hi");
        tool.on_commit(&mut ctx);
        let layer = d.tree.layers[1].id;
        assert_eq!(read_stored(&d.tree.layers[1]).unwrap().spec.text, "Hi");

        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(30.0, 80.0));
        assert_eq!(tool.editing.as_ref().unwrap().layer, layer, "resumed");
        select(&mut tool, 0, 2);
        tool.set_option("type-size", OptionValue::Num(96.0));
        tool.on_option_changed(&mut ctx, "type-size");
        type_text(&mut tool, &mut ctx, "Yo");
        tool.on_commit(&mut ctx);
        assert_eq!(read_stored(&d.tree.layers[1]).unwrap().spec.text, "Yo");
        assert_eq!(d.tree.layers[1].name, "Yo");

        assert_eq!(d.undo().as_deref(), Some(t("tool.type.history.edit")));
        let stored = read_stored(&d.tree.layers[1]).unwrap();
        assert_eq!(stored.spec.text, "Hi", "the spec undoes with the pixels");
        assert_eq!(stored.spec.size, 48.0);
        assert_eq!(d.tree.layers[1].name, "Hi");
        d.redo();
        assert_eq!(read_stored(&d.tree.layers[1]).unwrap().spec.text, "Yo");
        assert_eq!(d.tree.layers[1].name, "Yo");
    }

    #[test]
    fn a_bigger_size_renders_bigger_text() {
        let render = |size: f32| {
            let mut d = doc();
            let mut state = schist_plugin_api::EditorState::default();
            let mut tool = TypeTool::default();
            tool.set_option("type-size", OptionValue::Num(size));
            let mut ctx = ToolCtx {
                doc: &mut d,
                state: &mut state,
            };
            tool.on_pointer_down(&mut ctx, input(20.0, 120.0));
            type_text(&mut tool, &mut ctx, "AB");
            let layer = tool.editing.as_ref().unwrap().layer;
            d.tree.find(layer).unwrap().tight_bounds().width()
        };
        let small = render(16.0);
        let large = render(64.0);
        assert!(small > 0, "the small text drew something");
        assert!(
            large > small * 2,
            "64px text should be far wider than 16px: {small} vs {large}"
        );
    }

    #[test]
    fn editing_an_existing_layer_adopts_its_settings() {
        let mut d = doc();
        let mut state = schist_plugin_api::EditorState::default();
        let mut tool = TypeTool::default();
        {
            let mut ctx = ToolCtx {
                doc: &mut d,
                state: &mut state,
            };
            tool.set_option("type-size", OptionValue::Num(72.0));
            tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
            type_text(&mut tool, &mut ctx, "X");
            tool.on_commit(&mut ctx);

            // A new layer somewhere else, at a different size.
            tool.set_option("type-size", OptionValue::Num(20.0));
            tool.on_pointer_down(&mut ctx, input(200.0, 160.0));
            type_text(&mut tool, &mut ctx, "Y");
            tool.on_commit(&mut ctx);

            // Clicking back into the first one should show 72 again.
            // Aim at the glyph itself: hit-testing uses the inked bounds,
            // which sit below the origin you clicked to place the text.
            tool.on_pointer_down(&mut ctx, input(40.0, 100.0));
        }
        let shown = tool
            .options()
            .into_iter()
            .find(|o| o.key == "type-size")
            .expect("a size option")
            .value
            .num();
        assert_eq!(shown, 72.0, "the bar should describe the text you clicked");
    }

    fn ink(doc: &Document) -> usize {
        doc.tree
            .layers
            .last()
            .unwrap()
            .as_raster()
            .unwrap()
            .tiles
            .iter()
            .map(|(_, buf)| {
                (0..schist_core::TILE_PIXELS)
                    .filter(|&i| buf.get(i).a > 0.0)
                    .count()
            })
            .sum()
    }

    #[test]
    fn typing_creates_a_text_layer_with_pixels() {
        let mut doc = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "Hello");
        tool.on_commit(&mut ctx);

        assert_eq!(doc.tree.layers.len(), 2);
        assert!(ink(&doc) > 20, "text drew pixels");
        assert_eq!(doc.tree.layers[1].name, "Hello");
    }

    #[test]
    fn the_editing_outline_contains_one_text_coloured_caret() {
        let mut doc = doc();
        let mut state = EditorState {
            foreground: Rgba::from_u8(12, 34, 56, 255),
            ..EditorState::default()
        };
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "g");

        let mut outline = None;
        let mut caret = None;
        for overlay in tool.overlays(ctx.doc, ctx.state) {
            match overlay {
                Overlay::Rect(rect) => outline = Some(rect),
                Overlay::Caret {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                } => caret = Some((x1, y1, x2, y2, color.to_u8())),
                _ => {}
            }
        }
        let outline = outline.expect("text outline");
        let (x1, y1, x2, y2, color) = caret.expect("insertion caret");
        assert_eq!(color, [12, 34, 56, 255]);
        assert_eq!(x1, x2, "the caret is one vertical stroke");
        assert!(x1 >= outline.left as f32 && x1 <= outline.right as f32);
        assert!(y1 >= outline.top as f32, "caret starts inside the outline");
        assert!(y2 <= outline.bottom as f32, "caret ends inside the outline");
    }

    #[test]
    fn text_spec_is_preserved_on_the_layer() {
        let mut doc = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(10.0, 40.0));
        type_text(&mut tool, &mut ctx, "Hi");
        tool.on_commit(&mut ctx);

        let stored = read_stored(&doc.tree.layers[1]).expect("spec stored in extras");
        assert_eq!(stored.spec.text, "Hi");
        assert_eq!(stored.origin, (10, 40));
    }

    #[test]
    fn clicking_an_existing_text_layer_resumes_editing() {
        let mut doc = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        {
            let mut ctx = ToolCtx {
                doc: &mut doc,
                state: &mut state,
            };
            tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
            type_text(&mut tool, &mut ctx, "AB");
            tool.on_commit(&mut ctx);
        }
        let layers_before = doc.tree.layers.len();
        let bounds = doc.tree.layers[1].tight_bounds();

        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(
            &mut ctx,
            input(bounds.right as f32 + 2.0, bounds.top as f32 + 2.0),
        );
        assert!(is_editing(&tool), "resumed editing the existing layer");
        type_text(&mut tool, &mut ctx, "C");
        tool.on_commit(&mut ctx);

        assert_eq!(doc.tree.layers.len(), layers_before, "no new layer");
        assert_eq!(read_stored(&doc.tree.layers[1]).unwrap().spec.text, "ABC");
    }

    #[test]
    fn a_freshly_picked_colour_reaches_text_being_edited() {
        // The eyedropper bug: it wrote the foreground swatch, but a text
        // layer kept the colour it was created with forever, so picking a
        // colour and clicking back into the text changed nothing.
        let mut doc = doc();
        let mut state = EditorState {
            foreground: Rgba::from_u8(255, 255, 255, 255),
            ..Default::default()
        };
        {
            let mut ctx = ToolCtx {
                doc: &mut doc,
                state: &mut state,
            };
            let mut tool = TypeTool::default();
            tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
            type_text(&mut tool, &mut ctx, "AB");
            tool.on_commit(&mut ctx);
        }
        assert_eq!(
            read_stored(&doc.tree.layers[1]).unwrap().color,
            [255, 255, 255, 255]
        );

        // Sample a new colour (what the eyedropper does), then click back
        // into the text: it must adopt the pick.
        state.foreground = Rgba::from_u8(255, 128, 0, 255);
        let bounds = doc.tree.layers[1].tight_bounds();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(
            &mut ctx,
            input(bounds.left as f32 + 2.0, bounds.top as f32 + 2.0),
        );
        assert!(is_editing(&tool), "resumed editing the existing layer");
        tool.on_commit(&mut ctx);

        assert_eq!(
            read_stored(&doc.tree.layers[1]).unwrap().color,
            [255, 128, 0, 255],
            "the text must take the picked colour"
        );
        // And the rendered pixels are the new colour, not the old one.
        let raster = doc.tree.layers[1].as_raster().unwrap();
        let inked = raster
            .tiles
            .iter()
            .flat_map(|(_, buf)| (0..schist_core::TILE_PIXELS).map(|i| buf.get(i)))
            .find(|p| p.a > 0.9)
            .expect("the text still has ink");
        assert!(
            inked.g < 0.6 && inked.b < 0.1,
            "pixels should be orange now, got {inked:?}"
        );
    }

    #[test]
    fn backspace_deletes_and_undo_restores_previous_text() {
        let mut doc = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "Hey");
        tool.on_key(&mut ctx, "backspace", None, Modifiers::default());
        tool.on_commit(&mut ctx);
        assert_eq!(read_stored(&doc.tree.layers[1]).unwrap().spec.text, "He");

        let with_text = ink(&doc);
        doc.undo(); // "Edit Text"
        assert!(ink(&doc) < with_text, "undo removed the rendered glyphs");
    }

    #[test]
    fn escape_discards_a_new_empty_layer() {
        let mut doc = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_text(&mut tool, &mut ctx, "oops");
        tool.on_cancel(&mut ctx);
        assert_eq!(doc.tree.layers.len(), 1, "cancelled layer is gone");
    }

    #[test]
    fn modifier_keys_are_not_swallowed() {
        let mut doc = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        let consumed = tool.on_key(
            &mut ctx,
            "z",
            Some("z"),
            Modifiers {
                ctrl_or_cmd: true,
                ..Default::default()
            },
        );
        assert!(!consumed, "ctrl-z must reach the keymap");
    }

    #[test]
    fn newline_grows_the_layer_downwards() {
        let mut doc = doc();
        let mut state = EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 40.0));
        type_text(&mut tool, &mut ctx, "A");
        let one_line = doc.tree.layers[1].tight_bounds().height();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_key(&mut ctx, "enter", None, Modifiers::default());
        type_text(&mut tool, &mut ctx, "B");
        let two_lines = doc.tree.layers[1].tight_bounds().height();
        assert!(two_lines > one_line + 10, "{two_lines} vs {one_line}");
    }
    #[test]
    fn clicking_without_typing_leaves_earlier_edits_alone() {
        // The data loss. `on_commit` on an untouched session called
        // `undo()` twice, which unwinds whatever is on top rather than
        // this layer's own insert. The two only line up when nothing was
        // committed in between; a command run mid-session (which is
        // exactly what a shortcut does, since running one does not commit
        // the pending tool session) puts a real edit on top instead.
        let mut d = doc();
        let mut state = schist_plugin_api::EditorState::default();
        let mut tool = TypeTool::default();

        {
            let mut ctx = ToolCtx {
                doc: &mut d,
                state: &mut state,
            };
            tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        }

        // A real edit committed while the empty session is still open.
        {
            let mut edit = d.begin_edit("Important Edit");
            edit.insert_layer(LayerPath(vec![0]), Layer::new_raster("important"));
            edit.commit();
        }
        assert!(d.tree.iter().any(|l| l.name == "important"));

        // Now click away without having typed anything.
        {
            let mut ctx = ToolCtx {
                doc: &mut d,
                state: &mut state,
            };
            tool.on_commit(&mut ctx);
        }

        let after: Vec<String> = d.tree.iter().map(|l| l.name.clone()).collect();
        assert!(
            after.iter().any(|n| n == "important"),
            "the edit made during the session must survive: {after:?}"
        );
        assert!(
            d.history
                .entries()
                .iter()
                .any(|e| e.name == "Important Edit"),
            "and it must still be in history"
        );
    }

    #[test]
    fn discarding_an_empty_layer_leaves_no_junk_history() {
        let mut d = doc();
        let mut state = schist_plugin_api::EditorState::default();
        let mut tool = TypeTool::default();
        let before = d.history.entries().len();
        {
            let mut ctx = ToolCtx {
                doc: &mut d,
                state: &mut state,
            };
            tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
            tool.on_commit(&mut ctx);
        }
        assert_eq!(
            d.history.entries().len(),
            before,
            "the insert and its removal should collapse"
        );
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl_or_cmd: true,
            ..Modifiers::default()
        }
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }

    /// Type `s`, sending Enter for newlines the way a keyboard does.
    fn type_lines(tool: &mut TypeTool, ctx: &mut ToolCtx, s: &str) {
        for ch in s.chars() {
            let text = ch.to_string();
            match ch {
                '\n' => {
                    tool.on_key(ctx, "enter", None, Modifiers::default());
                }
                ' ' => {
                    tool.on_key(ctx, "space", Some(" "), Modifiers::default());
                }
                _ => {
                    tool.on_key(ctx, &text, Some(&text), Modifiers::default());
                }
            }
        }
    }

    /// A tool mid-session with `s` typed into it.
    fn editing(d: &mut Document, s: &str) -> TypeTool {
        let mut state = schist_plugin_api::EditorState::default();
        let mut tool = TypeTool::default();
        let mut ctx = ToolCtx {
            doc: d,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(20.0, 60.0));
        type_lines(&mut tool, &mut ctx, s);
        tool
    }

    fn key(tool: &mut TypeTool, d: &mut Document, k: &str, m: Modifiers) -> bool {
        let mut state = schist_plugin_api::EditorState::default();
        let mut ctx = ToolCtx {
            doc: d,
            state: &mut state,
        };
        tool.on_key(&mut ctx, k, None, m)
    }

    #[test]
    fn ctrl_a_selects_the_text_not_the_canvas() {
        // The reported bug. The binding change stops the canvas Select All
        // running; this is the half that makes the keystroke do the right
        // thing once it arrives.
        let mut d = doc();
        let mut tool = editing(&mut d, "hello");
        assert!(key(&mut tool, &mut d, "a", ctrl()), "must be consumed");
        let s = tool.editing.as_ref().unwrap();
        assert_eq!(s.selection(), 0..5);
        assert!(s.has_selection());
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut d = doc();
        let mut tool = editing(&mut d, "hello");
        key(&mut tool, &mut d, "a", ctrl());
        let mut state = schist_plugin_api::EditorState::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_key(&mut ctx, "x", Some("x"), Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().text(), "x");
    }

    #[test]
    fn the_caret_moves_and_inserts_in_the_middle() {
        let mut d = doc();
        let mut tool = editing(&mut d, "ac");
        key(&mut tool, &mut d, "left", Modifiers::default());
        let mut state = schist_plugin_api::EditorState::default();
        let mut ctx = ToolCtx {
            doc: &mut d,
            state: &mut state,
        };
        tool.on_key(&mut ctx, "b", Some("b"), Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().text(), "abc");
    }

    #[test]
    fn home_and_end_reach_the_line_edges() {
        let mut d = doc();
        let mut tool = editing(&mut d, "one\ntwo");
        key(&mut tool, &mut d, "home", Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().caret, 4);
        key(&mut tool, &mut d, "end", Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().caret, 7);
    }

    #[test]
    fn shift_arrow_extends_a_selection_and_a_plain_arrow_collapses_it() {
        let mut d = doc();
        let mut tool = editing(&mut d, "abcd");
        key(&mut tool, &mut d, "left", shift());
        key(&mut tool, &mut d, "left", shift());
        assert_eq!(tool.editing.as_ref().unwrap().selection(), 2..4);
        key(&mut tool, &mut d, "left", Modifiers::default());
        let s = tool.editing.as_ref().unwrap();
        assert!(!s.has_selection());
        assert_eq!(s.caret, 2, "collapses to the selection's near edge");
    }

    #[test]
    fn delete_forward_and_backspace_remove_one_character_each() {
        let mut d = doc();
        let mut tool = editing(&mut d, "abc");
        key(&mut tool, &mut d, "left", Modifiers::default());
        key(&mut tool, &mut d, "backspace", Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().text(), "ac");
        key(&mut tool, &mut d, "delete", Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().text(), "a");
    }

    #[test]
    fn backspace_removes_a_whole_grapheme() {
        // "e" + combining acute looks like one letter, so one backspace
        // should remove it, not peel off the accent.
        let mut d = doc();
        let mut tool = editing(&mut d, "x");
        {
            let s = tool.editing.as_mut().unwrap();
            s.stored.spec.text = "e\u{301}".into();
            s.caret = s.stored.spec.text.len();
            s.anchor = s.caret;
        }
        key(&mut tool, &mut d, "backspace", Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().text(), "");
    }

    #[test]
    fn ctrl_arrow_jumps_by_word() {
        let mut d = doc();
        let mut tool = editing(&mut d, "one two three");
        key(&mut tool, &mut d, "left", ctrl());
        assert_eq!(tool.editing.as_ref().unwrap().caret, 8, "start of 'three'");
        key(&mut tool, &mut d, "left", ctrl());
        assert_eq!(tool.editing.as_ref().unwrap().caret, 4, "start of 'two'");
    }

    #[test]
    fn rtl_word_arrows_follow_the_visual_direction() {
        let mut d = doc();
        let mut tool = editing(&mut d, "אבג דהו");
        select(&mut tool, 0, 0);
        key(&mut tool, &mut d, "left", ctrl());
        assert_eq!(tool.editing.as_ref().unwrap().caret, "אבג".len());
        key(&mut tool, &mut d, "left", ctrl());
        assert_eq!(tool.editing.as_ref().unwrap().caret, "אבג דהו".len());
        key(&mut tool, &mut d, "right", ctrl());
        assert_eq!(tool.editing.as_ref().unwrap().caret, "אבג ".len());
    }

    #[test]
    fn word_arrow_crosses_a_soft_wrap_affinity_boundary() {
        let mut d = doc();
        let mut tool = editing(&mut d, "abc def ghi");
        let session = tool.editing.as_mut().unwrap();
        session.stored.spec.wrap_width = Some(100.0);
        let spans = schist_text_engine::line_spans(&session.stored.spec);
        assert!(spans.len() > 1);
        session.caret = spans[0].end;
        session.anchor = session.caret;
        session.affinity = CaretAffinity::Upstream;
        key(&mut tool, &mut d, "right", ctrl());
        assert!(tool.editing.as_ref().unwrap().caret > spans[0].end);
    }

    #[test]
    fn up_and_down_keep_the_visual_column() {
        let mut d = doc();
        let mut tool = editing(&mut d, "long line\nlo");
        // Equal prefixes put column 2 at the same visual x in any font.
        key(&mut tool, &mut d, "up", Modifiers::default());
        assert_eq!(
            tool.editing.as_ref().unwrap().caret,
            2,
            "column 2 of line 1"
        );
        key(&mut tool, &mut d, "down", Modifiers::default());
        assert_eq!(tool.editing.as_ref().unwrap().caret, 12, "back to column 2");
    }

    #[test]
    fn an_unhandled_shortcut_is_not_swallowed() {
        // ctrl+s must still reach the app; only ctrl+a is ours.
        let mut d = doc();
        let mut tool = editing(&mut d, "hi");
        assert!(!key(&mut tool, &mut d, "s", ctrl()));
    }
}
