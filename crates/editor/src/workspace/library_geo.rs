//! Bind the independent map view to the editor's three map slots.
use super::*;
pub use schist_map_view::*;

impl Workspace {
    pub(super) fn map_mut(&mut self, slot: MapSlot) -> &mut MapState {
        match slot {
            MapSlot::Gallery => &mut self.library.map,
            MapSlot::World => &mut self.library.world_map,
            MapSlot::Info => &mut self.info_map,
        }
    }
    pub(super) fn prepare_map_paint(
        &mut self,
        slot: MapSlot,
        bounds: Bounds<Pixels>,
        scale: f32,
    ) -> MapPaint {
        self.map_mut(slot).prepare_paint(bounds, scale)
    }
    pub(super) fn kick_map_tiles(&mut self, slot: MapSlot, cx: &mut Context<Self>) {
        self.map_mut(slot)
            .load_tiles(move |ws| ws.map_mut(slot), cx);
    }
}
