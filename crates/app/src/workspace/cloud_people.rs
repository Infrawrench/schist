//! People names and boxes belong to the provider workspace; model credentials never leave it.
use super::gallery_chrome::{self as chrome, pal};
use super::*;
use gpui::{img, StatefulInteractiveElement as _};
use schist_cloud::{protocol::value, Face, FaceRect, Value};

pub(crate) fn rows(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Vec<gpui::AnyElement> {
    if !ws.cloud.show {
        return vec![];
    }
    let Some(people) = ws.cloud.people.clone() else {
        return vec![];
    };
    let mut rows = vec![chrome::sidebar_caption("PEOPLE").into_any_element()];
    for person in people.people {
        let id = person.id.clone();
        let edit = person.clone();
        rows.push(
            div()
                .flex()
                .items_center()
                .child(
                    chrome::sidebar_row_frame(
                        SharedString::from(format!("person-{}", person.id)),
                        person.name.clone(),
                        Some(person.asset_count as usize),
                        ws.cloud.query.filters.person_id.as_deref() == Some(&person.id),
                        0,
                    )
                    .flex_grow()
                    .on_click(
                        cx.listener(move |ws, _, _, cx| ws.cloud_person(Some(id.clone()), cx)),
                    ),
                )
                .child(chrome::gallery_button(
                    "…",
                    false,
                    move |ws, _, cx| {
                        ws.open_modal(
                            Modal::Cloud {
                                kind: "people-rename",
                                fields: vec![
                                    ("cloud-person-id", "".into(), edit.id.clone()),
                                    ("cloud-name", "Name".into(), edit.name.clone()),
                                ],
                            },
                            cx,
                        );
                    },
                    cx,
                ))
                .into_any_element(),
        );
    }
    rows.push(
        chrome::sidebar_link(
            format!("Unnamed faces   {}", people.unnamed),
            |ws, _, cx| ws.cloud_person(Some("unnamed".into()), cx),
            cx,
        )
        .into_any_element(),
    );
    rows.push(
        chrome::sidebar_link(
            if people.enabled {
                "Face detection settings…"
            } else {
                "+ Find faces…"
            },
            |ws, _, cx| {
                ws.open_modal(
                    Modal::Cloud {
                        kind: "people-enable",
                        fields: vec![],
                    },
                    cx,
                )
            },
            cx,
        )
        .into_any_element(),
    );
    if people.pending > 0 {
        rows.push(
            div()
                .px_2()
                .text_size(px(11.0))
                .child(format!("Finding faces in {} photos…", people.pending))
                .into_any_element(),
        );
    }
    if people.failed > 0 {
        rows.push(
            div()
                .px_2()
                .text_size(px(11.0))
                .child(format!("Could not process {} photos", people.failed))
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
                    ("cloud-name", "Name".into(), name.clone()),
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
        if let Some((_, image)) = ws.cloud.thumbnails.get(&asset.id).cloned() {
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
                "Cancel drawing"
            } else {
                "Add a face…"
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
                .map(|p| p.name.as_str())
                .unwrap_or("Unnamed");
            let a = asset.clone();
            let f = face.clone();
            let mut row = div()
                .flex()
                .gap_2()
                .items_center()
                .child(chrome::gallery_button(
                    format!("{}{}", person, if face.automatic { " · auto" } else { "" }),
                    false,
                    move |ws, _, cx| ws.cloud_face_name(&a, Some(&f), None, cx),
                    cx,
                ));
            for (label, method) in [("Not a face", "face.dismiss"), ("Not them", "face.reject")] {
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
                    format!("Is this {name}? Yes"),
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
            body =
                body.child("No faces named yet. Draw a box, or enable Find faces in the sidebar.");
        }
    } else {
        body = body.child("This photo is no longer available in this view.");
    }
    crate::ui::modal_frame(
        "People in this photo",
        620.,
        body,
        chrome::gallery_button("Close", false, |ws, _, cx| ws.close_modal(cx), cx),
    )
    .into_any_element()
}
pub(crate) fn submit(
    ws: &mut Workspace,
    kind: &str,
    get: impl Fn(&str) -> String,
) -> anyhow::Result<bool> {
    match kind {
        "people-enable" => ws.cloud_mutate(
            "people.enable",
            vec![(
                "enabled",
                (!ws.cloud.people.as_ref().is_some_and(|p| p.enabled)).into(),
            )],
        ),
        "people-rename" => {
            anyhow::ensure!(!get("cloud-name").is_empty(), "Enter a name");
            ws.cloud_mutate(
                "people.rename",
                vec![
                    ("id", get("cloud-person-id").into()),
                    ("name", get("cloud-name").into()),
                ],
            );
        }
        "face-name" | "face-add" => {
            anyhow::ensure!(!get("cloud-name").is_empty(), "Enter a name");
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
