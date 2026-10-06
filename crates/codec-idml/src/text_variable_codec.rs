//! Exact text-variable definitions remain shared recovery data alongside typed
//! custom definitions. Cached instances never become literal source text.
//! Native definitions supersede retained definitions with the same opaque Self;
//! equal display names do not identify a resource. The custom-text codec guards
//! its supported definitions; unknown preferences remain opaque and unresolved.
use crate::{designmap::DesignPackage, error::Error, export::escape, import::Report, xml};

const LABEL: &str = "Schist.TextVariables.v1";

pub(crate) fn read(
    opened: &DesignPackage<'_>,
    custom: &super::custom_text_codec::Imported,
    report: &mut Report,
) -> Result<Vec<String>, Error> {
    let root = xml::parse(opened.text_of(&opened.root)?).map_err(|message| Error::Xml {
        part: opened.root.clone(),
        message,
    })?;
    let mut archive = Vec::new();
    for entry in root
        .children_named("Properties")
        .flat_map(|p| p.children_named("Label"))
        .flat_map(|label| label.children_named("KeyValuePair"))
        .filter(|entry| entry.attr("Key") == Some(LABEL))
    {
        let value = entry.attr("Value").unwrap_or_default();
        match serde_json::from_str::<Vec<String>>(value) {
            Ok(values) => archive.extend(values),
            Err(_) => archive.push(format!(
                r#"<Properties><Label><KeyValuePair Key="{LABEL}" Value="{}"/></Label></Properties>"#,
                escape(value)
            )),
        }
    }
    let mut retained = Vec::new();
    let mut replaced = std::collections::BTreeSet::new();
    for native in root.children_named("TextVariable") {
        let id = native.attr("Self").unwrap_or_default();
        let model_id = custom.references.get(id).map_or(id, String::as_str);
        let definition = custom.definitions.iter().find(|d| d.id == model_id);
        // Preserve exact original spelling/order only when the live typed
        // definition still agrees. Native edits replace stale recovery data.
        let agrees = definition.is_some_and(|definition| {
            archive.iter().any(|raw| {
                xml::parse(raw)
                    .ok()
                    .and_then(|e| super::custom_text_codec::definition(&e))
                    .as_ref()
                    == Some(definition)
            })
        });
        if agrees {
            continue;
        }
        if let Some(raw) = &native.raw {
            retained.push(raw.to_string());
        }
        if !id.is_empty() {
            replaced.insert(id.to_owned());
        }
        if !model_id.is_empty() {
            replaced.insert(model_id.to_owned());
        }
    }
    retained.extend(archive.into_iter().filter(|raw| {
        !xml::parse(raw).ok().is_some_and(|element| {
            element.name == "TextVariable"
                && element.attr("Self").is_some_and(|id| replaced.contains(id))
        })
    }));
    // Definitions are kept as written for an exact save. An instance of one
    // no typed definition covers is reported where it is read
    // (`import::report_untyped`); unreadable metadata is reported here.
    if !custom.recovery.is_empty() {
        notice(&mut report.skipped);
    }
    retained.extend(custom.recovery.iter().cloned());
    Ok(retained)
}

pub(crate) fn retain(
    definitions: &[String],
    custom_label: &str,
    warnings: &mut Vec<String>,
) -> String {
    if definitions.is_empty() {
        return if custom_label.is_empty() {
            String::new()
        } else {
            format!("<Properties><Label>{custom_label}</Label></Properties>")
        };
    }
    notice(warnings);
    let value = serde_json::to_string(definitions).expect("text-variable recovery data");
    format!(
        r#"<Properties><Label><KeyValuePair Key="{LABEL}" Value="{}"/>{custom_label}</Label></Properties>"#,
        escape(&value)
    )
}

pub(crate) fn notice(warnings: &mut Vec<String>) {
    let message = schist_i18n::t("design.idml_story_structure").to_string();
    if !warnings.contains(&message) {
        warnings.push(message);
    }
}
