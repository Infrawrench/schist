//! Three-way merges preserve independent saves, deletions and conflicting copies.
use anyhow::{ensure, Result};
use serde_json::Value;
use std::collections::BTreeMap;
pub type Libraries = BTreeMap<String, Value>;
pub fn merge(kind: &str, base: Option<&Value>, local: &Value, remote: &Value) -> Result<Value> {
    let key = match kind {
        "brushes" => "presets",
        "actions" => "actions",
        "export_recipes" => "recipes",
        _ => anyhow::bail!(schist_i18n::t("common.unsupported_format")),
    };
    fn entries(value: &Value, key: &str) -> Result<BTreeMap<String, Value>> {
        let rows = value
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!(schist_i18n::t("common.unsupported_format")))?;
        let mut entries = BTreeMap::new();
        for row in rows {
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!(schist_i18n::t("common.unsupported_format")))?;
            let mut name = name.to_string();
            let root = name.clone();
            let mut suffix = 2;
            while entries.contains_key(&name) {
                name = format!("{root} ({suffix})");
                suffix += 1;
            }
            let mut row = row.clone();
            row["name"] = name.clone().into();
            entries.insert(name, row);
        }
        Ok(entries)
    }
    let base = base
        .map(|b| entries(b, key))
        .transpose()?
        .unwrap_or_default();
    let local = entries(local, key)?;
    let mut merged = entries(remote, key)?;
    for (name, old) in &base {
        if !local.contains_key(name) && merged.get(name) == Some(old) {
            merged.remove(name);
        }
    }
    for (name, row) in local {
        if base.get(&name) == Some(&row) {
            continue;
        }
        match merged.get(&name) {
            Some(other) if other == &row => {}
            Some(other) if base.get(&name) != Some(other) => {
                let mut suffix = 2;
                let mut copy = format!("{name} ({suffix})");
                let mut existing_copy = false;
                while let Some(existing) = merged.get(&copy) {
                    let mut renamed = row.clone();
                    renamed["name"] = copy.clone().into();
                    if existing == &renamed {
                        existing_copy = true;
                        break;
                    }
                    suffix += 1;
                    copy = format!("{name} ({suffix})");
                }
                if existing_copy {
                    continue;
                }
                let mut row = row;
                row["name"] = copy.clone().into();
                merged.insert(copy, row);
            }
            _ => {
                merged.insert(name, row);
            }
        }
    }
    ensure!(
        merged.len() <= if kind == "brushes" { 128 } else { 256 },
        schist_i18n::t("actions.library_too_large")
    );
    let mut result = remote.clone();
    result[key] = Value::Array(merged.into_values().collect());
    if kind == "export_recipes" {
        result["selected"] = 0.into();
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn book(rows: Value) -> Value {
        json!({"schema":1,"actions":rows})
    }
    #[test]
    fn concurrent_changes_keep_both_copies_and_independent_deletions() {
        let base = book(json!([{"name":"A","steps":[1]},{"name":"B","steps":[]}]));
        let local = book(json!([{"name":"A","steps":[2]}]));
        let remote =
            book(json!([{"name":"A","steps":[3]},{"name":"B","steps":[]},{"name":"C","steps":[]}]));
        let merged = merge("actions", Some(&base), &local, &remote).unwrap();
        assert_eq!(
            merged["actions"],
            json!([{"name":"A","steps":[3]},{"name":"A (2)","steps":[2]},{"name":"C","steps":[]}])
        )
    }
    #[test]
    fn retry_after_lost_acknowledgement_does_not_duplicate_conflicting_copy() {
        let base = book(json!([{ "name": "A", "steps": [1] }]));
        let local = book(json!([{ "name": "A", "steps": [2] }]));
        let remote = book(json!([{ "name": "A", "steps": [3] }]));
        let merged = merge("actions", Some(&base), &local, &remote).unwrap();
        assert_eq!(
            merge("actions", Some(&base), &local, &merged).unwrap(),
            merged
        );
    }
    #[test]
    fn first_sync_unions_and_unchanged_local_does_not_resurrect_remote_deletion() {
        let local = book(json!([{"name":"A","steps":[]}]));
        let remote = book(json!([]));
        assert_eq!(merge("actions", None, &local, &remote).unwrap(), local);
        assert_eq!(
            merge("actions", Some(&local), &local, &remote).unwrap(),
            remote
        );
    }
    #[test]
    fn remote_edit_survives_local_deletion() {
        let base = book(json!([{"name":"A","steps":[]}]));
        let remote = book(json!([{"name":"A","steps":[3]}]));
        assert_eq!(
            merge("actions", Some(&base), &book(json!([])), &remote).unwrap(),
            remote
        );
    }
}
