//! Gallery virtual copies: file identities remain stable across renames.
use super::*;
use schist_gallery::variants;
use schist_i18n::{t, tf};

impl Workspace {
    pub(super) fn variant_prompt(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {
        let name = if rename {
            variants::display_name(&path)
        } else {
            tf!("common.copy_suffix", name = variants::display_name(&path))
        };
        self.open_modal(
            Modal::VariantName {
                path,
                name: name.clone(),
                rename,
            },
            cx,
        );
        self.focus_field("variant-name", name);
    }

    pub(super) fn variant_delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // An open tab must never resurrect a deleted copy on its next save.
        if self
            .library
            .edit_backings
            .values()
            .chain(self.library.pending_backing.iter().map(|(_, p)| p))
            .any(|p| p == &path)
        {
            self.status = t("variants.close").into();
            cx.notify();
            return;
        }
        match variants::delete(&path) {
            Ok(()) => {
                self.library.selected.retain(|p| p != &path);
                self.library.thumbs.remove(&path);
                self.library_rescan(cx);
            }
            Err(error) => self.status = tf!("workspace.docs.save_failed", error = error).into(),
        }
        cx.notify();
    }

    fn variant_save_name(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::VariantName { path, name, rename }) = self.modal.clone() else {
            return;
        };
        if name.trim().is_empty() {
            return;
        }
        if rename {
            match variants::rename(&path, &name) {
                Ok(()) => {
                    self.library
                        .variant_names
                        .insert(path.clone(), name.trim().to_owned());
                    for doc in self
                        .doc
                        .iter_mut()
                        .chain(self.background_tabs.iter_mut().map(|tab| &mut tab.doc))
                    {
                        if self.library.edit_backings.get(&doc.id) == Some(&path) {
                            doc.title = name.trim().to_owned();
                        }
                    }
                    self.library_rescan(cx);
                    self.close_modal(cx);
                }
                Err(error) => self.status = tf!("workspace.docs.save_failed", error = error).into(),
            }
            cx.notify();
            return;
        }
        self.close_modal(cx);
        let codecs = self.registry.shared_codecs();
        self.status = t("versions.restoring").into();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let source = schist_gallery::backing_psd(&path)
                        .filter(|p| p.is_file())
                        .unwrap_or_else(|| path.clone());
                    let bytes = if source
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("psd"))
                    {
                        std::fs::read(&source).map_err(anyhow::Error::from)?
                    } else {
                        let doc = super::decode_file(&codecs, &source)?;
                        let codec = codecs
                            .iter()
                            .find(|c| c.can_export() && c.extensions().contains(&"psd"))
                            .ok_or_else(|| {
                                anyhow::anyhow!("{}", t("library.batch.no_psd_writer"))
                            })?;
                        codec.export(&doc)?
                    };
                    variants::create(&path, &name, &bytes).map_err(anyhow::Error::from)
                })
                .await;
            this.update(cx, |ws, cx| {
                match result {
                    Ok(path) => {
                        ws.status =
                            tf!("workspace.docs.saved", name = variants::display_name(&path))
                                .into();
                        ws.library_rescan(cx);
                    }
                    Err(error) => {
                        ws.status = tf!("workspace.docs.save_failed", error = error).into()
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

pub(crate) fn variant_name_dialog(
    ws: &mut Workspace,
    name: String,
    rename: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let field =
        super::library_view::bucket_field("variant-name", name, t("common.name").into(), ws, cx);
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(crate::ui::field_row(t("common.name"), field))
        .children((!rename).then(|| div().child(t("variants.saved_hint"))));
    let actions = div()
        .flex()
        .gap_2()
        .child(crate::ui::button(
            t("common.cancel"),
            false,
            |ws, _, cx| ws.close_modal(cx),
            cx,
        ))
        .child(crate::ui::button(
            t("common.save"),
            true,
            |ws, _, cx| ws.variant_save_name(cx),
            cx,
        ));
    crate::ui::modal_frame(
        t(if rename {
            "variants.rename"
        } else {
            "variants.create"
        }),
        420.0,
        body,
        actions,
    )
}
