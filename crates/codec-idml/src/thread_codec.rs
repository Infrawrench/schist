//! Native frame references are document-wide, including across spreads.
use crate::import::Report;
use schist_layout::{LayoutDocument, LayoutObject, ObjectId, StoryId};

#[derive(Clone)]
pub(crate) struct FrameReference {
    pub external: String,
    pub next: Option<String>,
    pub object: ObjectId,
}

pub(crate) fn resolve(doc: &mut LayoutDocument, refs: &[FrameReference], report: &mut Report) {
    for story_index in 0..doc.stories.len() {
        let story = StoryId(story_index as u32);
        let frames:Vec<_>=refs.iter().filter(|r|matches!(doc.object(r.object).or_else(|| doc.parents.iter().flat_map(|p| &p.objects).find(|o| o.object.id == r.object).map(|o| &o.object)).map(|o| &o.object),Some(LayoutObject::TextFrame {story:s,..}) if *s==story)).collect();
        if frames.is_empty() {
            continue;
        }
        let mut order = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let heads = frames.iter().filter(|r| {
            !frames
                .iter()
                .any(|other| other.next.as_deref() == Some(&r.external))
        });
        for start in heads.chain(frames.iter()) {
            let mut current = Some(*start);
            while let Some(frame) = current {
                if !visited.insert(frame.object) {
                    break;
                }
                order.push(frame.object);
                current = frame
                    .next
                    .as_ref()
                    .and_then(|next| frames.iter().find(|r| r.external == *next).copied());
                if frame.next.is_some()
                    && (current.is_none() || current.is_some_and(|r| visited.contains(&r.object)))
                {
                    report.skip(schist_i18n::t("design.idml_bad_thread").to_string());
                }
            }
        }
        doc.thread_order.push((story, order));
    }
}
