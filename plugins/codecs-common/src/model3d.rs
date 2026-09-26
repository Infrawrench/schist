use super::*;
pub struct ModelCodec(pub &'static str);
impl CodecPlugin for ModelCodec {
    fn id(&self) -> &'static str {
        match self.0 {
            "glb" => "codec.glb",
            "obj" => "codec.obj",
            _ => "codec.stl",
        }
    }
    fn name(&self) -> &'static str {
        match self.0 {
            "glb" => t("model3d.format.glb"),
            "obj" => t("model3d.format.obj"),
            _ => t("model3d.format.stl"),
        }
    }
    fn extensions(&self) -> &'static [&'static str] {
        match self.0 {
            "glb" => &["glb"],
            "obj" => &["obj"],
            _ => &["stl"],
        }
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        self.0 == "glb" && bytes.starts_with(b"glTF")
    }
    fn import(&self, bytes: &[u8]) -> anyhow::Result<Document> {
        let mesh = schist_model3d::import(bytes, self.0)?;
        let mut doc = Document::new(t("tool.model3d.name"), 1024, 1024, Depth::Eight);
        let model = schist_core::model3d::Model3d {
            mesh,
            placement: schist_core::model3d::Placement::fitted(doc.width, doc.height),
        };
        let layer =
            schist_model3d::layer(&model, doc.depth, doc.canvas_rect(), t("tool.model3d.name"))?;
        doc.push_layer(layer);
        doc.damage_all();
        doc.mark_saved();
        Ok(doc)
    }
}
