#![cfg(target_arch = "wasm32")]

use futures::{FutureExt as _, StreamExt as _};
use schist_app_platform::web::{import_dropped_file, listen_for_file_drops, read_file};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn file(name: &str, bytes: &[u8]) -> web_sys::File {
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes));
    web_sys::File::new_with_u8_array_sequence(&parts, name).unwrap()
}

fn dispatch(kind: &str, data: &web_sys::DataTransfer) -> web_sys::DragEvent {
    let init = web_sys::DragEventInit::new();
    init.set_data_transfer(Some(data));
    init.set_cancelable(true);
    init.set_bubbles(true);
    let event = web_sys::DragEvent::new_with_event_init_dict(kind, &init).unwrap();
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .body()
        .unwrap()
        .dispatch_event(&event)
        .unwrap();
    event
}

#[wasm_bindgen_test(async)]
async fn files_are_captured_before_drop_data_expires_and_keep_unique_paths() {
    let (_listener, mut drops) = listen_for_file_drops().unwrap();
    let data = web_sys::DataTransfer::new().unwrap();
    data.items()
        .add_with_file(&file("水彩.png", &[1, 2, 3]))
        .unwrap();
    data.items()
        .add_with_file(&file("水彩.png", &[4, 5]))
        .unwrap();
    assert!(dispatch("dragenter", &data).default_prevented());
    assert!(dispatch("dragover", &data).default_prevented());
    assert!(drops.next().now_or_never().is_none());
    assert!(dispatch("drop", &data).default_prevented());
    data.items().clear().unwrap();

    let files = drops.next().await.unwrap();
    assert_eq!(files.len(), 2);
    let mut paths = Vec::new();
    for file in files {
        paths.push(import_dropped_file(file).await.unwrap());
    }
    assert_ne!(paths[0], paths[1]);
    assert_eq!(paths[0].file_name().unwrap(), "水彩.png");
    assert_eq!(read_file(&paths[0]).unwrap().as_slice(), &[1, 2, 3]);
    assert_eq!(read_file(&paths[1]).unwrap().as_slice(), &[4, 5]);
}

#[wasm_bindgen_test(async)]
async fn text_drags_are_ignored_and_listeners_are_removed_on_close() {
    let (listener, mut drops) = listen_for_file_drops().unwrap();
    let data = web_sys::DataTransfer::new().unwrap();
    data.set_data("text/plain", "hello").unwrap();
    assert!(!dispatch("dragover", &data).default_prevented());
    assert!(!dispatch("drop", &data).default_prevented());
    assert!(drops.next().now_or_never().is_none());
    drop(listener);
    assert!(drops.next().await.is_none());
    data.items()
        .add_with_file(&file("closed.png", &[1]))
        .unwrap();
    assert!(!dispatch("drop", &data).default_prevented());
}

#[wasm_bindgen_test(async)]
async fn successive_drops_keep_batch_order_and_a_read_failure_does_not_stop_imports() {
    let (_listener, mut drops) = listen_for_file_drops().unwrap();
    let data = web_sys::DataTransfer::new().unwrap();
    let broken = file("broken.png", &[1]);
    js_sys::Reflect::set(
        &broken,
        &"arrayBuffer".into(),
        &js_sys::Function::new_no_args("return Promise.reject(new Error('unreadable'))"),
    )
    .unwrap();
    data.items().add_with_file(&broken).unwrap();
    dispatch("drop", &data);
    data.items().clear().unwrap();
    data.items().add_with_file(&file("next.png", &[2])).unwrap();
    dispatch("drop", &data);

    let first = drops.next().await.unwrap().remove(0);
    assert!(import_dropped_file(first)
        .await
        .unwrap_err()
        .contains("broken.png"));
    let next = drops.next().await.unwrap().remove(0);
    let path = import_dropped_file(next).await.unwrap();
    assert_eq!(path.file_name().unwrap(), "next.png");
    assert_eq!(read_file(&path).unwrap().as_slice(), &[2]);
}
