//! Semantic action recording and transactional replay. Actions never contain
//! file operations, absolute layer ids, callbacks, or serialized UI events.

use super::*;
use schist_i18n::{t, tf};
use schist_plugin_api::{CommandPlugin, FilterPlugin, FilterValues, NativeFilterBuffer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_STEPS: usize = 128;
const SCHEMA: u32 = 2;

/// Audited commands whose mutations are entirely represented in undo history.
pub fn command_supported(id: &str) -> bool {
    matches!(
        id,
        "select.all"
            | "select.deselect"
            | "select.inverse"
            | "edit.fill_foreground"
            | "edit.fill_background"
            | "layer.new"
            | "layer.duplicate"
            | "layer.flatten"
            | "layer.merge_down"
            | "layer.merge_visible"
            | "layer.rasterize"
    )
}

/// Deterministic, self-contained built-in filters. Resource-dependent and
/// random filters deliberately require an explicit future capability contract.
pub fn filter_supported(id: &str) -> bool {
    matches!(
        id,
        "filter.gaussian_blur"
            | "filter.box_blur"
            | "filter.motion_blur"
            | "filter.sharpen"
            | "filter.unsharp_mask"
            | "filter.median"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    SelectLayer {
        name: String,
    },
    Transform {
        params: schist_plugin_api::ActionTransform,
    },
    RawDevelopment {
        values: BTreeMap<String, f32>,
    },
    Stack {
        change: StackOperation,
    },
    Command {
        id: String,
        foreground: [f32; 4],
        background: [f32; 4],
    },
    Filter {
        id: String,
        values: BTreeMap<String, f32>,
    },
    AddAdjustment {
        params: schist_adjustments::Params,
    },
    SetAdjustment {
        params: schist_adjustments::Params,
    },
    PixelAdjustment {
        params: schist_adjustments::Params,
    },
}

/// Index identifies a position in the active layer's recipe. Expected IDs
/// reject a different stack instead of silently editing an unrelated effect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StackOperation {
    Add {
        effect: schist_core::filter_stack::FilterEffect,
    },
    Set {
        index: usize,
        effect: schist_core::filter_stack::FilterEffect,
    },
    Remove {
        index: usize,
        id: String,
    },
    Move {
        index: usize,
        to: usize,
        id: String,
    },
    Enable {
        index: usize,
        id: String,
        enabled: bool,
    },
    Bake,
}

impl StackOperation {
    pub fn effect_mut(&mut self) -> Option<&mut schist_core::filter_stack::FilterEffect> {
        match self {
            Self::Add { effect } | Self::Set { effect, .. } => Some(effect),
            _ => None,
        }
    }
}

impl Step {
    pub fn label(&self, registry: &PluginRegistry) -> String {
        match self {
            Self::SelectLayer { name } => format!("{}: {name}", t("menu.layer")),
            Self::Transform { params } => t(if params.selection {
                "tool.transform.selection.name"
            } else {
                "tool.transform.name"
            })
            .into(),
            Self::RawDevelopment { .. } => t("workspace.filters.raw_history").into(),
            Self::Stack { change } => {
                let label = t(match change {
                    StackOperation::Add { .. } => "filter_stack.add",
                    StackOperation::Set { .. } => "filter_stack.edit_history",
                    StackOperation::Remove { .. } => "filter_stack.remove",
                    StackOperation::Move { .. } => "filter_stack.title",
                    StackOperation::Enable { enabled, .. } => {
                        if *enabled {
                            "filter_stack.enable"
                        } else {
                            "filter_stack.disable"
                        }
                    }
                    StackOperation::Bake => "filter_stack.bake",
                });
                let target = match change {
                    StackOperation::Add { effect } => Some((None, effect.id.as_str())),
                    StackOperation::Set { index, effect } => {
                        Some((Some(*index), effect.id.as_str()))
                    }
                    StackOperation::Remove { index, id }
                    | StackOperation::Move { index, id, .. }
                    | StackOperation::Enable { index, id, .. } => Some((Some(*index), id.as_str())),
                    StackOperation::Bake => None,
                };
                if let Some((index, id)) = target {
                    let name = registry
                        .filters()
                        .find(|f| f.id() == id)
                        .map(|f| f.name())
                        .unwrap_or(id);
                    match index {
                        Some(index) => format!("{label}: {} · {name}", index + 1),
                        None => format!("{label}: {name}"),
                    }
                } else {
                    label.into()
                }
            }
            Self::Command { id, .. } => registry.command(id).map(|c| c.title).unwrap_or(id).into(),
            Self::Filter { id, .. } => registry
                .filters()
                .find(|f| f.id() == id)
                .map(|f| f.name())
                .unwrap_or(id)
                .into(),
            Self::AddAdjustment { params } => tf!(
                "actions.add_adjustment",
                name = crate::ui::adjustment_name(params.kind())
            ),
            Self::SetAdjustment { params } => tf!(
                "actions.set_adjustment",
                name = crate::ui::adjustment_name(params.kind())
            ),
            Self::PixelAdjustment { params } => tf!(
                "actions.pixel_adjustment",
                name = crate::ui::adjustment_name(params.kind())
            ),
        }
    }
    pub fn filter_parameters(&self) -> Option<(&str, &BTreeMap<String, f32>)> {
        match self {
            Self::Filter { id, values } => Some((id, values)),
            Self::RawDevelopment { values } => Some(("filter.camera_raw", values)),
            Self::Stack {
                change: StackOperation::Add { effect } | StackOperation::Set { effect, .. },
            } => Some((&effect.id, &effect.values)),
            _ => None,
        }
    }
    pub fn filter_parameters_mut(&mut self) -> Option<&mut BTreeMap<String, f32>> {
        match self {
            Self::Filter { values, .. } | Self::RawDevelopment { values } => Some(values),
            Self::Stack { change } => change.effect_mut().map(|effect| &mut effect.values),
            _ => None,
        }
    }
    pub fn adjustment_mut(&mut self) -> Option<&mut schist_adjustments::Params> {
        match self {
            Self::AddAdjustment { params }
            | Self::SetAdjustment { params }
            | Self::PixelAdjustment { params } => Some(params),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SavedAction {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionLibrary {
    schema: u32,
    pub actions: Vec<SavedAction>,
}
impl Default for ActionLibrary {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            actions: Vec::new(),
        }
    }
}
impl ActionLibrary {
    fn decode(text: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            text.len() <= 4 * 1024 * 1024,
            "{}",
            t("actions.library_too_large")
        );
        let mut library: Self = serde_json::from_str(text)?;
        anyhow::ensure!(
            (1..=SCHEMA).contains(&library.schema),
            "{}",
            t("actions.unknown_schema")
        );
        anyhow::ensure!(
            library.actions.len() <= 256,
            "{}",
            t("actions.library_too_large")
        );
        for action in &library.actions {
            validate_shape(action)?;
        }
        library.schema = SCHEMA;
        Ok(library)
    }
    pub fn load() -> anyhow::Result<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        let text = {
            let Some(path) = schist_folder().map(|p| p.join("actions.json")) else {
                return Ok(Self::default());
            };
            match std::fs::File::open(path) {
                Ok(file) => {
                    use std::io::Read;
                    let mut text = String::new();
                    file.take(4 * 1024 * 1024 + 1).read_to_string(&mut text)?;
                    text
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
                Err(e) => return Err(e.into()),
            }
        };
        #[cfg(target_arch = "wasm32")]
        let Some(text) = schist_app_platform::web::local_get("schist.actions.v1") else {
            return Ok(Self::default());
        };
        Self::decode(&text)
    }
    fn save(&self) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        // Validate the exact bytes before replacing the previous library.
        Self::decode(&json)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = schist_folder()
                .ok_or_else(|| anyhow::anyhow!("{}", t("actions.no_settings_folder")))?;
            save_library_file(&dir, &json)?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let storage = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .ok_or_else(|| anyhow::anyhow!("{}", t("actions.no_settings_folder")))?;
            storage
                .set_item("schist.actions.v1", &json)
                .map_err(|_| anyhow::anyhow!("{}", t("actions.storage_failed")))?;
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_library_file(dir: &std::path::Path, json: &str) -> anyhow::Result<()> {
    use std::io::Write;
    ActionLibrary::decode(json)?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(json.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(dir.join("actions.json"))?;
    Ok(())
}

#[derive(Default)]
pub struct Recorder {
    pub recording: bool,
    pub replaying: bool,
    document: Option<schist_core::DocumentId>,
    pub draft: Option<SavedAction>,
    pub selected_action: Option<usize>,
    /// The unsaved recording while a saved action is being inspected/edited.
    working_action: Option<SavedAction>,
    /// The last inserted adjustment can absorb its dialog's committed settings.
    added_adjustment: Option<(schist_core::LayerId, usize)>,
    pub batch_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
}
impl Recorder {
    fn select_action(&mut self, library: &ActionLibrary, index: Option<usize>) -> bool {
        if index == self.selected_action {
            return false;
        }
        let next = match index {
            Some(index) => {
                let Some(action) = library.actions.get(index) else {
                    return false;
                };
                Some(action.clone())
            }
            None => self.working_action.take(),
        };
        if self.selected_action.is_none() {
            self.working_action = self.draft.take();
        }
        self.draft = next;
        self.selected_action = index;
        true
    }

    fn mark_saved(&mut self, index: usize, action: SavedAction) {
        if self.selected_action.is_none() {
            self.working_action = Some(action.clone());
        }
        self.draft = Some(action);
        self.selected_action = Some(index);
    }

    fn fold_adjustment(
        &mut self,
        layer: schist_core::LayerId,
        params: &schist_adjustments::Params,
    ) -> bool {
        let Some((added, index)) = self.added_adjustment else {
            return false;
        };
        let Some(action) = self.draft.as_mut() else {
            return false;
        };
        if added != layer || index + 1 != action.steps.len() {
            return false;
        }
        let Some(Step::AddAdjustment { params: original }) = action.steps.get_mut(index) else {
            return false;
        };
        *original = params.clone();
        true
    }
    fn push(&mut self, document: Option<schist_core::DocumentId>, step: Step) -> bool {
        if !self.recording || self.replaying || document != self.document {
            return false;
        }
        let Some(draft) = self.draft.as_mut() else {
            return false;
        };
        if draft.steps.len() >= MAX_STEPS {
            self.recording = false;
            return false;
        }
        draft.steps.push(step);
        true
    }
}

fn validate_shape(action: &SavedAction) -> anyhow::Result<()> {
    anyhow::ensure!(
        !action.name.trim().is_empty() && action.name.len() <= 200,
        "{}",
        t("actions.invalid_name")
    );
    anyhow::ensure!(
        !action.steps.is_empty() && action.steps.len() <= MAX_STEPS,
        "{}",
        t("actions.invalid_steps")
    );
    for step in &action.steps {
        match step {
            Step::SelectLayer { name } => {
                anyhow::ensure!(
                    !name.is_empty() && name.len() <= 1024 && !name.contains('\0'),
                    "{}",
                    t("actions.invalid_parameters")
                );
            }
            Step::Transform { params } => {
                anyhow::ensure!(params.valid(), "{}", t("actions.invalid_parameters"));
            }
            Step::RawDevelopment { values } => {
                anyhow::ensure!(
                    values.len() == 15
                        && values.iter().all(|(key, value)| {
                            let range = match key.as_str() {
                                "exposure" => -5.0..=5.0,
                                "sharpening" => 0.0..=150.0,
                                "noise" => 0.0..=100.0,
                                "temperature" | "tint" | "contrast" | "highlights" | "shadows"
                                | "whites" | "blacks" | "clarity" | "dehaze" | "vibrance"
                                | "saturation" | "vignette" => -100.0..=100.0,
                                _ => return false,
                            };
                            value.is_finite() && range.contains(value)
                        }),
                    "{}",
                    t("actions.invalid_parameters")
                );
            }
            Step::Stack { change } => validate_stack_operation(change)?,
            Step::Command {
                id,
                foreground,
                background,
            } => {
                anyhow::ensure!(
                    command_supported(id),
                    "{}",
                    tf!("actions.unsupported", name = id)
                );
                anyhow::ensure!(
                    foreground
                        .iter()
                        .chain(background)
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                    "{}",
                    t("actions.invalid_parameters")
                );
            }
            Step::Filter { id, values } => {
                anyhow::ensure!(
                    filter_supported(id),
                    "{}",
                    tf!("actions.unsupported", name = id)
                );
                anyhow::ensure!(
                    values.len() <= 32 && values.values().all(|v| v.is_finite()),
                    "{}",
                    t("actions.invalid_parameters")
                );
            }
            Step::AddAdjustment { params }
            | Step::SetAdjustment { params }
            | Step::PixelAdjustment { params } => {
                validate_adjustment(params)?;
            }
        }
    }
    Ok(())
}

fn validate_stack_operation(change: &StackOperation) -> anyhow::Result<()> {
    use schist_core::filter_stack::MAX_EFFECTS;
    let valid_id = |id: &str| !id.is_empty() && id.len() <= 256;
    let valid = match change {
        StackOperation::Add { effect } | StackOperation::Set { effect, .. } => {
            valid_id(&effect.id)
                && effect.values.len() <= 256
                && effect
                    .values
                    .iter()
                    .all(|(key, value)| key.len() <= 256 && value.is_finite())
                && effect
                    .foreground
                    .iter()
                    .chain(&effect.background)
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && match change {
                    StackOperation::Set { index, .. } => *index < MAX_EFFECTS,
                    _ => true,
                }
        }
        StackOperation::Remove { index, id } | StackOperation::Enable { index, id, .. } => {
            *index < MAX_EFFECTS && valid_id(id)
        }
        StackOperation::Move { index, to, id } => {
            *index < MAX_EFFECTS && *to < MAX_EFFECTS && index != to && valid_id(id)
        }
        StackOperation::Bake => true,
    };
    anyhow::ensure!(valid, "{}", t("actions.invalid_parameters"));
    Ok(())
}

fn validate_adjustment(params: &schist_adjustments::Params) -> anyhow::Result<()> {
    use schist_adjustments::Params;
    let within = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
    let unit = |values: &[f32]| values.iter().all(|v| within(*v, 0.0, 1.0));
    for spec in params.param_specs() {
        anyhow::ensure!(
            within(spec.value, spec.min, spec.max),
            "{}",
            t("actions.invalid_parameters")
        );
    }
    let valid = match params {
        Params::Unsupported => false,
        Params::Levels(levels) => [&levels.rgb, &levels.red, &levels.green, &levels.blue]
            .iter()
            .all(|l| {
                unit(&[l.input_black, l.input_white, l.output_black, l.output_white])
                    && within(l.gamma, 0.1, 9.99)
            }),
        Params::Curves(curves) => [&curves.rgb, &curves.red, &curves.green, &curves.blue]
            .iter()
            .all(|c| {
                (2..=256).contains(&c.points.len())
                    && c.points.iter().all(|(x, y)| unit(&[*x, *y]))
                    && c.points.windows(2).all(|p| p[0].0 < p[1].0)
            }),
        Params::HueSaturation { ranges, .. } => {
            ranges.len() <= 6
                && ranges.iter().all(|r| {
                    r.bounds.iter().all(|v| within(*v, 0.0, 360.0))
                        && within(r.hue, -180.0, 180.0)
                        && within(r.saturation, -100.0, 100.0)
                        && within(r.lightness, -100.0, 100.0)
                })
        }
        Params::SolidColor { rgba } => unit(rgba),
        Params::PhotoFilter { color, .. } => unit(color),
        Params::GradientMap {
            from, to, stops, ..
        } => {
            unit(from)
                && unit(to)
                && stops.len() <= 256
                && stops
                    .iter()
                    .all(|(pos, color)| within(*pos, 0.0, 1.0) && unit(color))
                && stops.windows(2).all(|p| p[0].0 <= p[1].0)
        }
        Params::ChannelMixer { constant, .. } => constant.iter().all(|v| within(*v, -200.0, 200.0)),
        Params::BrightnessContrast { .. }
        | Params::BlackWhite { .. }
        | Params::Invert
        | Params::Posterize { .. }
        | Params::Threshold { .. }
        | Params::ColorBalance { .. }
        | Params::Vibrance { .. }
        | Params::Exposure { .. }
        | Params::SelectiveColor { .. }
        | Params::WhiteBalance { .. } => true,
    };
    anyhow::ensure!(valid, "{}", t("actions.invalid_parameters"));
    Ok(())
}

fn resolve_values(
    filter: &dyn FilterPlugin,
    values: &BTreeMap<String, f32>,
) -> anyhow::Result<FilterValues> {
    anyhow::ensure!(
        !filter.runs_out_of_process()
            && !filter.wants_backdrop()
            && !filter.wants_path()
            && filter.wants_map().is_none(),
        "{}",
        tf!("actions.unsupported", name = filter.id())
    );
    let specs = filter.params();
    anyhow::ensure!(
        values.len() == specs.len(),
        "{}",
        t("actions.invalid_parameters")
    );
    let mut result = FilterValues::default();
    for p in specs {
        let value = values
            .get(p.key)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("{}", t("actions.invalid_parameters")))?;
        anyhow::ensure!(
            value.is_finite() && (p.min..=p.max).contains(&value),
            "{}",
            t("actions.invalid_parameters")
        );
        result.set(p.key, value);
    }
    Ok(result)
}

/// Constructed on the UI thread, moved into a worker for gallery replay.
/// Commands come from the audited built-in provider even if a plug-in reused an id.
struct Runtime {
    commands: Vec<schist_plugin_api::Command>,
    filters: Vec<Arc<dyn FilterPlugin>>,
}
impl Runtime {
    fn new(registry: &PluginRegistry) -> Self {
        Self {
            commands: schist_commands_core::CoreCommandsPlugin.commands(),
            filters: registry
                .filters()
                .filter(|f| schist_plugin_api::filter_stack::eligible(*f))
                .filter_map(|f| registry.shared_filter(f.id()))
                .collect(),
        }
    }
    fn validate(&self, action: &SavedAction) -> anyhow::Result<()> {
        validate_shape(action)?;
        for step in &action.steps {
            let parameters = match step {
                Step::Filter { id, values } => Some((id.as_str(), values)),
                Step::RawDevelopment { values } => Some(("filter.camera_raw", values)),
                Step::Stack {
                    change: StackOperation::Add { effect } | StackOperation::Set { effect, .. },
                } => Some((effect.id.as_str(), &effect.values)),
                _ => None,
            };
            if let Some((id, values)) = parameters {
                let filter = self.filter(id)?;
                resolve_values(filter.as_ref(), values)?;
            }
        }
        Ok(())
    }
    fn filter(&self, id: &str) -> anyhow::Result<Arc<dyn FilterPlugin>> {
        self.filters
            .iter()
            .find(|f| f.id() == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{}", tf!("actions.unavailable", name = id)))
    }
    /// Replay against an isolated history; rollback restores the complete
    /// previous history (including redo/save point), then return one grouped edit.
    fn replay(&self, action: &SavedAction, doc: &mut Document) -> anyhow::Result<()> {
        self.validate(action)?;
        anyhow::ensure!(
            doc.active_channel.is_none(),
            "{}",
            t("actions.native_channel")
        );
        let mut scratch = schist_core::History::new();
        scratch.limit = usize::MAX;
        scratch.byte_limit = usize::MAX;
        let previous = std::mem::replace(&mut doc.history, scratch);
        let (active, selected, dirty) = (doc.active_layer, doc.selected.clone(), doc.dirty);
        let last_selection = doc.last_selection.clone();
        let mut outcome = Ok(());
        for (index, step) in action.steps.iter().enumerate() {
            if let Err(error) = self.run_step(step, doc) {
                outcome = Err(anyhow::anyhow!(
                    "{}",
                    tf!("actions.step_failed", n = index + 1, error = error)
                ));
                break;
            }
        }
        if outcome.is_err() {
            while doc.undo().is_some() {}
            doc.history = previous;
            doc.active_layer = active;
            doc.selected = selected;
            doc.dirty = dirty;
            doc.last_selection = last_selection;
            doc.damage_all();
            return outcome;
        }
        let ops = doc
            .history
            .entries()
            .iter()
            .flat_map(|edit| edit.ops.clone())
            .collect::<Vec<_>>();
        doc.history = previous;
        if !ops.is_empty() {
            doc.history.push(schist_core::Edit {
                name: tf!("actions.history", name = action.name),
                ops,
            });
            doc.dirty = true;
        }
        doc.damage_all();
        Ok(())
    }
    fn run_stack(&self, change: &StackOperation, doc: &mut Document) -> anyhow::Result<()> {
        use schist_core::filter_stack;
        let layer = doc
            .active_layer
            .and_then(|id| doc.tree.find(id))
            .ok_or_else(|| anyhow::anyhow!("{}", t("filter_stack.unavailable")))?;
        anyhow::ensure!(
            !layer.locked
                && layer.as_raster().is_some()
                && layer.shape.is_none()
                && !layer.extras.iter().any(|b| b.key == *b"PsTx"),
            "{}",
            t("filter_stack.unavailable")
        );
        let id = layer.id;
        if matches!(change, StackOperation::Bake) {
            anyhow::ensure!(
                filter_stack::has_stack(layer),
                "{}",
                t("actions.command_no_change")
            );
            let extras = filter_stack::without_stack(&layer.extras);
            let mut edit = doc.begin_edit(t("filter_stack.bake_history"));
            edit.set_extras(id, extras);
            edit.commit();
            return Ok(());
        }
        let (source, mut stack) = super::filter_stack::source_and_stack(layer, doc.canvas_rect())?;
        let expected = match change {
            StackOperation::Set { index, effect } => Some((*index, effect.id.as_str())),
            StackOperation::Remove { index, id }
            | StackOperation::Move { index, id, .. }
            | StackOperation::Enable { index, id, .. } => Some((*index, id.as_str())),
            _ => None,
        };
        if let Some((index, expected)) = expected {
            anyhow::ensure!(
                stack
                    .effects
                    .get(index)
                    .is_some_and(|effect| effect.id == expected),
                "{}",
                t("actions.invalid_parameters")
            );
        }
        let before = stack.clone();
        match change {
            StackOperation::Add { effect } => stack.effects.push(effect.clone()),
            StackOperation::Set { index, effect } => stack.effects[*index] = effect.clone(),
            StackOperation::Remove { index, .. } => {
                stack.effects.remove(*index);
            }
            StackOperation::Move { index, to, .. } => {
                anyhow::ensure!(
                    *to < stack.effects.len(),
                    "{}",
                    t("actions.invalid_parameters")
                );
                let effect = stack.effects.remove(*index);
                stack.effects.insert(*to, effect);
            }
            StackOperation::Enable { index, enabled, .. } => {
                stack.effects[*index].enabled = *enabled
            }
            StackOperation::Bake => unreachable!(),
        }
        anyhow::ensure!(stack != before, "{}", t("actions.command_no_change"));
        stack.validate()?;
        let filtered = schist_plugin_api::filter_stack::render_with(
            |id| self.filter(id).ok(),
            &stack,
            &source,
            doc.depth,
            doc.icc_profile.clone(),
        )?;
        let mut smart = layer.smart.clone();
        let tiles = if let Some(smart) = smart.as_mut() {
            smart.source = filtered.clone();
            smart.source_bounds = smart.source.content_bounds();
            smart.render(doc.depth, doc.canvas_rect())
        } else {
            stack.place(&filtered, doc.depth, doc.canvas_rect())
        };
        let extras = if stack.effects.is_empty() {
            filter_stack::without_stack(&layer.extras)
        } else {
            stack.blocks_with_render(layer, &source, &filtered)?
        };
        let mut edit = doc.begin_edit(t("filter_stack.edit_history"));
        edit.replace_layer_render(id, tiles);
        if smart.is_some() {
            edit.set_smart_object(id, smart);
        }
        edit.set_extras(id, extras);
        edit.commit();
        Ok(())
    }
    fn run_step(&self, step: &Step, doc: &mut Document) -> anyhow::Result<()> {
        match step {
            Step::SelectLayer { name } => {
                let ids: Vec<_> = doc
                    .tree
                    .iter()
                    .filter(|layer| layer.name == *name)
                    .map(|layer| layer.id)
                    .collect();
                anyhow::ensure!(
                    ids.len() == 1,
                    "{}",
                    tf!("actions.unavailable", name = name)
                );
                doc.active_layer = Some(ids[0]);
                doc.selected = ids;
            }
            Step::Transform { params } => {
                anyhow::ensure!(
                    schist_tools_transform::replay_transform(doc, *params),
                    "{}",
                    t("actions.needs_pixels")
                );
            }
            Step::RawDevelopment { values } => {
                let filter = self.filter("filter.camera_raw")?;
                let values = resolve_values(filter.as_ref(), values)?;
                let settings = super::filters::settings_from_values(&values);
                let id = doc
                    .active_layer
                    .ok_or_else(|| anyhow::anyhow!("{}", t("actions.needs_pixels")))?;
                let layer = doc
                    .tree
                    .find(id)
                    .ok_or_else(|| anyhow::anyhow!("{}", t("actions.needs_pixels")))?;
                anyhow::ensure!(
                    !layer.locked && layer.as_raster().is_some(),
                    "{}",
                    t("actions.layer_locked")
                );
                let mut raw = layer
                    .raw
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("{}", t("actions.needs_pixels")))?;
                let developed = super::filters::render_raw_capture(
                    raw.source.clone(),
                    settings,
                    schist_codecs_common::raw::RawQuality::Best,
                    filter,
                    values,
                )?;
                anyhow::ensure!(
                    developed.width == doc.width as usize
                        && developed.height == doc.height as usize,
                    "{}",
                    tf!(
                        "workspace.filters.raw_size_changed",
                        w = developed.width,
                        h = developed.height
                    )
                );
                let mut tiles = schist_core::TileMap::default();
                schist_core::blit_rgba_f32(
                    &mut tiles,
                    doc.depth,
                    doc.canvas_rect(),
                    &developed.rgba,
                );
                raw.settings = settings;
                let mut edit = doc.begin_edit(t("workspace.filters.raw_history"));
                edit.replace_layer_tiles(id, tiles);
                edit.set_raw_development(id, Some(raw));
                edit.commit();
            }
            Step::Stack { change } => self.run_stack(change, doc)?,
            Step::Command {
                id,
                foreground,
                background,
            } => {
                let command =
                    self.commands.iter().find(|c| c.id == id).ok_or_else(|| {
                        anyhow::anyhow!("{}", tf!("actions.unavailable", name = id))
                    })?;
                let mut state = EditorState {
                    foreground: Rgba::new(
                        foreground[0],
                        foreground[1],
                        foreground[2],
                        foreground[3],
                    ),
                    background: Rgba::new(
                        background[0],
                        background[1],
                        background[2],
                        background[3],
                    ),
                    ..Default::default()
                };
                let before = doc.history.entries().len();
                let mut ctx = CommandCtx {
                    doc,
                    state: &mut state,
                    refusal: None,
                };
                (command.run)(&mut ctx);
                if let Some(reason) = ctx.refusal {
                    anyhow::bail!("{reason}");
                }
                anyhow::ensure!(
                    doc.history.entries().len() > before,
                    "{}",
                    t("actions.command_no_change")
                );
            }
            Step::Filter { id, values } => {
                let filter = self
                    .filters
                    .iter()
                    .find(|f| f.id() == id)
                    .expect("validated filter");
                let values = resolve_values(filter.as_ref(), values)?;
                edit_pixels(doc, filter.name(), |buffer| {
                    filter.apply_native_with(buffer, &values, &Default::default());
                    if let Some(error) = filter.last_error() {
                        anyhow::bail!("{error}");
                    }
                    Ok(())
                })?;
            }
            Step::AddAdjustment { params } => {
                let mut layer = Layer::new_raster(crate::ui::adjustment_name(params.kind()));
                layer.kind = schist_core::LayerKind::Adjustment(schist_core::AdjustmentData {
                    kind: params.kind(),
                    raw: Vec::new(),
                    params_json: Some(serde_json::to_string(params)?),
                });
                let id = layer.id;
                let path = doc
                    .active_layer
                    .and_then(|id| doc.tree.path_of(id))
                    .map(|mut p| {
                        *p.0.last_mut().expect("layer path") += 1;
                        p
                    })
                    .unwrap_or_else(|| schist_core::LayerPath(vec![doc.tree.layers.len()]));
                let mut edit = doc.begin_edit(t("actions.adjustment_history"));
                edit.insert_layer(path, layer);
                edit.commit();
                doc.active_layer = Some(id);
            }
            Step::SetAdjustment { params } => {
                let layer = doc
                    .active_layer
                    .ok_or_else(|| anyhow::anyhow!("{}", t("actions.needs_adjustment")))?;
                let Some(schist_core::LayerKind::Adjustment(data)) =
                    doc.tree.find(layer).map(|l| &l.kind)
                else {
                    anyhow::bail!("{}", t("actions.needs_adjustment"));
                };
                anyhow::ensure!(
                    data.kind == params.kind(),
                    "{}",
                    t("actions.needs_adjustment")
                );
                let before = (data.params_json.clone(), data.raw.clone());
                let after = (Some(serde_json::to_string(params)?), Vec::new());
                let mut edit = doc.begin_edit(t("actions.adjustment_history"));
                edit.record_adjustment_params(layer, before, after);
                edit.commit();
            }
            Step::PixelAdjustment { params } => {
                edit_pixels(doc, crate::ui::adjustment_name(params.kind()), |buffer| {
                    buffer.process_rgba(|rgba, _, _| params.apply_buffer(rgba));
                    Ok(())
                })?;
            }
        }
        Ok(())
    }
}

fn edit_pixels(
    doc: &mut Document,
    label: &str,
    apply: impl FnOnce(&mut NativeFilterBuffer) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let id = doc
        .active_layer
        .ok_or_else(|| anyhow::anyhow!("{}", t("actions.needs_pixels")))?;
    let layer = doc
        .tree
        .find(id)
        .ok_or_else(|| anyhow::anyhow!("{}", t("actions.needs_pixels")))?;
    anyhow::ensure!(!layer.locked, "{}", t("actions.layer_locked"));
    let raster = layer
        .as_raster()
        .ok_or_else(|| anyhow::anyhow!("{}", t("actions.needs_pixels")))?;
    let region = if doc.selection.is_empty() {
        layer.content_bounds()
    } else {
        doc.selection.bounds()
    }
    .intersect(&doc.canvas_rect());
    anyhow::ensure!(!region.is_empty(), "{}", t("actions.needs_pixels"));
    let original = raster.tiles.clone();
    let mut buffer = NativeFilterBuffer::read(&original, region, doc.mode, doc.icc_profile.clone());
    apply(&mut buffer)?;
    let out = buffer.write(&original, region, doc.depth, &doc.selection);
    let mut edit = doc.begin_edit(label);
    edit.replace_layer_tiles(id, out);
    edit.commit();
    Ok(())
}

fn validate_recording_context(doc: &Document, step: &Step) -> anyhow::Result<()> {
    anyhow::ensure!(
        doc.active_channel.is_none(),
        "{}",
        t("actions.native_channel")
    );
    anyhow::ensure!(
        !(matches!(step, Step::PixelAdjustment { .. })
            && matches!(doc.mode, ColorMode::Cmyk | ColorMode::Lab)),
        "{}",
        t("actions.native_adjustment")
    );
    Ok(())
}

impl Workspace {
    pub fn select_recorded_action(&mut self, index: Option<usize>) {
        self.commit_focused_field();
        if !self
            .action_recorder
            .select_action(&self.action_library, index)
        {
            return;
        }
        let draft_name = self
            .action_recorder
            .draft
            .as_ref()
            .map(|action| action.name.clone())
            .unwrap_or_default();
        self.update_modal(|modal| {
            if let Modal::RecordedActions {
                selected,
                step,
                name,
            } = modal
            {
                *selected = index;
                *step = None;
                *name = draft_name;
            }
        });
    }

    pub fn open_actions(&mut self, cx: &mut Context<Self>) {
        if self
            .modal
            .as_ref()
            .is_some_and(|m| !matches!(m, Modal::RecordedActions { .. }))
        {
            self.status = t("actions.finish_dialog").into();
            cx.notify();
            return;
        }
        if matches!(self.editor.active_tool, "transform" | "transform.selection") {
            // Do not leave an activation-time snapshot alive across recording
            // or replay: a later tool switch could overwrite the action result.
            self.commit_gesture_with_async(false, cx);
        }
        self.open_modal(
            Modal::RecordedActions {
                selected: self.action_recorder.selected_action,
                step: None,
                name: self
                    .action_recorder
                    .draft
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_default(),
            },
            cx,
        );
    }
    pub fn start_action_recording(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = &self.doc else {
            self.status = t("actions.needs_document").into();
            cx.notify();
            return;
        };
        if doc.active_channel.is_some() {
            self.status = t("actions.native_channel").into();
            cx.notify();
            return;
        }
        self.action_recorder = Recorder {
            recording: true,
            document: Some(doc.id),
            draft: Some(SavedAction {
                name: t("actions.untitled").into(),
                steps: Vec::new(),
            }),
            ..Default::default()
        };
        self.close_modal(cx);
        self.status = t("actions.recording").into();
        cx.notify();
    }
    pub fn stop_action_recording(&mut self, cx: &mut Context<Self>) {
        self.action_recorder.recording = false;
        self.open_actions(cx);
    }
    pub(super) fn record_command_action(&mut self, id: &str) {
        if command_supported(id) {
            let rgba = |v: Rgba| [v.r, v.g, v.b, v.a];
            self.record_action_step(Step::Command {
                id: id.into(),
                foreground: rgba(self.editor.foreground),
                background: rgba(self.editor.background),
            });
        } else if self.action_recorder.recording {
            self.status = tf!("actions.skipped", name = id).into();
        }
    }
    pub(super) fn record_filter_action(&mut self, id: &str, values: &FilterValues) {
        if filter_supported(id) {
            self.record_action_step(Step::Filter {
                id: id.into(),
                values: values.0.iter().map(|(k, v)| ((*k).into(), *v)).collect(),
            });
        } else if self.action_recorder.recording {
            self.status = tf!("actions.skipped", name = id).into();
        }
    }
    /// A transform owns its activation-time layer snapshot. Finish it before a
    /// later recorded operation can change that layer or its pixels.
    pub(super) fn commit_recording_transform(&mut self, cx: &mut Context<Self>) {
        if self.action_recorder.recording
            && matches!(self.editor.active_tool, "transform" | "transform.selection")
        {
            self.commit_gesture(cx);
        }
    }
    pub(super) fn record_selected_action_layer(&mut self) {
        if !self.action_recorder.recording {
            return;
        }
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let Some(layer) = doc.active_layer.and_then(|id| doc.tree.find(id)) else {
            return;
        };
        if layer.name.is_empty()
            || layer.name.len() > 1024
            || layer.name.contains('\0')
            || doc.selected_layers().len() != 1
            || doc
                .tree
                .iter()
                .filter(|other| other.name == layer.name)
                .count()
                != 1
        {
            // Continuing after an unrepresentable selection would silently
            // replay subsequent edits on the previous layer.
            self.action_recorder.recording = false;
            self.status = tf!(
                "actions.not_recorded",
                error = t("actions.invalid_parameters")
            )
            .into();
            return;
        }
        self.record_action_step(Step::SelectLayer {
            name: layer.name.clone(),
        });
    }
    pub(super) fn record_action_step(&mut self, step: Step) {
        if !self.action_recorder.recording || self.action_recorder.replaying {
            return;
        }
        if let Some(doc) = &self.doc {
            if let Err(error) = validate_recording_context(doc, &step) {
                self.status = tf!("actions.not_recorded", error = error).into();
                return;
            }
        }
        if let Err(error) = validate_shape(&SavedAction {
            name: t("actions.untitled").into(),
            steps: vec![step.clone()],
        }) {
            self.status = tf!("actions.not_recorded", error = error).into();
            return;
        }
        let document = self.doc.as_ref().map(|d| d.id);
        if self.action_recorder.recording && self.action_recorder.document != document {
            self.action_recorder.recording = false;
            self.status = t("actions.document_changed").into();
            return;
        }
        if self.action_recorder.push(document, step) {
            self.status = tf!(
                "actions.recorded",
                n = self
                    .action_recorder
                    .draft
                    .as_ref()
                    .map_or(0, |a| a.steps.len())
            )
            .into();
        } else if self
            .action_recorder
            .draft
            .as_ref()
            .is_some_and(|a| a.steps.len() >= MAX_STEPS)
        {
            self.status = t("actions.step_limit").into();
        }
    }
    pub(super) fn record_added_adjustment(
        &mut self,
        layer: schist_core::LayerId,
        params: &schist_adjustments::Params,
    ) {
        let before = self
            .action_recorder
            .draft
            .as_ref()
            .map_or(0, |a| a.steps.len());
        self.record_action_step(Step::AddAdjustment {
            params: params.clone(),
        });
        if self.action_recorder.recording
            && self
                .action_recorder
                .draft
                .as_ref()
                .is_some_and(|a| a.steps.len() == before + 1)
        {
            let index = self
                .action_recorder
                .draft
                .as_ref()
                .map_or(0, |a| a.steps.len())
                .saturating_sub(1);
            self.action_recorder.added_adjustment = Some((layer, index));
        }
    }
    pub(super) fn record_adjustment_settings(
        &mut self,
        layer: schist_core::LayerId,
        params: &schist_adjustments::Params,
    ) {
        if !self.action_recorder.recording
            || self.action_recorder.replaying
            || self.doc.as_ref().map(|d| d.id) != self.action_recorder.document
        {
            return;
        }
        if let Err(error) = validate_adjustment(params) {
            self.status = tf!("actions.not_recorded", error = error).into();
            return;
        }
        if self.action_recorder.fold_adjustment(layer, params) {
            return;
        }
        if self.doc.as_ref().and_then(|d| d.active_layer) == Some(layer) {
            self.record_action_step(Step::SetAdjustment {
                params: params.clone(),
            });
        } else {
            self.status = t("actions.inactive_adjustment").into();
        }
    }
    pub fn save_recorded_action(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::RecordedActions { selected, name, .. }) = self.modal.clone() else {
            return;
        };
        let Some(mut action) = self.action_recorder.draft.clone() else {
            return;
        };
        action.name = name.trim().into();
        let result = (|| {
            Runtime::new(&self.registry).validate(&action)?;
            let mut library = self.action_library.clone();
            if let Some(index) = selected {
                *library
                    .actions
                    .get_mut(index)
                    .ok_or_else(|| anyhow::anyhow!("{}", t("actions.unavailable_action")))? =
                    action.clone();
            } else {
                library.actions.push(action.clone());
            }
            library.save()?;
            self.action_library = library;
            let index = selected.unwrap_or(self.action_library.actions.len() - 1);
            self.action_recorder.mark_saved(index, action);
            self.update_modal(|m| {
                if let Modal::RecordedActions { selected, .. } = m {
                    *selected = Some(index);
                }
            });
            Ok::<(), anyhow::Error>(())
        })();
        self.status = match result {
            Ok(()) => t("actions.saved").into(),
            Err(e) => tf!("actions.failed", error = e).into(),
        };
        cx.notify();
    }
    pub fn delete_recorded_action(&mut self, index: usize, cx: &mut Context<Self>) {
        let mut library = self.action_library.clone();
        if index >= library.actions.len() {
            return;
        }
        library.actions.remove(index);
        match library.save() {
            Ok(()) => {
                self.action_library = library;
                self.action_recorder
                    .select_action(&self.action_library, None);
                self.open_actions(cx);
            }
            Err(e) => self.status = tf!("actions.failed", error = e).into(),
        }
        cx.notify();
    }
    pub fn replay_recorded_action(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        if self.action_recorder.recording || self.action_recorder.replaying {
            return;
        }
        let Some(action) = self.action_recorder.draft.clone() else {
            return;
        };
        let runtime = Runtime::new(&self.registry);
        let Some(doc) = self.doc.as_mut() else {
            self.status = t("actions.needs_document").into();
            cx.notify();
            return;
        };
        self.action_recorder.replaying = true;
        let result = runtime.replay(&action, doc);
        self.action_recorder.replaying = false;
        self.status = match result {
            Ok(()) => tf!("actions.played", name = action.name).into(),
            Err(e) => tf!("actions.failed", error = e).into(),
        };
        self.close_modal(cx);
        self.after_change(cx);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Workspace {
    pub fn replay_action_gallery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_focused_field();
        if self.action_recorder.recording || self.action_recorder.batch_cancel.is_some() {
            return;
        }
        let Some(action) = self.action_recorder.draft.clone() else {
            return;
        };
        let photos = self.library.selected.clone();
        if photos.is_empty() {
            self.status = t("actions.select_photos").into();
            cx.notify();
            return;
        }
        if let Err(error) = Runtime::new(&self.registry).validate(&action) {
            self.status = tf!("actions.failed", error = error).into();
            cx.notify();
            return;
        }
        let rx = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some(t("actions.choose_output").into()),
            },
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = rx.await else {
                return;
            };
            let Some(dir) = dirs.pop() else {
                return;
            };
            this.update_in(cx, |ws, _window, cx| {
                ws.run_action_gallery(photos, action, dir, cx)
            })
            .ok();
        })
        .detach();
    }

    fn run_action_gallery(
        &mut self,
        photos: Vec<PathBuf>,
        action: SavedAction,
        dir: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let total = photos.len();
        let codecs = self.registry.shared_codecs();
        let filters = Runtime::new(&self.registry).filters;
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.action_recorder.batch_cancel = Some(cancel.clone());
        self.open_modal(
            Modal::RecordedActionBatch {
                done: 0,
                total,
                outputs: Vec::new(),
                failures: Vec::new(),
                finished: false,
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let mut outputs = Vec::new();
            let mut failures = Vec::new();
            for (index, path) in photos.into_iter().enumerate() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let runtime = Runtime {
                    commands: schist_commands_core::CoreCommandsPlugin.commands(),
                    filters: filters.clone(),
                };
                let (job_codecs, job_action, job_dir, job_path) =
                    (codecs.clone(), action.clone(), dir.clone(), path.clone());
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        process_action_photo(
                            &job_codecs,
                            &runtime,
                            &job_path,
                            &job_action,
                            &job_dir,
                        )
                    })
                    .await;
                match result {
                    Ok(out) => outputs.push(out),
                    Err(error) => failures.push((path, error.to_string())),
                }
                if this
                    .update(cx, |ws, cx| {
                        ws.update_modal(|m| {
                            if let Modal::RecordedActionBatch {
                                done,
                                outputs: shown,
                                failures: errors,
                                ..
                            } = m
                            {
                                *done = index + 1;
                                *shown = outputs.clone();
                                *errors = failures.clone();
                            }
                        });
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            this.update(cx, |ws, cx| {
                ws.action_recorder.batch_cancel = None;
                ws.status = tf!(
                    "actions.batch_summary",
                    saved = outputs.len(),
                    failed = failures.len(),
                    total = total
                )
                .into();
                ws.open_modal(
                    Modal::RecordedActionBatch {
                        done: outputs.len() + failures.len(),
                        total,
                        outputs,
                        failures,
                        finished: true,
                    },
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }
}

/// Decode a gallery edit if present, replay in memory, and create a new PSD.
/// `persist_noclobber` prevents collisions, including concurrent batch writers.
#[cfg(not(target_arch = "wasm32"))]
fn process_action_photo(
    codecs: &[Arc<dyn schist_plugin_api::CodecPlugin>],
    runtime: &Runtime,
    path: &std::path::Path,
    action: &SavedAction,
    dir: &std::path::Path,
) -> anyhow::Result<PathBuf> {
    let source = schist_gallery::backing_psd(path)
        .filter(|p| p.exists())
        .unwrap_or_else(|| path.to_path_buf());
    let mut doc = super::decode_file(codecs, &source)?;
    if doc.active_layer.is_none() {
        doc.active_layer = doc
            .tree
            .layers
            .iter()
            .rev()
            .find(|l| l.as_raster().is_some())
            .map(|l| l.id);
    }
    runtime.replay(action, &mut doc)?;
    schist_compositor::restyle_layers(&mut doc.tree.layers, &mut Vec::new());
    let writer = codecs
        .iter()
        .find(|c| c.can_export() && c.extensions().contains(&"psd"))
        .ok_or_else(|| anyhow::anyhow!("{}", t("actions.no_psd_writer")))?;
    let bytes = writer.export(&doc)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "photo".into());
    write_action_copy(dir, &format!("{stem}-action"), &bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_action_copy(dir: &std::path::Path, stem: &str, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    for index in 1..=100_000 {
        let path = dir.join(if index == 1 {
            format!("{stem}.psd")
        } else {
            format!("{stem}-{index}.psd")
        });
        match tmp.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                tmp = error.file
            }
            Err(error) => return Err(error.error.into()),
        }
    }
    anyhow::bail!("{}", t("actions.no_output_name"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(id: &str) -> Step {
        Step::Command {
            id: id.into(),
            foreground: [0.25, 0.5, 0.75, 1.0],
            background: [1.0; 4],
        }
    }
    fn action(steps: Vec<Step>) -> SavedAction {
        SavedAction {
            name: "Recorded test".into(),
            steps,
        }
    }
    fn document() -> Document {
        let mut doc = Document::new("test", 2, 2, Depth::Eight);
        let mut layer = Layer::new_raster("pixels");
        blit_rgba8(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::Eight,
            IntRect::from_size(2, 2),
            &[20, 40, 60, 255].repeat(4),
        );
        doc.push_layer(layer);
        doc.mark_saved();
        doc
    }
    fn pixels(doc: &Document) -> Vec<u8> {
        schist_compositor::composite_region_rgba8(doc, doc.canvas_rect())
    }
    fn runtime() -> Runtime {
        Runtime::new(&PluginRegistry::new())
    }

    #[test]
    fn selecting_working_action_restores_its_name_and_edited_steps() {
        let mut working = action(vec![command("select.all"), command("layer.duplicate")]);
        working.name = "Unsaved recording".into();
        let saved = action(vec![command("select.deselect")]);
        let library = ActionLibrary {
            actions: vec![saved.clone()],
            ..Default::default()
        };
        let mut recorder = Recorder {
            draft: Some(working.clone()),
            ..Default::default()
        };

        assert!(recorder.select_action(&library, Some(0)));
        assert_eq!(recorder.draft.as_ref(), Some(&saved));
        recorder.draft.as_mut().unwrap().steps.clear();
        // Selecting the same entry must not discard its in-progress edits.
        assert!(!recorder.select_action(&library, Some(0)));
        assert!(recorder.draft.as_ref().unwrap().steps.is_empty());
        assert!(recorder.select_action(&library, None));
        assert_eq!(recorder.selected_action, None);
        assert_eq!(recorder.draft.as_ref(), Some(&working));
        assert_eq!(library.actions, vec![saved]);

        // Edits made after returning to the working draft survive another visit.
        working.steps.remove(0);
        recorder.draft = Some(working.clone());
        assert!(recorder.select_action(&library, Some(0)));
        assert!(recorder.select_action(&library, None));
        assert_eq!(recorder.draft, Some(working));
    }

    #[test]
    fn selecting_working_action_without_a_recording_clears_saved_draft() {
        let library = ActionLibrary {
            actions: vec![action(vec![command("select.all")])],
            ..Default::default()
        };
        let mut recorder = Recorder::default();
        assert!(recorder.select_action(&library, Some(0)));
        assert!(!recorder.select_action(&library, Some(99)));
        assert_eq!(recorder.selected_action, Some(0));
        assert!(recorder.select_action(&library, None));
        assert!(recorder.draft.is_none());
        assert_eq!(recorder.selected_action, None);
    }

    #[test]
    fn saving_an_action_keeps_the_working_recording_available() {
        let working = action(vec![command("select.all")]);
        let mut recorder = Recorder {
            draft: Some(working.clone()),
            ..Default::default()
        };
        let mut library = ActionLibrary {
            actions: vec![working.clone()],
            ..Default::default()
        };
        recorder.mark_saved(0, working.clone());
        // Updating the saved action must not overwrite the separate working copy.
        let changed = action(vec![command("select.deselect")]);
        library.actions[0] = changed.clone();
        recorder.mark_saved(0, changed);
        assert!(recorder.select_action(&library, None));
        assert_eq!(recorder.draft, Some(working));
    }

    #[test]
    fn recording_requires_same_document_and_never_records_during_replay() {
        let id = document().id;
        let mut recorder = Recorder {
            recording: true,
            document: Some(id),
            draft: Some(action(Vec::new())),
            ..Default::default()
        };
        assert!(recorder.push(Some(id), command("select.all")));
        recorder.replaying = true;
        assert!(!recorder.push(Some(id), command("select.deselect")));
        recorder.replaying = false;
        assert!(!recorder.push(Some(document().id), command("select.deselect")));
        recorder.recording = false;
        assert!(!recorder.push(Some(id), command("select.deselect")));
        assert_eq!(recorder.draft.unwrap().steps, [command("select.all")]);
    }

    #[test]
    fn adjustment_settings_fold_only_into_an_adjacent_insertion() {
        let doc = document();
        let layer = doc.active_layer.unwrap();
        let initial = schist_adjustments::Params::BrightnessContrast {
            brightness: 0.0,
            contrast: 0.0,
        };
        let changed = schist_adjustments::Params::BrightnessContrast {
            brightness: 20.0,
            contrast: 0.0,
        };
        let mut recorder = Recorder {
            recording: true,
            document: Some(doc.id),
            draft: Some(action(vec![Step::AddAdjustment {
                params: initial.clone(),
            }])),
            added_adjustment: Some((layer, 0)),
            ..Default::default()
        };
        assert!(recorder.fold_adjustment(layer, &changed));
        recorder.draft.as_mut().unwrap().steps[0] = Step::AddAdjustment {
            params: initial.clone(),
        };
        recorder.push(Some(doc.id), command("layer.duplicate"));
        assert!(!recorder.fold_adjustment(layer, &changed));
        assert_eq!(
            recorder.draft.unwrap().steps[0],
            Step::AddAdjustment { params: initial }
        );
    }

    #[test]
    fn unchanged_adjustment_has_no_committed_edit_to_record() {
        let mut doc = document();
        runtime()
            .replay(
                &action(vec![Step::AddAdjustment {
                    params: schist_adjustments::Params::Invert,
                }]),
                &mut doc,
            )
            .unwrap();
        let layer = doc.active_layer.unwrap();
        let Some(schist_core::LayerKind::Adjustment(data)) = doc.tree.find(layer).map(|l| &l.kind)
        else {
            panic!("adjustment inserted")
        };
        let original = (data.params_json.clone(), data.raw.clone());
        let after = (
            Some(serde_json::to_string(&schist_adjustments::Params::Invert).unwrap()),
            Vec::new(),
        );
        let revision = doc.revision;
        let entries = doc.history.entries().len();
        let mut edit = doc.begin_edit("settings");
        edit.record_adjustment_params(layer, original, after);
        assert!(
            !edit.commit(),
            "the recorder's committed-edit hook must remain idle"
        );
        assert_eq!(doc.revision, revision);
        assert_eq!(doc.history.entries().len(), entries);
    }

    #[test]
    fn library_roundtrips_owned_parameters_and_rejects_unsafe_operations() {
        let original = action(vec![
            command("edit.fill_foreground"),
            Step::Filter {
                id: "filter.gaussian_blur".into(),
                values: BTreeMap::from([("radius".into(), 4.25)]),
            },
            Step::AddAdjustment {
                params: schist_adjustments::Params::Exposure {
                    exposure: 0.75,
                    offset: 0.01,
                    gamma: 1.25,
                },
            },
        ]);
        let library = ActionLibrary {
            schema: SCHEMA,
            actions: vec![original.clone()],
        };
        let json = serde_json::to_string(&library).unwrap();
        assert_eq!(ActionLibrary::decode(&json).unwrap().actions, [original]);
        for id in [
            "edit.undo",
            "edit.redo",
            "edit.paste",
            "edit.cut",
            "layer.delete",
            "file.save",
            "app.quit",
        ] {
            assert!(
                runtime().validate(&action(vec![command(id)])).is_err(),
                "{id}"
            );
        }
        assert!(ActionLibrary::decode(&json.replace("\"schema\":2", "\"schema\":999")).is_err());
        assert!(runtime()
            .validate(&action(vec![Step::Filter {
                id: "filter.add_noise".into(),
                values: BTreeMap::new()
            }]))
            .is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn library_file_survives_reload_and_rejects_invalid_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let library = ActionLibrary {
            schema: SCHEMA,
            actions: vec![action(vec![command("select.all")])],
        };
        let json = serde_json::to_string(&library).unwrap();
        save_library_file(dir.path(), &json).unwrap();
        let path = dir.path().join("actions.json");
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            ActionLibrary::decode(&saved).unwrap().actions,
            library.actions
        );
        assert!(save_library_file(dir.path(), "{broken").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        save_library_file(
            dir.path(),
            &serde_json::to_string(&ActionLibrary::default()).unwrap(),
        )
        .unwrap();
        assert!(
            ActionLibrary::decode(&std::fs::read_to_string(path).unwrap())
                .unwrap()
                .actions
                .is_empty()
        );
    }

    #[test]
    fn replay_is_deterministic_and_one_undo_restores_every_step() {
        let mut a = document();
        let mut b = document();
        let original = pixels(&a);
        let recipe = action(vec![
            command("select.all"),
            command("edit.fill_foreground"),
            Step::AddAdjustment {
                params: schist_adjustments::Params::Invert,
            },
        ]);
        runtime().replay(&recipe, &mut a).unwrap();
        runtime().replay(&recipe, &mut b).unwrap();
        assert_eq!(pixels(&a), pixels(&b));
        assert_ne!(pixels(&a), original);
        assert_eq!(a.history.entries().len(), 1);
        a.undo().unwrap();
        assert_eq!(pixels(&a), original);
        assert!(a.history.at_saved());
        assert!(!a.dirty);
        a.redo().unwrap();
        assert_eq!(pixels(&a), pixels(&b));
    }

    #[test]
    fn failed_step_rolls_back_pixels_reselect_and_previous_redo_branch() {
        let mut doc = document();
        runtime()
            .replay(&action(vec![command("edit.fill_foreground")]), &mut doc)
            .unwrap();
        doc.undo().unwrap();
        doc.selection.select_all(doc.canvas_rect());
        doc.last_selection = None;
        doc.selected = vec![doc.active_layer.unwrap()];
        let original = pixels(&doc);
        let active = doc.active_layer;
        let selected = doc.selected.clone();
        let redo_name = doc.history.redo_name().unwrap().to_string();
        let recipe = action(vec![
            command("select.deselect"),
            command("edit.fill_background"),
            Step::SetAdjustment {
                params: schist_adjustments::Params::Invert,
            },
        ]);
        let error = runtime().replay(&recipe, &mut doc).unwrap_err().to_string();
        assert!(error.contains("3"));
        assert_eq!(pixels(&doc), original);
        assert_eq!(doc.active_layer, active);
        assert_eq!(doc.selected, selected);
        assert_eq!(doc.selection.bounds(), doc.canvas_rect());
        assert!(doc.last_selection.is_none());
        assert!(doc.history.at_saved());
        assert_eq!(doc.history.redo_name(), Some(redo_name.as_str()));
        assert!(!doc.history.can_undo());
        assert!(!doc.dirty);
    }

    #[test]
    fn missing_filter_and_noop_command_fail_without_altering_document() {
        let mut doc = document();
        let before = pixels(&doc);
        let recipe = action(vec![
            command("edit.fill_foreground"),
            Step::Filter {
                id: "filter.gaussian_blur".into(),
                values: BTreeMap::new(),
            },
        ]);
        assert!(runtime().replay(&recipe, &mut doc).is_err());
        assert_eq!(pixels(&doc), before);
        assert!(!doc.history.can_undo());
        assert!(runtime()
            .replay(&action(vec![command("select.inverse")]), &mut doc)
            .is_err());
    }

    #[test]
    fn hidden_adjustment_fields_and_native_recording_are_validated() {
        let mut levels = schist_adjustments::Levels::default();
        levels.red.gamma = 0.0;
        assert!(validate_adjustment(&schist_adjustments::Params::Levels(levels)).is_err());
        let mut curves = schist_adjustments::Curves::default();
        curves.blue.points.reverse();
        assert!(validate_adjustment(&schist_adjustments::Params::Curves(curves)).is_err());
        assert!(
            validate_adjustment(&schist_adjustments::Params::SolidColor {
                rgba: [0.0, 0.0, 0.0, 2.0]
            })
            .is_err()
        );
        assert!(
            validate_adjustment(&schist_adjustments::Params::PhotoFilter {
                color: [-1.0; 3],
                density: 10.0,
                preserve_luminosity: true
            })
            .is_err()
        );
        let mut doc = document();
        doc.active_channel = Some(0);
        assert!(validate_recording_context(&doc, &command("edit.fill_foreground")).is_err());
        assert!(runtime()
            .replay(&action(vec![command("edit.fill_foreground")]), &mut doc)
            .is_err());
        doc.active_channel = None;
        doc.mode = ColorMode::Cmyk;
        assert!(validate_recording_context(
            &doc,
            &Step::PixelAdjustment {
                params: schist_adjustments::Params::Invert
            }
        )
        .is_err());
    }

    struct ControlledFilter {
        fail: bool,
    }
    impl FilterPlugin for ControlledFilter {
        fn id(&self) -> &'static str {
            "filter.gaussian_blur"
        }
        fn name(&self) -> &'static str {
            "Controlled test filter"
        }
        fn params(&self) -> Vec<schist_plugin_api::FilterParam> {
            vec![schist_plugin_api::FilterParam {
                key: "gain",
                label: "Gain",
                min: 0.0,
                max: 1.0,
                default: 0.5,
                suffix: "",
                choices: &[],
            }]
        }
        fn apply(&self, pixels: &mut [f32], _: usize, _: usize, values: &FilterValues) {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel[0] *= values.get("gain");
            }
        }
        fn last_error(&self) -> Option<String> {
            self.fail.then(|| "injected failure".into())
        }
    }

    #[test]
    fn filter_values_are_validated_and_filter_failure_is_transactional() {
        let filter = Step::Filter {
            id: "filter.gaussian_blur".into(),
            values: BTreeMap::from([("gain".into(), 0.5)]),
        };
        let mut registry = PluginRegistry::new();
        registry.register_filter(Box::new(ControlledFilter { fail: false }));
        let rt = Runtime::new(&registry);
        let mut doc = document();
        let before = pixels(&doc);
        rt.replay(&action(vec![filter.clone()]), &mut doc).unwrap();
        assert_eq!(pixels(&doc)[0], before[0] / 2);
        doc.undo().unwrap();
        assert_eq!(pixels(&doc), before);
        assert!(rt
            .validate(&action(vec![Step::Filter {
                id: "filter.gaussian_blur".into(),
                values: BTreeMap::from([("gain".into(), 2.0)])
            }]))
            .is_err());
        assert!(rt
            .validate(&action(vec![Step::Filter {
                id: "filter.gaussian_blur".into(),
                values: BTreeMap::from([("unknown".into(), 0.5)])
            }]))
            .is_err());
        let mut registry = PluginRegistry::new();
        registry.register_filter(Box::new(ControlledFilter { fail: true }));
        assert!(Runtime::new(&registry)
            .replay(
                &action(vec![command("edit.fill_background"), filter]),
                &mut doc
            )
            .is_err());
        assert_eq!(pixels(&doc), before);
        assert!(doc.history.can_redo());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gallery_replay_uses_saved_edit_and_never_overwrites_originals_or_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("photo.psd");
        let source = document();
        let original_bytes = schist_codec_psd::write_psd(&source).unwrap();
        std::fs::write(&original, &original_bytes).unwrap();
        let sidecar = schist_gallery::backing_psd(&original).unwrap();
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        let mut edited = document();
        runtime()
            .replay(&action(vec![command("edit.fill_background")]), &mut edited)
            .unwrap();
        let edited_bytes = schist_codec_psd::write_psd(&edited).unwrap();
        std::fs::write(&sidecar, &edited_bytes).unwrap();
        let codec: Vec<Arc<dyn schist_plugin_api::CodecPlugin>> =
            vec![Arc::new(schist_codecs_common::PsdCodec)];
        let recipe = action(vec![Step::PixelAdjustment {
            params: schist_adjustments::Params::Invert,
        }]);
        let first =
            process_action_photo(&codec, &runtime(), &original, &recipe, dir.path()).unwrap();
        let first_bytes = std::fs::read(&first).unwrap();
        let second =
            process_action_photo(&codec, &runtime(), &original, &recipe, dir.path()).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), first_bytes);
        assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
        assert_eq!(std::fs::read(&sidecar).unwrap(), edited_bytes);
        let result = schist_codec_psd::read_psd(&first_bytes).unwrap();
        assert_eq!(pixels(&result), [0, 0, 0, 255].repeat(4));
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        let failure = action(vec![Step::SetAdjustment {
            params: schist_adjustments::Params::Invert,
        }]);
        assert!(process_action_photo(&codec, &runtime(), &original, &failure, dir.path()).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
    }
    fn transform() -> schist_plugin_api::ActionTransform {
        schist_plugin_api::ActionTransform {
            selection: false,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
            offset_x: 0.25,
            offset_y: 0.0,
            interpolation: 0,
        }
    }

    fn stack_effect(gain: f32) -> schist_core::filter_stack::FilterEffect {
        schist_core::filter_stack::FilterEffect {
            id: "filter.gaussian_blur".into(),
            enabled: true,
            values: [("gain".into(), gain)].into(),
            foreground: [0.0, 0.0, 0.0, 1.0],
            background: [1.0; 4],
        }
    }
    fn stack_step(change: StackOperation) -> Step {
        Step::Stack { change }
    }
    fn stack_runtime() -> Runtime {
        let mut registry = PluginRegistry::new();
        registry.register_filter(Box::new(ControlledFilter { fail: false }));
        Runtime::new(&registry)
    }

    #[test]
    fn extended_schema_roundtrips_and_loads_version_one_libraries() {
        let library = ActionLibrary {
            schema: SCHEMA,
            actions: vec![action(vec![
                Step::SelectLayer {
                    name: "Artwork".into(),
                },
                Step::Transform {
                    params: transform(),
                },
                stack_step(StackOperation::Add {
                    effect: stack_effect(0.5),
                }),
                stack_step(StackOperation::Move {
                    index: 1,
                    to: 0,
                    id: "filter.gaussian_blur".into(),
                }),
            ])],
        };
        let json = serde_json::to_string(&library).unwrap();
        assert_eq!(
            ActionLibrary::decode(&json).unwrap().actions,
            library.actions
        );
        assert!(!json.contains("layer_id") && !json.contains("pointer"));
        let old = r#"{"schema":1,"actions":[{"name":"old","steps":[{"operation":"command","id":"select.all","foreground":[0,0,0,1],"background":[1,1,1,1]}]}]}"#;
        let decoded = ActionLibrary::decode(old).unwrap();
        assert_eq!(decoded.schema, SCHEMA);
        runtime()
            .replay(&decoded.actions[0], &mut document())
            .unwrap();
    }

    #[test]
    fn extended_schema_rejects_nonfinite_unbounded_and_unknown_parameters() {
        for value in [f32::NAN, f32::INFINITY, -f32::INFINITY, 101.0, 0.0] {
            let mut params = transform();
            params.scale_x = value;
            assert!(validate_shape(&action(vec![Step::Transform { params }])).is_err());
        }
        let mut params = transform();
        params.offset_x = 11.0;
        assert!(validate_shape(&action(vec![Step::Transform { params }])).is_err());
        params = transform();
        params.rotation = f32::NAN;
        assert!(validate_shape(&action(vec![Step::Transform { params }])).is_err());
        params = transform();
        params.interpolation = 3;
        assert!(validate_shape(&action(vec![Step::Transform { params }])).is_err());
        assert!(validate_shape(&action(vec![Step::SelectLayer {
            name: "x".repeat(1025)
        }]))
        .is_err());
        assert!(
            validate_shape(&action(vec![stack_step(StackOperation::Remove {
                index: 256,
                id: "x".into()
            })]))
            .is_err()
        );
        let mut effect = stack_effect(0.5);
        effect.foreground[0] = f32::NAN;
        assert!(validate_shape(&action(vec![stack_step(StackOperation::Add { effect })])).is_err());
        let json = serde_json::to_string(&action(vec![Step::Transform {
            params: transform(),
        }]))
        .unwrap();
        assert!(serde_json::from_str::<SavedAction>(
            &json.replace("\"selection\":false", "\"selection\":false,\"layer_id\":42")
        )
        .is_err());
    }

    #[test]
    fn selection_by_name_searches_groups_and_rejects_ambiguous_or_missing_names() {
        let mut doc = document();
        let mut group = Layer::new_group("Folder");
        let mut child = Layer::new_raster("Artwork");
        child.as_raster_mut().unwrap().tiles =
            doc.tree.layers[0].as_raster().unwrap().tiles.clone();
        let child_id = child.id;
        if let schist_core::LayerKind::Group(data) = &mut group.kind {
            data.children.push(child);
        }
        doc.push_layer(group);
        let recipe = action(vec![
            Step::SelectLayer {
                name: "Artwork".into(),
            },
            command("edit.fill_background"),
        ]);
        runtime().replay(&recipe, &mut doc).unwrap();
        assert_eq!(doc.active_layer, Some(child_id));
        assert_eq!(doc.selected, [child_id]);
        assert_eq!(
            doc.tree
                .find(child_id)
                .unwrap()
                .as_raster()
                .unwrap()
                .tiles
                .pixel(0, 0),
            Rgba::WHITE
        );
        doc.undo().unwrap();
        let active = doc.active_layer;
        doc.push_layer(Layer::new_raster("Artwork"));
        assert!(runtime().replay(&recipe, &mut doc).is_err());
        let selection = doc.active_layer;
        assert!(runtime()
            .replay(
                &action(vec![Step::SelectLayer {
                    name: "Missing".into()
                }]),
                &mut doc
            )
            .is_err());
        assert_eq!(doc.active_layer, selection);
        assert!(active.is_some());
    }

    #[test]
    fn relative_transform_uses_each_target_canvas_and_undo_restores_source() {
        for width in [8, 16] {
            let mut doc = document();
            doc.width = width;
            doc.height = 8;
            let before = pixels(&doc);
            runtime()
                .replay(
                    &action(vec![Step::Transform {
                        params: transform(),
                    }]),
                    &mut doc,
                )
                .unwrap();
            let tiles = &doc
                .tree
                .find(doc.active_layer.unwrap())
                .unwrap()
                .as_raster()
                .unwrap()
                .tiles;
            assert_eq!(tiles.pixel(width as i32 / 4, 0).a, 1.0);
            assert_eq!(tiles.pixel(0, 0).a, 0.0);
            assert_eq!(doc.history.entries().len(), 1);
            doc.undo().unwrap();
            assert_eq!(pixels(&doc), before);
        }
        let mut doc = document();
        doc.width = 8;
        doc.selection.select_all(IntRect::from_size(2, 2));
        let before = pixels(&doc);
        let mut params = transform();
        params.selection = true;
        runtime()
            .replay(&action(vec![Step::Transform { params }]), &mut doc)
            .unwrap();
        assert_eq!(doc.selection.bounds(), IntRect::new(2, 0, 4, 2));
        assert_eq!(pixels(&doc), before);
        doc.undo().unwrap();
        assert_eq!(doc.selection.bounds(), IntRect::from_size(2, 2));
    }

    #[test]
    fn stack_actions_preserve_transformed_placement_through_reedit_remove_and_undo() {
        use schist_core::filter_stack::{has_stack, read_source, FilterStack};
        let mut doc = document();
        doc.width = 8;
        let id = doc.active_layer.unwrap();
        let before = pixels(&doc);
        let recipe = action(vec![
            stack_step(StackOperation::Add {
                effect: stack_effect(0.5),
            }),
            Step::Transform {
                params: transform(),
            },
            stack_step(StackOperation::Set {
                index: 0,
                effect: stack_effect(0.75),
            }),
        ]);
        stack_runtime().replay(&recipe, &mut doc).unwrap();
        let layer = doc.tree.find(id).unwrap();
        assert!(FilterStack::read(layer)
            .unwrap()
            .unwrap()
            .placement
            .is_some());
        assert_eq!(read_source(layer).unwrap().pixel(0, 0).to_u8()[0], 20);
        assert_eq!(layer.as_raster().unwrap().tiles.pixel(0, 0).a, 0.0);
        assert_eq!(layer.as_raster().unwrap().tiles.pixel(2, 0).to_u8()[0], 15);
        assert_eq!(doc.history.entries().len(), 1);
        stack_runtime()
            .replay(
                &action(vec![stack_step(StackOperation::Remove {
                    index: 0,
                    id: "filter.gaussian_blur".into(),
                })]),
                &mut doc,
            )
            .unwrap();
        let layer = doc.tree.find(id).unwrap();
        assert!(!has_stack(layer));
        assert_eq!(layer.as_raster().unwrap().tiles.pixel(0, 0).a, 0.0);
        assert_eq!(layer.as_raster().unwrap().tiles.pixel(2, 0).to_u8()[0], 20);
        doc.undo().unwrap();
        assert_eq!(
            doc.tree
                .find(id)
                .unwrap()
                .as_raster()
                .unwrap()
                .tiles
                .pixel(2, 0)
                .to_u8()[0],
            15
        );
        doc.undo().unwrap();
        assert_eq!(pixels(&doc), before);
        doc.redo().unwrap();
        assert_eq!(
            doc.tree
                .find(id)
                .unwrap()
                .as_raster()
                .unwrap()
                .tiles
                .pixel(2, 0)
                .to_u8()[0],
            15
        );
    }

    #[test]
    fn stack_add_set_move_disable_remove_are_source_based_and_one_undo() {
        use schist_core::filter_stack::{has_stack, read_source, FilterStack};
        let mut doc = document();
        let before = pixels(&doc);
        let id = doc.active_layer.unwrap();
        let recipe = action(vec![
            stack_step(StackOperation::Add {
                effect: stack_effect(0.5),
            }),
            stack_step(StackOperation::Add {
                effect: stack_effect(0.25),
            }),
            stack_step(StackOperation::Move {
                index: 1,
                to: 0,
                id: "filter.gaussian_blur".into(),
            }),
            stack_step(StackOperation::Set {
                index: 1,
                effect: stack_effect(0.75),
            }),
            stack_step(StackOperation::Enable {
                index: 0,
                id: "filter.gaussian_blur".into(),
                enabled: false,
            }),
        ]);
        stack_runtime().replay(&recipe, &mut doc).unwrap();
        assert_eq!(doc.history.entries().len(), 1);
        let layer = doc.tree.find(id).unwrap();
        let stack = FilterStack::read(layer).unwrap().unwrap();
        assert_eq!(stack.effects[0].values["gain"], 0.25);
        assert!(!stack.effects[0].enabled);
        assert_eq!(stack.effects[1].values["gain"], 0.75);
        assert_eq!(
            read_source(layer).unwrap().pixel(0, 0).to_u8()[0],
            before[0]
        );
        assert_eq!(pixels(&doc)[0], 15);
        stack_runtime()
            .replay(
                &action(vec![
                    stack_step(StackOperation::Remove {
                        index: 1,
                        id: "filter.gaussian_blur".into(),
                    }),
                    stack_step(StackOperation::Remove {
                        index: 0,
                        id: "filter.gaussian_blur".into(),
                    }),
                ]),
                &mut doc,
            )
            .unwrap();
        assert!(!has_stack(doc.tree.find(id).unwrap()));
        assert_eq!(pixels(&doc), before);
        doc.undo().unwrap();
        assert!(has_stack(doc.tree.find(id).unwrap()));
        doc.undo().unwrap();
        assert!(!has_stack(doc.tree.find(id).unwrap()));
        assert_eq!(pixels(&doc), before);
    }

    #[test]
    fn missing_stack_effect_can_be_disabled_or_removed_and_failure_rolls_back() {
        use schist_core::filter_stack::FilterStack;
        let mut doc = document();
        let id = doc.active_layer.unwrap();
        let mut stack = FilterStack::new(doc.canvas_rect());
        let mut effect = stack_effect(0.5);
        effect.id = "missing".into();
        stack.effects.push(effect);
        let layer = doc.tree.find(id).unwrap();
        let extras = stack
            .blocks(layer, &layer.as_raster().unwrap().tiles)
            .unwrap();
        doc.tree.find_mut(id).unwrap().extras = extras;
        runtime()
            .replay(
                &action(vec![stack_step(StackOperation::Enable {
                    index: 0,
                    id: "missing".into(),
                    enabled: false,
                })]),
                &mut doc,
            )
            .unwrap();
        assert!(
            !FilterStack::read(doc.tree.find(id).unwrap())
                .unwrap()
                .unwrap()
                .effects[0]
                .enabled
        );
        let before = pixels(&doc);
        assert!(runtime()
            .replay(
                &action(vec![
                    command("select.all"),
                    stack_step(StackOperation::Enable {
                        index: 0,
                        id: "missing".into(),
                        enabled: true
                    })
                ]),
                &mut doc
            )
            .is_err());
        assert!(doc.selection.is_empty());
        assert_eq!(pixels(&doc), before);
        runtime()
            .replay(
                &action(vec![stack_step(StackOperation::Remove {
                    index: 0,
                    id: "missing".into(),
                })]),
                &mut doc,
            )
            .unwrap();
        assert!(FilterStack::read(doc.tree.find(id).unwrap())
            .unwrap()
            .is_none());
    }

    #[test]
    fn extended_replay_rolls_back_history_selection_reselect_names_and_stack() {
        let mut doc = document();
        doc.width = 8;
        runtime()
            .replay(&action(vec![command("edit.fill_foreground")]), &mut doc)
            .unwrap();
        doc.undo().unwrap();
        doc.selection.select_all(IntRect::from_size(2, 2));
        doc.last_selection = Some(doc.selection.clone());
        let before = pixels(&doc);
        let selected = doc.selected.clone();
        let active = doc.active_layer;
        let redo = doc.history.redo_name().unwrap().to_string();
        let recipe = action(vec![
            command("select.deselect"),
            Step::SelectLayer {
                name: "pixels".into(),
            },
            stack_step(StackOperation::Add {
                effect: stack_effect(0.5),
            }),
            Step::Transform {
                params: transform(),
            },
            stack_step(StackOperation::Remove {
                index: 9,
                id: "filter.gaussian_blur".into(),
            }),
        ]);
        assert!(stack_runtime().replay(&recipe, &mut doc).is_err());
        assert_eq!(pixels(&doc), before);
        assert_eq!(doc.selected, selected);
        assert_eq!(doc.active_layer, active);
        assert_eq!(doc.selection.bounds(), IntRect::from_size(2, 2));
        assert_eq!(
            doc.last_selection.as_ref().unwrap().bounds(),
            IntRect::from_size(2, 2)
        );
        assert_eq!(doc.history.redo_name(), Some(redo.as_str()));
        assert!(doc.history.at_saved());
        assert!(!schist_core::filter_stack::has_stack(
            doc.tree.find(active.unwrap()).unwrap()
        ));
        assert!(!doc.dirty);
    }

    fn raw_fixture() -> (Runtime, Document, Step) {
        use schist_plugin_api::CodecPlugin;
        let mut registry = PluginRegistry::new();
        registry.register_filter(Box::new(schist_filters_core::camera_raw::CameraRaw));
        let filter = registry.shared_filter("filter.camera_raw").unwrap();
        let mut values = FilterValues::defaults(&filter.params());
        values.set("exposure", 0.75);
        let doc = schist_codecs_common::raw::RawCodec
            .import(include_bytes!(
                "../../../codec-raw/tests/fixtures/gpu-development.dng"
            ))
            .unwrap();
        (
            Runtime::new(&registry),
            doc,
            Step::RawDevelopment {
                values: values.0.into_iter().map(|(k, v)| (k.into(), v)).collect(),
            },
        )
    }

    #[test]
    fn raw_development_replays_original_capture_and_undo_restores_settings() {
        let (rt, mut doc, step) = raw_fixture();
        let id = doc.active_layer.unwrap();
        let before = pixels(&doc);
        let original = doc.tree.find(id).unwrap().raw.clone().unwrap();
        rt.replay(&action(vec![step.clone()]), &mut doc).unwrap();
        assert_eq!(
            doc.tree
                .find(id)
                .unwrap()
                .raw
                .as_ref()
                .unwrap()
                .settings
                .exposure,
            0.75
        );
        let rendered = pixels(&doc);
        assert_ne!(rendered, before);
        rt.replay(&action(vec![step.clone()]), &mut doc).unwrap();
        assert_eq!(
            pixels(&doc),
            rendered,
            "development starts from sensor bytes, never the rendered layer"
        );
        doc.undo().unwrap();
        doc.undo().unwrap();
        assert_eq!(pixels(&doc), before);
        assert_eq!(doc.tree.find(id).unwrap().raw.as_ref().unwrap(), &original);
        assert!(rt
            .replay(&action(vec![step.clone()]), &mut document())
            .is_err());
        let Step::RawDevelopment { mut values } = step else {
            unreachable!()
        };
        for invalid in [f32::NAN, f32::INFINITY, 6.0] {
            values.insert("exposure".into(), invalid);
            assert!(rt
                .validate(&action(vec![Step::RawDevelopment {
                    values: values.clone()
                }]))
                .is_err());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gallery_extended_action_preserves_raw_source_and_writes_only_new_copy() {
        let (rt, source, raw_step) = raw_fixture();
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("capture.psd");
        let original_bytes = schist_codec_psd::write_psd(&source).unwrap();
        std::fs::write(&original, &original_bytes).unwrap();
        let name = source
            .tree
            .find(source.active_layer.unwrap())
            .unwrap()
            .name
            .clone();
        let recipe = action(vec![
            Step::SelectLayer { name },
            raw_step,
            Step::Transform {
                params: transform(),
            },
        ]);
        let codecs: Vec<Arc<dyn schist_plugin_api::CodecPlugin>> =
            vec![Arc::new(schist_codecs_common::PsdCodec)];
        let out = process_action_photo(&codecs, &rt, &original, &recipe, dir.path()).unwrap();
        assert_ne!(out, original);
        assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
        let rendered = schist_codec_psd::read_psd(&std::fs::read(out).unwrap()).unwrap();
        let result_raw = rendered.tree.layers[0].raw.as_ref().unwrap();
        assert_eq!(result_raw.settings.exposure, 0.75);
        assert_eq!(
            result_raw.source,
            source.tree.layers[0].raw.as_ref().unwrap().source
        );
    }
}
