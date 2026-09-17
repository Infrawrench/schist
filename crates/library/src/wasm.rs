use wasm_bindgen::prelude::*;

/// Each JavaScript object owns its application. `free()` releases all its state.
#[wasm_bindgen(js_name = Schist)]
pub struct WasmApp(crate::App);

#[wasm_bindgen(js_class = Schist)]
impl WasmApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(crate::App::new())
    }

    pub fn request(&mut self, json: &str) -> Result<String, JsValue> {
        let bytes = self.0.request_json(json.as_bytes()).map_err(error)?;
        String::from_utf8(bytes).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[wasm_bindgen(js_name = importFile)]
    pub fn import_file(&mut self, name: &str, bytes: &[u8]) -> Result<u32, JsValue> {
        self.0.import(name, bytes).map_err(error)
    }

    #[wasm_bindgen(js_name = loadModel)]
    pub fn load_model(&mut self, id: &str, bytes: &[u8]) -> Result<(), JsValue> {
        self.0.people.load_model(id, bytes).map_err(error)
    }

    #[wasm_bindgen(js_name = exportFile)]
    pub fn export_file(
        &mut self,
        session: u32,
        extension: &str,
        bit_depth: u8,
    ) -> Result<Vec<u8>, JsValue> {
        if !matches!(bit_depth, 8 | 16 | 32) {
            return Err(JsValue::from_str("Invalid bit depth"));
        }
        self.0
            .export(
                session,
                extension,
                schist_plugin_api::ExportOptions {
                    bit_depth,
                    ..Default::default()
                },
            )
            .map_err(error)
    }
}

impl Default for WasmApp {
    fn default() -> Self {
        Self::new()
    }
}

fn error(error: anyhow::Error) -> JsValue {
    JsValue::from_str(&format!("{error:#}"))
}
