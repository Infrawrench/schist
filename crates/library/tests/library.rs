use schist::{App, People, PeopleRequest};
use schist_core::color::Depth;
use schist_plugin_api::ExportOptions;
use serde_json::json;

#[test]
fn instances_own_documents_tools_clipboards_and_history() {
    let mut a = App::new();
    let mut b = App::new();
    let aid = a.create("A", 16, 12, Depth::Eight).unwrap();
    let bid = b.create("B", 16, 12, Depth::Eight).unwrap();
    assert_eq!(aid, bid, "session counters belong to each instance");
    let white = b.render(bid, None).unwrap().1;
    a.call(aid, "adjust_invert", &json!({})).unwrap();
    assert_ne!(a.render(aid, None).unwrap().1, white);
    assert_eq!(b.render(bid, None).unwrap().1, white);
    a.call(
        aid,
        "set_editor",
        &json!({"foreground":"#ff0000", "brush_size": 3}),
    )
    .unwrap();
    a.call(aid, "tool_brush", &json!({})).unwrap();
    assert_ne!(
        a.session(aid).unwrap().editor.active_tool,
        b.session(bid).unwrap().editor.active_tool
    );
    assert_eq!(b.session(bid).unwrap().editor.brush_size, 24.0);
    assert!(!b.session(bid).unwrap().document.history.can_undo());
    a.call(aid, "cmd_edit_copy", &json!({})).unwrap();
    assert!(a.session(aid).unwrap().editor.clipboard.is_some());
    assert!(b.session(bid).unwrap().editor.clipboard.is_none());
    a.session(aid).unwrap().document.undo().unwrap();
    assert_eq!(a.render(aid, None).unwrap().1, white);
    a.close(aid).unwrap();
    assert!(a.session(aid).is_err());
    assert!(b.session(bid).is_ok());
}

#[test]
fn portable_catalog_and_binary_round_trip_use_the_real_editor() {
    let mut app = App::new();
    let catalog = app.request(&json!({"op":"catalog"})).unwrap();
    assert!(catalog["actions"].as_array().unwrap().len() > 150);
    for name in [
        "tool_brush",
        "tool_type",
        "tool_stroke",
        "get_state",
        "adjust_invert",
    ] {
        assert!(
            catalog["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["name"] == name),
            "{name}"
        );
    }
    let id = app.create("test", 8, 6, Depth::Eight).unwrap();
    app.call(id, "adjust_invert", &json!({})).unwrap();
    let bytes = app.export(id, "psd", ExportOptions::default()).unwrap();
    let restored = app.import("test.psd", &bytes).unwrap();
    assert_eq!(
        app.render(id, None).unwrap().1,
        app.render(restored, None).unwrap().1
    );
    assert!(app
        .call(id, "render", &json!({"path":"unexpected.png"}))
        .is_err());
}

#[test]
fn bad_requests_do_not_destroy_the_instance() {
    let mut app = App::new();
    for request in [
        b"{".as_slice(),
        br#"{"op":"create","width":0,"height":1}"#,
        br#"{"op":"call","session":999,"name":"get_state"}"#,
    ] {
        assert!(app.request_json(request).is_err());
    }
    assert!(app.create("ok", 1, 1, Depth::Eight).is_ok());
}

fn pixels(boxes: Option<Vec<schist::people::FaceRect>>) -> PeopleRequest {
    PeopleRequest {
        width: 320,
        height: 240,
        rgb: vec![128; 320 * 240 * 3],
        boxes,
    }
}

#[test]
fn people_models_are_owned_replaceable_and_use_the_desktop_pipeline() {
    let mut a = People::default();
    let b = People::default();
    a.load_model("face", include_bytes!("fixtures/detector.onnx"))
        .unwrap();
    a.load_model("face-embed", include_bytes!("fixtures/recogniser.onnx"))
        .unwrap();
    let faces = a.process(pixels(None)).unwrap();
    assert_eq!(faces.len(), 1);
    assert!((faces[0].rect.x - 0.25).abs() < 0.0001);
    assert!((faces[0].rect.w - 0.5).abs() < 0.0001);
    assert_eq!(faces[0].embedding.len(), 128);
    let norm: f32 = faces[0].embedding.iter().map(|v| v * v).sum();
    assert!((norm - 1.0).abs() < 0.0001);
    assert!(b.process(pixels(None)).is_err());
    assert!(a.load_model("face", b"broken ONNX").is_err());
    assert_eq!(a.process(pixels(None)).unwrap().len(), 1);
    a.unload_model("face").unwrap();
    assert!(a.process(pixels(None)).is_err());
    assert_eq!(
        a.process(pixels(Some(vec![faces[0].rect]))).unwrap().len(),
        1
    );
    assert!(b.process(pixels(Some(vec![faces[0].rect]))).is_err());
    assert!(a.process(pixels(Some(vec![]))).unwrap().is_empty());
}

#[test]
fn people_rejects_unbounded_or_invalid_inputs_before_loading_models() {
    let p = People::default();
    assert!(p
        .process(PeopleRequest {
            width: 0,
            height: 1,
            rgb: vec![],
            boxes: None
        })
        .is_err());
    assert!(p
        .process(PeopleRequest {
            width: 1025,
            height: 1,
            rgb: vec![],
            boxes: None
        })
        .is_err());
    assert!(p
        .process(PeopleRequest {
            width: 1,
            height: 1,
            rgb: vec![0; 2],
            boxes: None
        })
        .is_err());
    let rect = schist::people::FaceRect {
        x: 0.,
        y: 0.,
        w: 1.,
        h: 1.,
    };
    assert!(p.process(pixels(Some(vec![rect; 101]))).is_err());
    assert!(p
        .process(pixels(Some(vec![schist::people::FaceRect {
            x: f32::NAN,
            ..rect
        }])))
        .is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn c_abi_owns_outputs_and_reports_errors_without_a_global_error_slot() {
    use schist::ffi::*;
    unsafe {
        let a = schist_create();
        let b = schist_create();
        assert!(!a.is_null() && !b.is_null());
        let mut out = SchistBuffer {
            data: std::ptr::null_mut(),
            len: 0,
        };
        assert_eq!(schist_request(a, std::ptr::null(), 1, &mut out), 1);
        assert!(out.len > 0);
        schist_buffer_free(&mut out);
        assert!(out.data.is_null());
        assert_eq!(out.len, 0);
        let request = br#"{"op":"sessions"}"#;
        assert_eq!(
            schist_request(b, request.as_ptr(), request.len(), &mut out),
            0
        );
        assert_eq!(std::slice::from_raw_parts(out.data, out.len), b"[]");
        schist_buffer_free(&mut out);
        schist_buffer_free(&mut out);
        schist_destroy(a);
        schist_destroy(b);
        schist_destroy(std::ptr::null_mut());
    }
}
