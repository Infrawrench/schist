//! Frame animation commands: the Timeline panel's buttons and the Layer ▸
//! Animation menu, registered like every other command so they are also
//! keybindable, recordable in actions and published over MCP.

use schist_core::animation::{self, FrameResult, Refusal};
use schist_i18n::{t, tf};
use schist_plugin_api::{Command, CommandCtx, CommandPlugin};

pub struct AnimationCommands;

/// The user-facing reason a frame operation did nothing.
pub fn refusal_message(refusal: Refusal) -> Option<&'static str> {
    Some(t(match refusal {
        Refusal::NoAnimation => "animation.refuse.no_animation",
        Refusal::AlreadyAnimated => "animation.refuse.already_animated",
        Refusal::LastFrame => "animation.refuse.last_frame",
        Refusal::TooManyFrames => "animation.refuse.too_many_frames",
        Refusal::NoLayers => "animation.refuse.no_layers",
        Refusal::Nothing => return None,
    }))
}

fn report(ctx: &mut CommandCtx, result: FrameResult) {
    if let Err(refusal) = result {
        if let Some(why) = refusal_message(refusal) {
            ctx.refuse(why);
        }
    }
}

/// A command whose history entry is named by its own title.
fn frame_cmd(
    id: &'static str,
    title_key: &'static str,
    description_key: &'static str,
    run: fn(&mut CommandCtx, &str) -> FrameResult,
) -> Command {
    let title = t(title_key);
    Command {
        id,
        title,
        description: t(description_key),
        keybind: None,
        run: Box::new(move |ctx| {
            let result = run(ctx, title);
            report(ctx, result);
        }),
    }
}

/// Every animation command id, for the recorded-actions allow list.
pub const IDS: [&str; 11] = [
    "animation.create",
    "animation.make_frames_from_layers",
    "animation.flatten_frames_into_layers",
    "animation.new_frame",
    "animation.delete_frame",
    "animation.first_frame",
    "animation.previous_frame",
    "animation.next_frame",
    "animation.last_frame",
    "animation.reverse_frames",
    "animation.delete_animation",
];

impl CommandPlugin for AnimationCommands {
    fn commands(&self) -> Vec<Command> {
        vec![
            frame_cmd(
                "animation.create",
                "command.animation.create.title",
                "command.animation.create.description",
                |ctx, name| animation::create(ctx.doc, name),
            ),
            frame_cmd(
                "animation.make_frames_from_layers",
                "command.animation.make_frames_from_layers.title",
                "command.animation.make_frames_from_layers.description",
                |ctx, name| animation::make_frames_from_layers(ctx.doc, name),
            ),
            frame_cmd(
                "animation.flatten_frames_into_layers",
                "command.animation.flatten_frames_into_layers.title",
                "command.animation.flatten_frames_into_layers.description",
                |ctx, name| {
                    schist_animation::flatten_frames(ctx.doc, name, |i| {
                        tf!("animation.frame_layer_name", n = i + 1)
                    })
                },
            ),
            frame_cmd(
                "animation.new_frame",
                "command.animation.new_frame.title",
                "command.animation.new_frame.description",
                |ctx, name| animation::duplicate(ctx.doc, name),
            ),
            frame_cmd(
                "animation.delete_frame",
                "command.animation.delete_frame.title",
                "command.animation.delete_frame.description",
                |ctx, name| {
                    let current = ctx
                        .doc
                        .timeline
                        .as_ref()
                        .ok_or(Refusal::NoAnimation)?
                        .current;
                    animation::delete(ctx.doc, current, name)
                },
            ),
            frame_cmd(
                "animation.first_frame",
                "command.animation.first_frame.title",
                "command.animation.first_frame.description",
                |ctx, name| select_end(ctx, false, name),
            ),
            frame_cmd(
                "animation.previous_frame",
                "command.animation.previous_frame.title",
                "command.animation.previous_frame.description",
                |ctx, name| animation::step(ctx.doc, -1, name),
            ),
            frame_cmd(
                "animation.next_frame",
                "command.animation.next_frame.title",
                "command.animation.next_frame.description",
                |ctx, name| animation::step(ctx.doc, 1, name),
            ),
            frame_cmd(
                "animation.last_frame",
                "command.animation.last_frame.title",
                "command.animation.last_frame.description",
                |ctx, name| select_end(ctx, true, name),
            ),
            frame_cmd(
                "animation.reverse_frames",
                "command.animation.reverse_frames.title",
                "command.animation.reverse_frames.description",
                |ctx, name| animation::reverse(ctx.doc, name),
            ),
            frame_cmd(
                "animation.delete_animation",
                "command.animation.delete_animation.title",
                "command.animation.delete_animation.description",
                |ctx, name| animation::delete_animation(ctx.doc, name),
            ),
        ]
    }
}

fn select_end(ctx: &mut CommandCtx, last: bool, name: &str) -> FrameResult {
    let len = ctx
        .doc
        .timeline
        .as_ref()
        .ok_or(Refusal::NoAnimation)?
        .frames
        .len();
    animation::select(ctx.doc, if last { len - 1 } else { 0 }, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_core::{Document, Layer};
    use schist_plugin_api::EditorState;

    fn run(doc: &mut Document, id: &str) -> Option<String> {
        let commands = AnimationCommands.commands();
        let command = commands.iter().find(|c| c.id == id).unwrap();
        let mut state = EditorState::default();
        let mut ctx = CommandCtx {
            doc,
            state: &mut state,
            refusal: None,
        };
        (command.run)(&mut ctx);
        ctx.refusal
    }

    #[test]
    fn commands_drive_the_timeline_and_explain_refusals() {
        let ids: Vec<_> = AnimationCommands.commands().iter().map(|c| c.id).collect();
        assert_eq!(ids, IDS);
        let mut doc = Document::new("anim", 8, 8, schist_color::Depth::Eight);
        doc.push_layer(Layer::new_raster("a"));
        doc.push_layer(Layer::new_raster("b"));
        assert!(run(&mut doc, "animation.next_frame").is_some());
        assert!(run(&mut doc, "animation.make_frames_from_layers").is_none());
        assert!(
            run(&mut doc, "animation.create").is_some(),
            "already animated"
        );
        assert!(run(&mut doc, "animation.last_frame").is_none());
        assert_eq!(doc.timeline.as_ref().unwrap().current, 1);
        assert!(run(&mut doc, "animation.next_frame").is_none());
        assert_eq!(doc.timeline.as_ref().unwrap().current, 0, "wraps");
        assert!(run(&mut doc, "animation.flatten_frames_into_layers").is_none());
        assert_eq!(doc.tree.len(), 4);
        assert!(run(&mut doc, "animation.delete_frame").is_none());
        assert!(
            run(&mut doc, "animation.delete_frame").is_some(),
            "last frame"
        );
        assert!(run(&mut doc, "animation.delete_animation").is_none());
        assert!(doc.timeline.is_none());
        // Every step was one undoable edit.
        while doc.undo().is_some() {}
        assert!(doc.timeline.is_none());
        assert_eq!(doc.tree.len(), 2);
    }
}
