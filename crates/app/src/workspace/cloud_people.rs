//! People names and boxes belong to the provider workspace; model credentials never leave it.
use super::cloud_view::CLOUD_GLYPH;
use super::gallery_chrome::{self as chrome, pal};
use super::*;
use gpui::{img, StatefulInteractiveElement as _};
use schist_cloud::{protocol::value, Face, FaceRect, Value};
use schist_i18n::{t, tf, tn};
use schist_ui::Badge;

const PREVIEW_PX: u32 = 64;

impl super::cloud::CloudState {
    /// Crop once per source image and face box, using local People's crop and
    /// padding. RenderImage already holds BGRA: resampling preserves that order.
    pub(super) fn face_preview_image(
        &mut self,
        image: &Arc<RenderImage>,
        rect: &FaceRect,
    ) -> Option<Arc<RenderImage>> {
        let coordinates = [rect.x, rect.y, rect.w, rect.h];
        let dimensions = image.size(0);
        let width = u32::from(dimensions.width);
        let height = u32::from(dimensions.height);
        if width == 0
            || height == 0
            || !coordinates.iter().all(|v| v.is_finite())
            || rect.w <= 0.0
            || rect.h <= 0.0
        {
            return None;
        }
        let key = (image.id, coordinates.map(f32::to_bits));
        if let Some(crop) = self.face_previews.get(&key) {
            return Some(crop.clone());
        }
        let rect = schist_gallery::FaceRect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
        };
        let pixels = schist_gallery::face_crop_rgba(
            image.as_bytes(0)?,
            width,
            height,
            &rect,
            schist_gallery::AVATAR_GROW,
            PREVIEW_PX,
        )?;
        let buffer = image::RgbaImage::from_raw(PREVIEW_PX, PREVIEW_PX, pixels)?;
        let crop = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        self.face_previews.insert(key, crop.clone());
        Some(crop)
    }
}

fn face_preview(
    cloud: &mut super::cloud::CloudState,
    image: Option<Arc<RenderImage>>,
    rect: &FaceRect,
    size: f32,
) -> gpui::AnyElement {
    let mut frame = div()
        .w(px(size))
        .h(px(size))
        .flex_none()
        .rounded_full()
        .overflow_hidden()
        .bg(gpui::rgb(pal().cell_edge));
    if let Some(image) = image.and_then(|image| cloud.face_preview_image(&image, rect)) {
        frame = frame.child(img(image).w(px(size)).h(px(size)).rounded_full());
    }
    frame.into_any_element()
}

pub(super) fn naming_preview(
    ws: &mut Workspace,
    fields: &[(&str, String, String)],
) -> Option<gpui::AnyElement> {
    let get = |key| {
        fields
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, _, v)| v.as_str())
    };
    let asset = ws
        .cloud
        .assets
        .iter()
        .find(|a| Some(a.id.as_str()) == get("cloud-asset-id"))?;
    let rect = asset
        .faces
        .iter()
        .find(|face| Some(face.id.as_str()) == get("cloud-face-id"))
        .map(|face| face.rect.clone())
        .or_else(|| serde_json::from_str::<FaceRect>(get("cloud-face-rect")?).ok())?;
    let image = ws
        .cloud
        .thumbnails
        .get(&asset.id)
        .filter(|(revision, _)| *revision == asset.revision)
        .map(|(_, image)| image.clone());
    Some(face_preview(&mut ws.cloud, image, &rect, 64.0))
}

/// The cloud's people, drawn like the local PEOPLE rows: a round badge,
/// the name, a count, and the actions on the right-click menu. Signed
/// out, or before the provider has looked, there is nothing to list.
/// `caption` adds the section heading, for a sidebar without a local
/// list above.
pub(crate) fn rows(
    ws: &mut Workspace,
    caption: bool,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    if !crate::feature_enabled("schist-cloud") || ws.cloud.account.is_none() {
        return vec![];
    }
    let Some(people) = ws.cloud.people.clone() else {
        return vec![];
    };
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    if caption {
        rows.push(chrome::sidebar_caption(t("cloud.sidebar.people")).into_any_element());
    }
    let viewing = ws.cloud.query.filters.person_id.clone();
    let badge = |glyph: &'static str| {
        Badge::new(glyph)
            .outlined()
            .colors(pal().text_dim, pal().text_dim)
    };
    let person_row = |id: SharedString,
                      portrait: gpui::AnyElement,
                      name: String,
                      count: u64,
                      selected: bool,
                      filter: String,
                      context: Option<String>,
                      cx: &mut Context<Workspace>| {
        let mut row = div()
            .id(id)
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(26.0))
            .text_size(px(12.0))
            .cursor_pointer()
            .bg(gpui::rgb(if selected {
                pal().sidebar_selected
            } else {
                pal().chrome_bg
            }))
            .hover(|s| s.bg(gpui::rgb(pal().sidebar_selected)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                    ws.cloud_person(Some(filter.clone()), cx)
                }),
            );
        if let Some(person) = context {
            row = row.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                    ws.cloud.context = Some((
                        ev.position,
                        super::cloud::CloudContext::Person(person.clone()),
                    ));
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ws.library.context = None;
                    }
                    cx.notify();
                }),
            );
        }
        row.child(portrait)
            .child(div().flex_grow().truncate().child(SharedString::from(name)))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(gpui::rgb(pal().text_dim))
                    .child(format!("{count}")),
            )
            .into_any_element()
    };
    for person in &people.people {
        let portrait = person
            .avatar
            .as_ref()
            .map(|avatar| {
                let image = ws.cloud.avatar_source(avatar).and_then(|(revision, _)| {
                    ws.cloud
                        .thumbnails
                        .get(&avatar.asset_id)
                        .filter(|(r, _)| *r == revision)
                        .map(|(_, image)| image.clone())
                });
                face_preview(&mut ws.cloud, image, &avatar.rect, 20.0)
            })
            .unwrap_or_else(|| badge(CLOUD_GLYPH).into_any_element());
        rows.push(person_row(
            SharedString::from(format!("cloud-person-{}", person.id)),
            portrait,
            person.name.clone(),
            person.asset_count,
            viewing.as_deref() == Some(&person.id),
            person.id.clone(),
            Some(person.id.clone()),
            cx,
        ));
    }
    if people.unnamed > 0 {
        rows.push(person_row(
            "cloud-person-unnamed".into(),
            badge("?").into_any_element(),
            t("cloud.people.unnamed_faces_row").into(),
            people.unnamed,
            viewing.as_deref() == Some("unnamed"),
            "unnamed".into(),
            None,
            cx,
        ));
    }
    if people.pending > 0 {
        rows.push(
            div()
                .px_2()
                .py_1()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(tn("cloud.people.finding_faces", people.pending))
                .into_any_element(),
        );
    }
    if people.failed > 0 {
        rows.push(
            div()
                .px_2()
                .py_1()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(tn("cloud.people.could_not_process", people.failed))
                .into_any_element(),
        );
    }
    rows
}
impl Workspace {
    pub(crate) fn cloud_person(&mut self, person: Option<String>, cx: &mut Context<Self>) {
        self.cloud.query.filters.person_id = if self.cloud.query.filters.person_id == person {
            None
        } else {
            person
        };
        self.cloud.query.offset = 0;
        self.cloud_watch_assets(false);
        cx.notify();
    }
    pub(crate) fn cloud_people_view(&mut self, cx: &mut Context<Self>) {
        if let Some(asset) = self.cloud_lead_asset() {
            self.cloud.face_drawing = false;
            self.cloud.face_draft = None;
            self.open_modal(
                Modal::Cloud {
                    kind: "people-view",
                    fields: vec![("cloud-asset-id", "".into(), asset.id)],
                },
                cx,
            );
        }
    }
    fn cloud_face_name(
        &mut self,
        asset: &schist_cloud::Asset,
        face: Option<&Face>,
        rect: Option<FaceRect>,
        cx: &mut Context<Self>,
    ) {
        let name = face
            .and_then(|f| f.person_id.as_ref())
            .and_then(|id| {
                self.cloud
                    .people
                    .as_ref()?
                    .people
                    .iter()
                    .find(|p| &p.id == id)
            })
            .map(|p| p.name.clone())
            .unwrap_or_default();
        self.open_modal(
            Modal::Cloud {
                kind: if face.is_some() {
                    "face-name"
                } else {
                    "face-add"
                },
                fields: vec![
                    ("cloud-asset-id", "".into(), asset.id.clone()),
                    ("cloud-revision", "".into(), asset.revision.to_string()),
                    (
                        "cloud-face-id",
                        "".into(),
                        face.map(|f| f.id.clone()).unwrap_or_default(),
                    ),
                    (
                        "cloud-face-rect",
                        "".into(),
                        rect.map(|r| serde_json::to_string(&r).unwrap())
                            .unwrap_or_default(),
                    ),
                    ("cloud-name", t("common.name").into(), name.clone()),
                ],
            },
            cx,
        );
        self.focus_field("cloud-name", name);
    }
}
pub(crate) fn viewer(
    ws: &mut Workspace,
    fields: Vec<(&'static str, String, String)>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let id = fields
        .iter()
        .find(|(k, _, _)| *k == "cloud-asset-id")
        .map(|(_, _, v)| v);
    let asset = id
        .and_then(|id| ws.cloud.assets.iter().find(|a| &a.id == id))
        .cloned();
    let mut body = div().flex().flex_col().gap_2();
    if let Some(asset) = asset {
        let image = ws
            .cloud
            .thumbnails
            .get(&asset.id)
            .filter(|(revision, _)| *revision == asset.revision)
            .map(|(_, image)| image.clone());
        if let Some(image) = image.clone() {
            let size = image.size(0);
            let ratio = u32::from(size.width) as f32 / u32::from(size.height) as f32;
            let width = 520.0_f32.min(380.0 * ratio);
            let height = width / ratio;
            let drawing = ws.cloud.face_drawing;
            let up = asset.clone();
            let probe = cx.entity();
            let mut frame = div()
                .relative()
                .w(px(width))
                .h(px(height))
                .child(img(image).w(px(width)).h(px(height)))
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            probe.update(cx, |ws, _| ws.cloud.face_bounds = bounds)
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, e: &MouseDownEvent, _, cx| {
                        if !drawing {
                            return;
                        }
                        let b = ws.cloud.face_bounds;
                        let x = (f32::from(e.position.x - b.origin.x) / width).clamp(0., 1.);
                        let y = (f32::from(e.position.y - b.origin.y) / height).clamp(0., 1.);
                        ws.cloud.face_start = Some((x, y));
                        ws.cloud.face_draft = None;
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(move |ws, e: &MouseMoveEvent, _, cx| {
                    if let Some((x, y)) = ws.cloud.face_start {
                        let b = ws.cloud.face_bounds;
                        let ex = (f32::from(e.position.x - b.origin.x) / width).clamp(0., 1.);
                        let ey = (f32::from(e.position.y - b.origin.y) / height).clamp(0., 1.);
                        ws.cloud.face_draft = Some(FaceRect {
                            x: x.min(ex),
                            y: y.min(ey),
                            w: (ex - x).abs(),
                            h: (ey - y).abs(),
                        });
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |ws, _, _, cx| {
                        ws.cloud.face_start = None;
                        if let Some(rect) = ws
                            .cloud
                            .face_draft
                            .take()
                            .filter(|r| r.w > 0.005 && r.h > 0.005)
                        {
                            ws.cloud_face_name(&up, None, Some(rect), cx);
                        }
                    }),
                );
            if !drawing {
                for face in &asset.faces {
                    let a = asset.clone();
                    let f = face.clone();
                    let rect = &face.rect;
                    frame = frame.child(
                        div()
                            .id(SharedString::from(format!("face-{}", face.id)))
                            .absolute()
                            .left(px(rect.x * width))
                            .top(px(rect.y * height))
                            .w(px(rect.w * width))
                            .h(px(rect.h * height))
                            .border_2()
                            .border_color(gpui::rgb(if face.person_id.is_some() {
                                pal().green
                            } else {
                                pal().header
                            }))
                            .cursor_pointer()
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                ws.cloud_face_name(&a, Some(&f), None, cx)
                            })),
                    );
                }
            }
            if let Some(r) = &ws.cloud.face_draft {
                frame = frame.child(
                    div()
                        .absolute()
                        .left(px(r.x * width))
                        .top(px(r.y * height))
                        .w(px(r.w * width))
                        .h(px(r.h * height))
                        .border_2()
                        .border_color(gpui::rgb(pal().header)),
                );
            }
            body = body.child(frame);
        }
        body = body.child(chrome::gallery_button(
            if ws.cloud.face_drawing {
                t("cloud.people.cancel_drawing")
            } else {
                t("cloud.people.add_face")
            },
            false,
            |ws, _, cx| {
                ws.cloud.face_drawing = !ws.cloud.face_drawing;
                ws.cloud.face_start = None;
                ws.cloud.face_draft = None;
                cx.notify();
            },
            cx,
        ));
        for face in &asset.faces {
            let person = face
                .person_id
                .as_ref()
                .and_then(|id| {
                    ws.cloud
                        .people
                        .as_ref()?
                        .people
                        .iter()
                        .find(|p| &p.id == id)
                })
                .map(|p| p.name.clone())
                .unwrap_or_else(|| t("cloud.people.unnamed").to_string());
            let a = asset.clone();
            let f = face.clone();
            let mut row = div()
                .flex()
                .gap_2()
                .items_center()
                .child(face_preview(&mut ws.cloud, image.clone(), &face.rect, 48.0))
                .child(chrome::gallery_button(
                    if face.automatic {
                        tf!("cloud.people.auto_suffix", name = person)
                    } else {
                        person
                    },
                    false,
                    move |ws, _, cx| ws.cloud_face_name(&a, Some(&f), None, cx),
                    cx,
                ));
            for (label, method) in [
                (t("cloud.people.not_a_face"), "face.dismiss"),
                (t("cloud.people.not_them"), "face.reject"),
            ] {
                if method == "face.reject" && !face.automatic && face.suggestion.is_none() {
                    continue;
                }
                let a = asset.clone();
                let f = face.clone();
                row = row.child(chrome::gallery_button(
                    label,
                    false,
                    move |ws, _, _| {
                        ws.cloud_mutate(
                            method,
                            vec![
                                ("asset_id", a.id.clone().into()),
                                ("revision", a.revision.into()),
                                ("face_id", f.id.clone().into()),
                            ],
                        )
                    },
                    cx,
                ));
            }
            if let Some(person) = face.suggestion.as_ref().and_then(|id| {
                ws.cloud
                    .people
                    .as_ref()?
                    .people
                    .iter()
                    .find(|p| &p.id == id)
            }) {
                let name = person.name.clone();
                let a = asset.clone();
                let f = face.clone();
                row = row.child(chrome::gallery_button(
                    tf!("cloud.people.confirm_suggestion", name = name),
                    false,
                    move |ws, _, _| {
                        ws.cloud_mutate(
                            "face.name",
                            vec![
                                ("asset_id", a.id.clone().into()),
                                ("revision", a.revision.into()),
                                ("face_id", f.id.clone().into()),
                                ("name", name.clone().into()),
                            ],
                        )
                    },
                    cx,
                ));
            }
            body = body.child(row);
        }
        if asset.faces.is_empty() {
            body = body.child(t("cloud.people.no_faces"));
        }
    } else {
        body = body.child(t("cloud.people.photo_unavailable"));
    }
    crate::ui::modal_frame(
        t("cloud.people.title"),
        620.,
        body,
        chrome::gallery_button(t("common.close"), false, |ws, _, cx| ws.close_modal(cx), cx),
    )
    .into_any_element()
}
pub(crate) fn submit(
    ws: &mut Workspace,
    kind: &str,
    get: impl Fn(&str) -> String,
) -> anyhow::Result<bool> {
    match kind {
        "people-rename" => {
            anyhow::ensure!(
                !get("cloud-name").is_empty(),
                t("cloud.people.enter_a_name")
            );
            ws.cloud_mutate(
                "people.rename",
                vec![
                    ("id", get("cloud-person-id").into()),
                    ("name", get("cloud-name").into()),
                ],
            );
        }
        "face-name" | "face-add" => {
            anyhow::ensure!(
                !get("cloud-name").is_empty(),
                t("cloud.people.enter_a_name")
            );
            let mut fields = vec![
                ("asset_id", get("cloud-asset-id").into()),
                ("revision", get("cloud-revision").parse::<u64>()?.into()),
                ("name", get("cloud-name").into()),
            ];
            if kind == "face-add" {
                fields.push((
                    "rect",
                    value(&serde_json::from_str::<FaceRect>(&get("cloud-face-rect"))?),
                ));
            } else {
                fields.push(("face_id", Value::from(get("cloud-face-id"))));
            }
            ws.cloud_mutate(
                if kind == "face-add" {
                    "face.add"
                } else {
                    "face.name"
                },
                fields,
            );
        }
        _ => return Ok(false),
    }
    Ok(true)
}
