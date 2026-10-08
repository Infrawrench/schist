use schist_layout::{authoring, blank_a4, History, ObjectId, Rect};

#[test]
fn creation_order_survives_stacking_deletion_and_exact_single_step_creation_undo() {
    for count in [1, 2, 7] {
        let mut doc = blank_a4();
        let mut history = History::default();
        let blank = doc.clone();
        let mut ids = Vec::new();
        for index in 0..count {
            let before = doc.clone();
            let depth = history.undo_depth();
            let frame = authoring::text_frame(
                &mut doc,
                &mut history,
                0,
                Rect::new(20.0 + index as f32, 30.0, 100.0, 100.0),
            )
            .unwrap();
            assert_eq!(history.undo_depth(), depth + 1);
            let created = doc.clone();
            assert!(history.undo(&mut doc));
            assert_eq!(doc, before);
            assert!(history.redo(&mut doc));
            assert_eq!(doc, created);
            ids.push(frame.object);
        }
        assert_eq!(doc.creation_order, ids);
        doc.objects.reverse();
        for (rank, id) in ids.iter().enumerate() {
            assert_eq!(doc.creation_rank(*id), Some(rank));
        }
        let before = doc.clone();
        let depth = history.undo_depth();
        assert!(authoring::delete_all(&mut doc, &mut history, &ids));
        assert_eq!(history.undo_depth(), depth + 1);
        assert_eq!(doc.creation_order, ids);
        assert!(ids.iter().all(|id| doc.creation_rank(*id).is_none()));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        // Restore the paint order before reversing the indexed creation edits.
        doc.objects.reverse();
        while history.can_undo() {
            assert!(history.undo(&mut doc));
        }
        assert_eq!(doc, blank);
    }
}

#[test]
fn missing_or_ambiguous_creation_evidence_never_guesses_from_numeric_ids() {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 30.0, 100.0, 100.0),
    )
    .unwrap();
    assert_eq!(doc.creation_rank(frame.object), Some(0));
    doc.creation_order.clear();
    assert_eq!(doc.creation_rank(frame.object), None);
    doc.creation_order = vec![frame.object, frame.object];
    assert_eq!(doc.creation_rank(frame.object), None);
    doc.creation_order = vec![ObjectId(u32::MAX), frame.object];
    assert_eq!(doc.creation_rank(frame.object), Some(1));
    doc.objects.push(doc.objects[0].clone());
    assert_eq!(doc.creation_rank(frame.object), None);
}

#[test]
fn batched_chronology_keeps_the_same_evidence_rules_through_aliases_and_tombstones() {
    for count in [2, 7, 63] {
        let mut doc = blank_a4();
        let mut ids = Vec::new();
        for _ in 0..count {
            ids.push(
                authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(20.0, 30.0, 100.0, 100.0),
                )
                .unwrap()
                .object,
            );
        }
        for mutation in 0..5 {
            match mutation {
                0 => doc.objects.reverse(),
                1 => doc.creation_order.push(ids[0]),
                2 => {
                    let duplicate = doc.object(ids[1]).unwrap().clone();
                    doc.objects.push(duplicate);
                }
                3 => doc.objects.retain(|object| object.id != ids[0]),
                4 => doc.creation_order.insert(0, ObjectId(u32::MAX)),
                _ => unreachable!(),
            }
            let ranks = doc.creation_ranks();
            for id in &ids {
                assert_eq!(ranks.get(id).copied(), doc.creation_rank(*id));
            }
            assert!(!ranks.contains_key(&ObjectId(u32::MAX)));
            for id in ids.iter().skip(2) {
                assert!(ranks.contains_key(id));
            }
        }
    }
}
