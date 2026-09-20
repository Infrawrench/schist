//! Apply the same editor workflows to cloud assets and upload separate results.
use super::recorded_actions::{Runtime, SavedAction};
use super::*;
use anyhow::ensure;
use schist_cloud::{self as remote, Asset};
use schist_i18n::{t, tf};
use std::sync::atomic::{AtomicBool, Ordering};

enum Operation {
    Action(SavedAction, Runtime),
    Recipe(crate::export_recipes::Recipe),
}
impl Workspace {
    pub(crate) fn cloud_replay_action_gallery(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(action) = self.action_recorder.draft.clone() else {
            return;
        };
        let runtime = Runtime::new(&self.registry);
        if let Err(error) = runtime.validate(&action) {
            self.cloud_error(tf!("actions.failed", error = error));
            cx.notify();
            return;
        }
        let photos = self
            .cloud
            .assets
            .iter()
            .filter(|a| self.cloud.selected.contains(&a.id))
            .cloned()
            .collect();
        self.cloud_run_batch(photos, Operation::Action(action, runtime), cx);
    }
    pub(super) fn cloud_run_recipe(
        &mut self,
        photos: Vec<Asset>,
        recipe: crate::export_recipes::Recipe,
        cx: &mut Context<Self>,
    ) {
        self.cloud_run_batch(photos, Operation::Recipe(recipe), cx);
    }
    fn cloud_run_batch(
        &mut self,
        photos: Vec<Asset>,
        mut operation: Operation,
        cx: &mut Context<Self>,
    ) {
        if self.cloud.batch_cancel.is_some() || photos.is_empty() {
            return;
        }
        let Some(client) = &self.cloud.client else {
            return;
        };
        let (handle, sender, epoch, capabilities, account_cancel) = (
            client.handle.clone(),
            self.cloud.sender.clone(),
            self.cloud.epoch,
            self.cloud.capabilities.clone(),
            self.cloud.cancel.clone(),
        );
        let codecs = self.registry.shared_codecs();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cloud.batch_cancel = Some(cancel.clone());
        self.close_modal(cx);
        remote::runtime::spawn(async move {
            let mut written = 0;
            let mut failed = 0;
            let mut first = None;
            let total = photos.len();
            for (index, asset) in photos.into_iter().enumerate() {
                if cancel.load(Ordering::Relaxed) || account_cancel.load(Ordering::Relaxed) {
                    break;
                }
                let result = async {
                    let download = handle
                        .download_asset_async(&asset.id, None, capabilities.as_ref())
                        .await?;
                    ensure!(
                        download.revision == asset.revision,
                        t("library.similar.stale")
                    );
                    let ext = std::path::Path::new(&asset.name)
                        .extension()
                        .and_then(|e| e.to_str());
                    let codec = codecs
                        .iter()
                        .find(|c| c.probe(&download.bytes))
                        .or_else(|| {
                            codecs
                                .iter()
                                .find(|c| ext.is_some_and(|e| c.extensions().contains(&e)))
                        })
                        .ok_or_else(|| anyhow::anyhow!(t("common.unsupported_format")))?;
                    let mut doc = codec.import(&download.bytes)?;
                    doc.title = asset.name.clone();
                    let outputs = match &mut operation {
                        Operation::Action(action, runtime) => {
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
                            schist_compositor::restyle_layers(
                                &mut doc.tree.layers,
                                &mut Vec::new(),
                            );
                            let bytes = schist_codec_psd::write_psd(&doc)?;
                            let stem = std::path::Path::new(&asset.name)
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy();
                            vec![(format!("{stem}-action.psd"), bytes)]
                        }
                        Operation::Recipe(recipe) => {
                            super::export_recipes::cloud_outputs(&doc, recipe, &codecs)?
                        }
                    };
                    for (name, bytes) in outputs {
                        if cancel.load(Ordering::Relaxed) || account_cancel.load(Ordering::Relaxed)
                        {
                            break;
                        }
                        let mime = if name.ends_with(".psd") {
                            "image/vnd.adobe.photoshop"
                        } else {
                            "application/octet-stream"
                        };
                        handle
                            .upload_async(remote::Upload {
                                name: &name,
                                bytes: &bytes,
                                mime,
                                folder: asset.folder_id.as_deref(),
                                asset: None,
                                relative: None,
                                mutation: &remote::Uuid::new_v4().to_string(),
                            })
                            .await?;
                        written += 1;
                    }
                    Ok::<_, anyhow::Error>(())
                }
                .await;
                if let Err(error) = result {
                    failed += 1;
                    if first.is_none() {
                        first = Some(error.to_string());
                    }
                }
                let _ = sender.send(super::cloud::Job::Progress {
                    epoch,
                    done: (index + 1) as u64,
                    total: total as u64,
                    label: tf!(
                        "export_recipes.progress",
                        done = index + 1,
                        total = total,
                        written = written,
                        failed = failed
                    ),
                });
            }
            let message = match first {
                Some(error) => tf!(
                    "export_recipes.partial",
                    written = written,
                    failed = failed,
                    error = error
                ),
                None => tf!("export_recipes.done", written = written),
            };
            let _ = sender.send(super::cloud::Job::BatchFinished { epoch, message });
        });
        cx.notify();
    }
}
