//! Text-variable definitions are shared recovery data. Their cached instances
//! are anchored story structures, not literal text or evaluated live variables.
//! Native definitions supersede retained definitions with the same opaque Self;
//! equal display names do not identify a resource. Retention never interprets
//! preferences or emits unresolved native resource references.
use crate::{designmap::DesignPackage, error::Error, export::escape, import::Report, xml};

const LABEL: &str = "Schist.TextVariables.v1";

pub(crate) fn read(opened: &DesignPackage<'_>, report: &mut Report) -> Result<Vec<String>, Error> {
    let root = xml::parse(opened.text_of(&opened.root)?).map_err(|message| Error::Xml {
        part: opened.root.clone(),
        message,
    })?;
    let native: Vec<_> = root.children_named("TextVariable").collect();
    let ids: std::collections::BTreeSet<_> = native
        .iter()
        .filter_map(|element| element.attr("Self"))
        .filter(|id| !id.is_empty())
        .collect();
    let mut retained: Vec<String> = native
        .iter()
        .filter_map(|element| element.raw.as_ref().map(ToString::to_string))
        .collect();
    for entry in root
        .children_named("Properties")
        .flat_map(|p| p.children_named("Label"))
        .flat_map(|label| label.children_named("KeyValuePair"))
        .filter(|entry| entry.attr("Key") == Some(LABEL))
    {
        let value = entry.attr("Value").unwrap_or_default();
        match serde_json::from_str::<Vec<String>>(value) {
            Ok(values) => retained.extend(values.into_iter().filter(|raw| {
                // Only live native resources can replace archived definitions.
                // Do not deduplicate unknown/malformed entries or recursively
                // interpret a retained metadata wrapper.
                !xml::parse(raw).ok().is_some_and(|element| {
                    element.name == "TextVariable"
                        && element.attr("Self").is_some_and(|id| ids.contains(id))
                })
            })),
            Err(_) => retained.push(format!(
                r#"<Properties><Label><KeyValuePair Key="{LABEL}" Value="{}"/></Label></Properties>"#,
                escape(value)
            )),
        }
    }
    if !retained.is_empty() {
        notice(&mut report.skipped);
    }
    Ok(retained)
}

pub(crate) fn retain(definitions: &[String], warnings: &mut Vec<String>) -> String {
    if definitions.is_empty() {
        return String::new();
    }
    notice(warnings);
    let value = serde_json::to_string(definitions).expect("text-variable recovery data");
    format!(
        r#"<Properties><Label><KeyValuePair Key="{LABEL}" Value="{}"/></Label></Properties>"#,
        escape(&value)
    )
}

fn notice(warnings: &mut Vec<String>) {
    let message = schist_i18n::t("design.idml_story_structure").to_string();
    if !warnings.contains(&message) {
        warnings.push(message);
    }
}
