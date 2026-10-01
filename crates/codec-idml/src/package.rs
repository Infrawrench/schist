//! Portable IDML delivery package. Source documents and link paths are never
//! edited; the archive gets a relinked copy and original artwork bytes.
use schist_layout::{LayoutDocument, LayoutObject};
use std::collections::BTreeMap;

pub const MAX_PACKAGE_BYTES: usize = 512 * 1024 * 1024;
pub struct Packaged {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

pub fn build(
    document: &LayoutDocument,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Packaged, String> {
    let mut copy = document.clone();
    let mut files = Vec::new();
    let mut names = BTreeMap::new();
    let mut links = Vec::new();
    let mut size = 0usize;
    for object in copy.objects.iter_mut().chain(
        copy.parents
            .iter_mut()
            .flat_map(|p| p.objects.iter_mut().map(|o| &mut o.object)),
    ) {
        let LayoutObject::GraphicFrame {
            link,
            embedded: false,
            ..
        } = &mut object.object
        else {
            continue;
        };
        let packaged = if let Some(name) = names.get(&link.path) {
            String::clone(name)
        } else {
            let bytes = read(&link.path)?;
            size = size
                .checked_add(bytes.len())
                .filter(|s| *s <= MAX_PACKAGE_BYTES)
                .ok_or_else(|| schist_i18n::t("design.package_too_large").to_string())?;
            let basename = link
                .path
                .rsplit(['/', '\\'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("artwork");
            let basename: String = basename
                .chars()
                .map(|c| {
                    if c.is_control() || ":<>\"|?*".contains(c) {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            let name = format!("Links/{:05}-{basename}", names.len() + 1);
            links.push(serde_json::json!({"source":link.path,"packaged":name,"bytes":bytes.len()}));
            files.push((name.clone(), bytes));
            names.insert(link.path.clone(), name.clone());
            name
        };
        link.path = packaged;
        link.present = true;
    }
    let exported = crate::export::write(&copy);
    let faces = crate::export::font_inventory(&copy);
    let fonts: std::collections::BTreeSet<_> = faces.iter().map(|(family, _)| family).collect();
    let font_styles: Vec<_> = faces
        .iter()
        .map(|(family, style)| serde_json::json!({"family":family,"style":style}))
        .collect();
    let manifest = serde_json::json!({"format":"schist-idml-package","version":1,"document":"layout.idml","links":links,"font_families":fonts,"font_styles":font_styles,"fonts_included":false,"warnings":exported.warnings});
    files.push(("layout.idml".into(), exported.bytes));
    files.push((
        "manifest.json".into(),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    ));
    if files
        .iter()
        .try_fold(0usize, |n, (_, b)| n.checked_add(b.len()))
        .is_none_or(|n| n > MAX_PACKAGE_BYTES)
    {
        return Err(schist_i18n::t("design.package_too_large").to_string());
    }
    Ok(Packaged {
        bytes: crate::container::write(&files),
        warnings: exported.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_basenames_and_repeated_links_remain_portable_without_editing_the_source() {
        let mut doc = schist_layout::blank_a4();
        for path in ["/a/picture.psd", "/b/picture.psd", "/a/picture.psd"] {
            schist_layout::authoring::graphic_frame(
                &mut doc,
                &mut Default::default(),
                0,
                schist_layout::Rect::new(0.0, 0.0, 50.0, 50.0),
                path,
                false,
            )
            .unwrap();
        }
        let before = doc.clone();
        let mut reads = 0;
        let package = build(&doc, |path| {
            reads += 1;
            Ok(path.as_bytes().to_vec())
        })
        .unwrap();
        assert_eq!(doc, before);
        assert_eq!(reads, 2);
        let zip = crate::container::read(&package.bytes).unwrap();
        let restored = crate::import::read(zip.get("layout.idml").unwrap())
            .unwrap()
            .document;
        for (original, placed) in doc.objects.iter().zip(&restored.objects) {
            let LayoutObject::GraphicFrame { link: old, .. } = &original.object else {
                panic!()
            };
            let LayoutObject::GraphicFrame { link: new, .. } = &placed.object else {
                panic!()
            };
            assert!(new.path.starts_with("Links/"));
            assert_eq!(zip.get(&new.path).unwrap(), old.path.as_bytes());
        }
    }
    #[test]
    fn an_unavailable_link_refuses_the_package_without_a_partial_result() {
        let mut doc = schist_layout::blank_a4();
        schist_layout::authoring::graphic_frame(
            &mut doc,
            &mut Default::default(),
            0,
            schist_layout::Rect::new(0.0, 0.0, 50.0, 50.0),
            "missing.psd",
            false,
        )
        .unwrap();
        assert!(build(&doc, |_| Err("missing".into())).is_err());
    }
}
