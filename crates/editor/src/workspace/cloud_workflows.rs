//! Saved workflows synchronize independently of documents and open editing drafts.
use super::*;
use anyhow::{ensure, Result};
use schist_cloud::{
    self as remote,
    gallery::Workflow,
    workflows::{merge, Libraries},
};
use schist_i18n::{t, tf};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) struct State {
    ticks: u16,
    running: bool,
    key: Option<String>,
    base: Libraries,
    pub ready: Option<Event>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            ticks: 400,
            running: false,
            key: None,
            base: BTreeMap::new(),
            ready: None,
        }
    }
}
pub(super) struct Event {
    sent: Libraries,
    result: std::result::Result<Vec<Workflow>, String>,
}
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    base: Libraries,
}

impl Workspace {
    pub(crate) fn cloud_workflows_changed(&mut self) {
        self.cloud.workflows.ticks = 400;
    }
    fn workflow_libraries(&self) -> Result<Libraries> {
        let mut recipes = crate::export_recipes::Book::load()?;
        for recipe in &mut recipes.recipes {
            recipe.destination = PathBuf::new();
        }
        recipes.selected = 0;
        Ok(BTreeMap::from([
            ("brushes".into(), serde_json::to_value(&self.brush_library)?),
            (
                "actions".into(),
                serde_json::to_value(&self.action_library)?,
            ),
            ("export_recipes".into(), serde_json::to_value(&recipes)?),
        ]))
    }
    fn apply_workflows(&mut self, libraries: &Libraries) -> Result<()> {
        let encode = |key: &str| -> Result<String> {
            Ok(serde_json::to_string(libraries.get(key).ok_or_else(
                || anyhow::anyhow!(t("common.unsupported_format")),
            )?)?)
        };
        // Validate every collection before replacing any local file.
        let brushes = schist_app_settings::brushes::BrushLibrary::from_json(&encode("brushes")?)
            .ok_or_else(|| anyhow::anyhow!(t("common.unsupported_format")))?;
        let actions = super::recorded_actions::ActionLibrary::decode(&encode("actions")?)?;
        let mut recipes = crate::export_recipes::Book::parse(&encode("export_recipes")?)?;
        let local = crate::export_recipes::Book::load()?;
        for recipe in &mut recipes.recipes {
            recipe.destination = local
                .recipes
                .iter()
                .find(|r| r.name == recipe.name)
                .map(|r| r.destination.clone())
                .unwrap_or_default();
        }
        let selected = self
            .action_recorder
            .selected_action
            .and_then(|i| self.action_library.actions.get(i))
            .map(|a| a.name.clone());
        ensure!(brushes.save(), t("common.failed"));
        actions.save()?;
        recipes.save()?;
        self.brush_library = brushes;
        self.action_library = actions;
        self.action_recorder.selected_action = selected.and_then(|name| {
            self.action_library
                .actions
                .iter()
                .position(|a| a.name == name)
        });
        Ok(())
    }
    pub(super) fn cloud_workflows_tick(&mut self, cx: &mut Context<Self>) {
        if !self.cloud.connected
            || !self
                .cloud
                .capabilities
                .as_ref()
                .is_some_and(|c| c.supports_gallery("workflows"))
        {
            return;
        }
        // A popup's row indices and an unsaved action draft must stay stable.
        if self.modal.is_some()
            || self.focused_field.is_some()
            || self.action_recorder.recording
            || self.action_recorder.replaying
        {
            return;
        }
        if let Some(event) = self.cloud.workflows.ready.take() {
            self.cloud.workflows.running = false;
            let result = (|| -> Result<()> {
                let workflows = event.result.map_err(anyhow::Error::msg)?;
                let local = self.workflow_libraries()?;
                let mut base = BTreeMap::new();
                let mut applied = BTreeMap::new();
                for workflow in workflows {
                    let value = merge(
                        &workflow.kind,
                        event.sent.get(&workflow.kind),
                        &local[&workflow.kind],
                        &workflow.data,
                    )?;
                    base.insert(workflow.kind.clone(), workflow.data);
                    applied.insert(workflow.kind, value);
                }
                if applied != local {
                    self.apply_workflows(&applied)?;
                }
                if applied != base {
                    self.cloud_workflows_changed();
                }
                if let Some(key) = &self.cloud.workflows.key {
                    save_checkpoint(key, &Checkpoint { base: base.clone() })?;
                }
                self.cloud.workflows.base = base;
                Ok(())
            })();
            if let Err(error) = result {
                self.cloud_error(tf!("actions.failed", error = error));
            }
            cx.notify();
        }
        if self.cloud.workflows.running {
            return;
        }
        self.cloud.workflows.ticks = self.cloud.workflows.ticks.saturating_add(1);
        if self.cloud.workflows.ticks < 400 {
            return;
        }
        self.cloud.workflows.ticks = 0;
        let Some(account) = self
            .cloud
            .capabilities
            .as_ref()
            .and_then(|c| c.account_id.as_deref())
        else {
            return;
        };
        let Some(connection) = &self.cloud.account else {
            return;
        };
        let key =
            remote::transfer::sha256_hex(format!("{}\n{account}", connection.domain).as_bytes());
        if self.cloud.workflows.key.as_ref() != Some(&key) {
            match load_checkpoint(&key) {
                Ok(base) => self.cloud.workflows.base = base,
                Err(error) => {
                    self.cloud_error(error.to_string());
                    return;
                }
            }
            self.cloud.workflows.key = Some(key);
        }
        let local = match self.workflow_libraries() {
            Ok(local) => local,
            Err(error) => {
                self.cloud_error(error.to_string());
                return;
            }
        };
        let Some(client) = &self.cloud.client else {
            return;
        };
        let (handle, sender, epoch, base) = (
            client.handle.clone(),
            self.cloud.sender.clone(),
            self.cloud.epoch,
            self.cloud.workflows.base.clone(),
        );
        self.cloud.workflows.running = true;
        remote::runtime::spawn(async move {
            let result = sync(&handle, &base, &local)
                .await
                .map_err(|e| e.to_string());
            let _ = sender.send(super::cloud::Job::Workflows {
                epoch,
                event: Event {
                    sent: local,
                    result,
                },
            });
        });
    }
}
async fn sync(
    handle: &remote::Handle,
    base: &Libraries,
    local: &Libraries,
) -> Result<Vec<Workflow>> {
    // Independent per-library compare-and-swap writes. On a conflict, fetch again
    // and replay the same three-way merge, preserving a simultaneous device save.
    let mut acknowledged = base.clone();
    for attempt in 0..3 {
        let remote = handle.workflows().await?;
        let mut saved = Vec::new();
        let mut retry = false;
        for (kind, local) in local {
            let existing = remote.iter().find(|w| &w.kind == kind);
            let empty = empty_library(kind);
            let data = merge(
                kind,
                acknowledged.get(kind),
                local,
                existing.map(|w| &w.data).unwrap_or(&empty),
            )?;
            validate(kind, &data)?;
            let workflow = Workflow {
                kind: kind.clone(),
                revision: existing.map_or(0, |w| w.revision),
                data,
            };
            if existing.is_some_and(|w| w.data == workflow.data) {
                saved.push(workflow);
                continue;
            }
            match handle.save_workflow(&workflow).await {
                Ok(result) => {
                    acknowledged.insert(kind.clone(), local.clone());
                    saved.push(result);
                }
                Err(error) if error.to_string().starts_with("conflict:") && attempt < 2 => {
                    retry = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if !retry {
            return Ok(saved);
        }
    }
    anyhow::bail!(t("library.similar.stale"))
}
fn empty_library(kind: &str) -> serde_json::Value {
    match kind {
        "brushes" => serde_json::json!({"presets":[]}),
        "actions" => serde_json::json!({"schema":2,"actions":[]}),
        _ => serde_json::json!({"version":1,"recipes":[],"selected":0}),
    }
}
fn validate(kind: &str, value: &serde_json::Value) -> Result<()> {
    let text = serde_json::to_string(value)?;
    match kind {
        "brushes" => ensure!(
            schist_app_settings::brushes::BrushLibrary::from_json(&text).is_some(),
            t("common.unsupported_format")
        ),
        "actions" => {
            super::recorded_actions::ActionLibrary::decode(&text)?;
        }
        _ => {
            crate::export_recipes::Book::parse(&text)?;
        }
    }
    Ok(())
}
fn load_checkpoint(key: &str) -> Result<Libraries> {
    #[cfg(not(target_arch = "wasm32"))]
    let text = {
        use std::io::Read as _;
        let Some(dir) = schist_app_settings::schist_folder() else {
            return Ok(BTreeMap::new());
        };
        match std::fs::File::open(dir.join(format!("workflow-sync-{key}.json"))) {
            Ok(file) => {
                let mut text = String::new();
                file.take(32 * 1024 * 1024 + 1).read_to_string(&mut text)?;
                Some(text)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        }
    };
    #[cfg(target_arch = "wasm32")]
    let text = crate::web::local_get(&format!("schist.workflow-sync.{key}"));
    let Some(text) = text else {
        return Ok(BTreeMap::new());
    };
    ensure!(
        text.len() <= 32 * 1024 * 1024,
        t("actions.library_too_large")
    );
    Ok(serde_json::from_str::<Checkpoint>(&text)?.base)
}
fn save_checkpoint(key: &str, checkpoint: &Checkpoint) -> Result<()> {
    let text = serde_json::to_string(checkpoint)?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write as _;
        let dir = schist_app_settings::schist_folder()
            .ok_or_else(|| anyhow::anyhow!(t("actions.no_settings_folder")))?;
        std::fs::create_dir_all(&dir)?;
        let mut file = tempfile::NamedTempFile::new_in(&dir)?;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(dir.join(format!("workflow-sync-{key}.json")))?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let key = format!("schist.workflow-sync.{key}");
        schist_app_platform::web::local_set(&key, &text);
        ensure!(
            crate::web::local_get(&key).as_deref() == Some(text.as_str()),
            t("actions.storage_failed")
        );
    }
    Ok(())
}
