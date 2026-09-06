//! The chrome palette, and which of its two themes is on.

/// The chrome colours for one theme. Everything that isn't document
/// content draws from here; the active set is swapped by [`set_light`].
pub struct Palette {
    /// The window shell behind the panels.
    pub window_bg: u32,
    /// The area surrounding the document canvas.
    pub canvas_bg: u32,
    pub panel_bg: u32,
    /// Recessed strips: the document tab bar, the curve editor well.
    pub deep_bg: u32,
    pub status_bg: u32,
    pub ruler_bg: u32,
    pub field_bg: u32,
    pub popup_bg: u32,
    /// Small inline controls: step buttons, the active tab, badges.
    pub control_bg: u32,
    pub button_bg: u32,
    pub button_hover: u32,
    /// Row hover inside menus, popups and panels.
    pub hover: u32,
    /// Hairlines inside a panel (separators, section borders).
    pub divider: u32,
    /// Borders around fields, popups and modals.
    pub edge: u32,
    /// The border between panels and the shell.
    pub panel_edge: u32,
    /// Grid lines drawn on `deep_bg` (curve editor).
    pub grid: u32,
    pub text: u32,
    pub text_dim: u32,
    pub text_faint: u32,
    pub accent: u32,
    pub accent_hover: u32,
    /// Text and icons drawn on top of `accent`.
    pub accent_text: u32,
    /// Selected rows that keep their own text colour (lists, tiles).
    pub selection_bg: u32,
}

pub const DARK: Palette = Palette {
    window_bg: 0x1E1E1E,
    canvas_bg: 0x262626,
    panel_bg: 0x1A1A1A,
    deep_bg: 0x141414,
    status_bg: 0x161616,
    ruler_bg: 0x202020,
    field_bg: 0x0E0E0E,
    popup_bg: 0x242424,
    control_bg: 0x2A2A2A,
    button_bg: 0x333333,
    button_hover: 0x3E3E3E,
    hover: 0x2E2E2E,
    divider: 0x2A2A2A,
    edge: 0x3A3A3A,
    panel_edge: 0x111111,
    grid: 0x262626,
    text: 0xD8D8D8,
    text_dim: 0x9A9A9A,
    text_faint: 0x666666,
    accent: 0x3A6EA5,
    accent_hover: 0x4A80BC,
    accent_text: 0xFFFFFF,
    selection_bg: 0x2F5B8C,
};

pub const LIGHT: Palette = Palette {
    window_bg: 0xE8E8E8,
    canvas_bg: 0xB4B4B4,
    panel_bg: 0xF0F0F0,
    deep_bg: 0xE0E0E0,
    status_bg: 0xE4E4E4,
    ruler_bg: 0xE6E6E6,
    field_bg: 0xFFFFFF,
    popup_bg: 0xFAFAFA,
    control_bg: 0xD6D6D6,
    button_bg: 0xD0D0D0,
    button_hover: 0xC2C2C2,
    hover: 0xDCDCDC,
    divider: 0xD4D4D4,
    edge: 0xB8B8B8,
    panel_edge: 0xC4C4C4,
    grid: 0xC8C8C8,
    text: 0x1C1C1C,
    text_dim: 0x5A5A5A,
    text_faint: 0x9E9E9E,
    accent: 0x3A6EA5,
    accent_hover: 0x2E5E95,
    accent_text: 0xFFFFFF,
    selection_bg: 0xB8D2EE,
};

static LIGHT_THEME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Select the palette that [`palette`] returns. The application calls
/// this every frame from the persisted preference, so widgets built
/// during that render (and canvas paint callbacks after it) all agree.
pub fn set_light(light: bool) {
    LIGHT_THEME.store(light, std::sync::atomic::Ordering::Relaxed);
}

pub fn palette() -> &'static Palette {
    if is_light() {
        &LIGHT
    } else {
        &DARK
    }
}

/// Whether the light theme is active this frame, for chrome that keeps
/// its own palette (the gallery) but still follows the theme choice.
pub fn is_light() -> bool {
    LIGHT_THEME.load(std::sync::atomic::Ordering::Relaxed)
}
