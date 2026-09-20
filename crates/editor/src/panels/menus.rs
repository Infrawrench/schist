//! The menu model: what each menu holds, and the filter, plug-in and
//! layer-comp entries built from the registry.

use super::*;
use schist_i18n::{t, t_in, tf, Locale};

pub(crate) enum MenuEntry {
    /// A registered plugin command (label + keybind resolved from registry).
    Cmd(&'static str),
    /// An app-level item handled by the shell.
    App(&'static str, AppItem, Option<&'static str>),
    /// Create an adjustment layer of this kind.
    Adjustment(schist_core::AdjustmentKind),
    /// Open a registered filter's dialog.
    Filter(&'static str),
    /// A nested menu, opened by hovering its row.
    Sub(&'static str, Vec<MenuEntry>),
    /// An app item whose label is not known at compile time -- the names
    /// of layer comps, for instance.
    Dynamic(String, AppItem),
    Sep,
}

/// A RAW-backed layer uses Camera Raw as a non-destructive development
/// workflow; everywhere else it remains the ordinary destructive filter.
/// Keep the menu label in step with the dialog title for both menu-bar
/// implementations.
pub(crate) fn filter_menu_label(ws: &Workspace, id: &str) -> String {
    if ws.is_raw_redevelopment(id) {
        t("menu.filter.camera_raw_development").to_string()
    } else {
        ws.registry
            .filters()
            .find(|filter| filter.id() == id)
            .map(|filter| format!("{}…", filter.name()))
            .unwrap_or_else(|| id.to_string())
    }
}

pub(crate) fn menus(ws: &Workspace) -> Vec<(&'static str, Vec<MenuEntry>)> {
    #[cfg(not(target_arch = "wasm32"))]
    let mut menus = if ws.gallery_open() {
        prune_sandboxed(gallery_menus(ws))
    } else {
        editor_menus(ws)
    };
    #[cfg(target_arch = "wasm32")]
    let mut menus = editor_menus(ws);
    if let Some((_, entries)) = menus.first_mut() {
        entries.insert(
            0,
            MenuEntry::App(t("common.search"), AppItem::Search, Some("cmd-shift-p")),
        );
        entries.insert(1, MenuEntry::Sep);
    }
    menus
}

/// Every available menu action, independent of which workspace is showing.
pub(crate) fn search_menus(ws: &Workspace) -> Vec<(&'static str, Vec<MenuEntry>)> {
    #[allow(unused_mut)]
    let mut menus = editor_menus(ws);
    #[cfg(not(target_arch = "wasm32"))]
    menus.extend(prune_sandboxed(gallery_menus(ws)));
    menus
}

fn editor_menus(ws: &Workspace) -> Vec<(&'static str, Vec<MenuEntry>)> {
    use AppItem::*;
    use MenuEntry::*;
    // `mut` for the desktop-only recents insertion below.
    #[allow(unused_mut)]
    let mut menus = vec![
        (
            t("menu.file"),
            vec![
                App(t("menu.file.new"), New, Some("cmd-n")),
                App(t("menu.file.open"), Open, Some("cmd-o")),
                App(
                    t("menu.file.browse_gallery"),
                    OpenGallery,
                    Some("cmd-shift-g"),
                ),
                App(t("menu.file.close"), Close, Some("cmd-w")),
                App(t("menu.file.save"), Save, Some("cmd-s")),
                App(t("menu.file.save_as"), SaveAs, Some("cmd-shift-s")),
                App(t("menu.file.export"), Export, Some("cmd-shift-alt-s")),
                Sep,
                Sub(
                    "Export",
                    vec![
                        App(t("menu.file.export_artboards"), ExportArtboards, None),
                        App(t("menu.file.export_slices"), ExportSlices, None),
                    ],
                ),
                Sep,
                App(t("menu.file.plugins"), Plugins, None),
                App(t("menu.file.missing_fonts"), ManageFonts, None),
                App(t("menu.file.check_for_updates"), CheckForUpdates, None),
                Sep,
                App(t("menu.file.quit"), Quit, Some("cmd-q")),
            ],
        ),
        (
            t("menu.edit"),
            vec![
                App(t("actions.title"), RecordedActions, None),
                Sep,
                Cmd("edit.undo"),
                Cmd("edit.redo"),
                Sep,
                Cmd("edit.cut"),
                Cmd("edit.copy"),
                Cmd("edit.copy_merged"),
                Cmd("edit.paste"),
                Cmd("edit.paste_in_place"),
                Sep,
                Cmd("edit.fill_foreground"),
                Cmd("edit.fill_background"),
                App(t("menu.edit.fill"), FillItem, Some("shift-f5")),
                App(t("menu.edit.stroke"), StrokeItem, None),
                App(t("menu.edit.content_aware_fill"), ContentAwareFill, None),
                App(
                    t("menu.edit.content_aware_scale"),
                    ContentAwareScaleItem,
                    None,
                ),
                App(t("menu.edit.puppet_warp"), PuppetWarpItem, None),
                Sep,
                App(t("menu.edit.free_transform"), FreeTransform, Some("cmd-t")),
                Sub(
                    "Transform",
                    vec![
                        App(t("menu.edit.rotate_180"), Rotate180, None),
                        App(t("menu.edit.rotate_90_cw"), RotateCw, None),
                        App(t("menu.edit.rotate_90_ccw"), RotateCcw, None),
                        Sep,
                        App(t("menu.edit.flip_horizontal"), FlipCanvasH, None),
                        App(t("menu.edit.flip_vertical"), FlipCanvasV, None),
                    ],
                ),
            ],
        ),
        (
            t("menu.image"),
            vec![
                Sub(
                    "Mode",
                    vec![
                        App(t("common.rgb"), ModeRgb, None),
                        App(t("common.grayscale"), ModeGrayscale, None),
                        App(t("common.cmyk"), ModeCmyk, None),
                        App(t("common.lab"), ModeLab, None),
                        App(t("common.indexed"), ModeIndexed, None),
                    ],
                ),
                Sub(
                    t("menu.image.adjustments"),
                    destructive_adjustment_entries(),
                ),
                Sep,
                App(t("menu.image.auto_tone"), AutoTone, None),
                App(t("menu.image.auto_contrast"), AutoContrast, None),
                App(t("menu.image.auto_color"), AutoColor, None),
                Sep,
                App(t("menu.image.image_size"), ImageSize, Some("cmd-alt-i")),
                App(t("menu.image.canvas_size"), CanvasSize, Some("cmd-alt-c")),
                Sub(
                    "Image Rotation",
                    vec![
                        App(t("menu.image.rotate_180"), Rotate180, None),
                        App(t("menu.image.rotate_90_cw"), RotateCw, None),
                        App(t("menu.image.rotate_90_ccw"), RotateCcw, None),
                        Sep,
                        App(t("menu.image.flip_canvas_horizontal"), FlipCanvasH, None),
                        App(t("menu.image.flip_canvas_vertical"), FlipCanvasV, None),
                    ],
                ),
                App(t("menu.image.crop_to_selection"), Crop, None),
                App(t("menu.image.trim"), Trim, None),
                Sep,
                App(t("menu.image.assign_profile"), AssignProfile, None),
                App(t("menu.image.convert_to_profile"), ConvertProfile, None),
            ],
        ),
        (
            t("menu.select"),
            vec![
                Cmd("select.all"),
                Cmd("select.deselect"),
                Cmd("select.reselect"),
                Cmd("select.inverse"),
                Sep,
                App(t("menu.select.color_range"), ColorRangeItem, None),
                App(t("mask_refine.title"), RefineMask, None),
                Sep,
                Sub(
                    "Modify",
                    vec![
                        App(t("menu.select.border"), SelectBorder, None),
                        App(t("menu.select.smooth"), SelectSmooth, None),
                        App(t("menu.select.expand"), SelectExpand, None),
                        App(t("menu.select.contract"), SelectContract, None),
                        App(t("menu.select.feather"), SelectFeatherItem, None),
                    ],
                ),
                Sep,
                App(
                    t("menu.select.transform_selection"),
                    TransformSelection,
                    None,
                ),
                Sep,
                Cmd("select.grow"),
                Cmd("select.similar"),
                Sep,
                Cmd("select.save"),
                Cmd("select.load"),
            ],
        ),
        (
            t("menu.layer"),
            vec![
                Cmd("layer.new"),
                Cmd("layer.duplicate"),
                Cmd("layer.delete"),
                Sep,
                Cmd("layer.smart_object"),
                App(t("smart.place_embedded"), SmartPlaceEmbedded, None),
                App(t("smart.place_linked"), SmartPlaceLinked, None),
                App(t("smart.edit_contents"), SmartEditContents, None),
                App(t("smart.replace_contents"), SmartReplaceContents, None),
                App(t("smart.relink"), SmartRelink, None),
                App(t("smart.update_linked"), SmartUpdateLinked, None),
                Cmd("layer.rasterize"),
                Sep,
                App(t("menu.layer.layer_style"), LayerStyleItem, None),
                Sep,
                Sub(t("menu.layer.layer_comps"), layer_comp_entries(ws)),
                Sep,
                Sub(
                    "Path",
                    vec![
                        App(t("menu.layer.fill_path"), PathFill, None),
                        App(t("menu.layer.stroke_path"), PathStroke, None),
                        App(t("menu.layer.make_selection"), PathToSelection, None),
                        Sep,
                        App(t("menu.layer.delete_path"), PathDelete, None),
                    ],
                ),
                Sep,
                Cmd("layer.group"),
                Cmd("layer.merge_down"),
                Cmd("layer.merge_visible"),
            ],
        ),
        (
            t("menu.adjust"),
            schist_adjustments::Params::creatable()
                .iter()
                .map(|&k| Adjustment(k))
                .collect(),
        ),
        (t("menu.filter"), {
            // Liquify and Vanishing Point sit above the categories, as
            // they do in Photoshop's Filter menu.
            let mut out = vec![
                App(t("menu.filter.filter_gallery"), FilterGalleryItem, None),
                Filter("filter.adaptive_wide_angle"),
                Filter("filter.camera_raw"),
                Filter("filter.lens_correction"),
                App(t("menu.filter.liquify"), LiquifyItem, None),
                App(t("menu.filter.vanishing_point"), VanishingPointItem, None),
                Sep,
            ];
            out.extend(filter_menu_entries(ws));
            out
        }),
        (
            t("menu.view"),
            vec![
                App(t("menu.view.rotate_view_cw"), RotateViewCw, None),
                App(t("menu.view.rotate_view_ccw"), RotateViewCcw, None),
                App(t("menu.view.reset_view"), ResetView, None),
                Sep,
                App(t("menu.view.zoom_in"), ZoomIn, Some("cmd-=")),
                App(t("menu.view.zoom_out"), ZoomOut, Some("cmd--")),
                App(t("menu.view.fit_on_screen"), ZoomFit, Some("cmd-0")),
                App(t("menu.view.actual_size"), ZoomActual, Some("cmd-1")),
                Sep,
                App(t("menu.view.rulers"), ToggleRulers, Some("cmd-r")),
                App(t("menu.view.grid"), ToggleGrid, Some("cmd-'")),
                App(t("menu.view.guides"), ToggleGuides, Some("cmd-;")),
                App(t("menu.view.notes"), ToggleNotes, None),
                App(t("menu.view.ai_panel"), ToggleAi, Some("cmd-shift-a")),
                App(t("menu.view.extras"), ToggleExtras, Some("cmd-h")),
                App(t("menu.view.snap"), ToggleSnap, Some("cmd-shift-;")),
                App(t("menu.view.clear_guides"), ClearGuides, Some("cmd-alt-;")),
                App(t("menu.view.clear_notes"), ClearNotes, None),
                App(t("menu.view.clear_count"), ClearCounts, None),
                Sep,
                App(t("menu.view.screen_mode"), ScreenModeItem, Some("f")),
                App(t("menu.view.proof_colors"), ProofColors, None),
                Sep,
                App(t("common.preferences"), Preferences, Some("cmd-k")),
            ],
        ),
    ];
    if let Some(cloud) = cloud_menu(
        crate::feature_enabled("schist-cloud"),
        ws.cloud.account.is_some(),
    ) {
        menus[0].1.insert(2, cloud);
    }
    // Open Recent, after Open…. Desktop only: browser paths are invented
    // per session, so a recents list would be a list of nothing.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let recents = recent_entries(ws);
        if !recents.is_empty() {
            menus[0]
                .1
                .insert(2, Sub(t("menu.file.open_recent"), recents));
        }
    }
    // The camera roll is where an iPad or iPhone keeps pictures, so the
    // File menu there can save straight to it, after Export.
    if cfg!(target_os = "ios") {
        if let Some((_, file)) = menus.iter_mut().find(|(name, _)| *name == t("menu.file")) {
            if let Some(at) = file.iter().position(|e| matches!(e, App(_, Export, _))) {
                file.insert(
                    at + 1,
                    App(t("menu.file.save_to_photos"), SaveToPhotos, None),
                );
            }
        }
    }
    prune_sandboxed(menus)
}

/// Drops the items whose whole subsystem is compiled out on the web
/// and on iOS: plug-in hosts (no subprocesses or JITs there), the
/// self-updater (a web deployment updates by serving newer files, an
/// iOS app through the store), the AI panel (drives locally installed
/// CLIs), and Quit (neither a tab nor an iOS app quits itself). The
/// gallery goes too on the web, which has no folders; iOS keeps it. A
/// menu item that answers "this does nothing here" is worse than no
/// item. Separators left leading, trailing or doubled go with them.
/// Both menu sets pass through here, the editor's and the gallery's.
#[cfg_attr(not(sandboxed), allow(clippy::needless_pass_by_value))]
fn prune_sandboxed(
    menus: Vec<(&'static str, Vec<MenuEntry>)>,
) -> Vec<(&'static str, Vec<MenuEntry>)> {
    #[cfg(not(sandboxed))]
    {
        menus
    }
    #[cfg(sandboxed)]
    {
        use AppItem::*;
        use MenuEntry::*;
        let mut menus = menus;
        for (_, entries) in &mut menus {
            entries.retain(|e| {
                !matches!(e, App(_, Plugins | CheckForUpdates | ToggleAi | Quit, _))
                    && !(cfg!(target_arch = "wasm32") && matches!(e, App(_, OpenGallery, _)))
            });
            entries.dedup_by(|a, b| matches!(a, Sep) && matches!(b, Sep));
            while matches!(entries.first(), Some(Sep)) {
                entries.remove(0);
            }
            while matches!(entries.last(), Some(Sep)) {
                entries.pop();
            }
        }
        menus
    }
}

/// The n-th recent files as menu rows.
#[cfg(not(target_arch = "wasm32"))]
fn recent_entries(ws: &Workspace) -> Vec<MenuEntry> {
    ws.library
        .recents
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            MenuEntry::Dynamic(label, AppItem::OpenRecent(i))
        })
        .collect()
}

/// The menu bar while the gallery is showing. Small on purpose: the
/// gallery browses and hands photos to the editor, it does not edit.
#[cfg(not(target_arch = "wasm32"))]
fn gallery_menus(ws: &Workspace) -> Vec<(&'static str, Vec<MenuEntry>)> {
    use AppItem::*;
    use MenuEntry::*;
    let mut file = vec![
        App(t("menu.file.new"), New, Some("cmd-n")),
        App(t("menu.file.open"), Open, Some("cmd-o")),
    ];
    if let Some(cloud) = cloud_menu(
        crate::feature_enabled("schist-cloud"),
        ws.cloud.account.is_some(),
    ) {
        file.push(cloud);
    }
    let recents = recent_entries(ws);
    if !recents.is_empty() {
        file.push(Sub(t("menu.file.open_recent"), recents));
    }
    file.extend([
        Sep,
        App(t("menu.file.add_folder_to_gallery"), GalleryAddFolder, None),
        App(
            if cfg!(target_os = "ios") {
                t("menu.file.import_from_photos")
            } else {
                t("menu.file.import_from_camera")
            },
            GalleryImportCamera,
            None,
        ),
        Sep,
        App(t("menu.file.quit"), Quit, Some("cmd-q")),
    ]);
    vec![
        (t("menu.file"), file),
        (
            t("menu.gallery"),
            vec![
                App(t("actions.title"), RecordedActions, None),
                App(t("menu.gallery.edit_selected"), GalleryEditSelected, None),
                App(t("menu.gallery.refresh"), GalleryRefresh, None),
                App(t("menu.gallery.map_filter"), GalleryMapFilter, None),
                Sep,
                // The content filter's model downloads live here too, so
                // turning the filter on never requires leaving the room.
                App(t("menu.filter.manage_models"), ManageModels, None),
                Sep,
                App(
                    t("menu.gallery.back_to_editor"),
                    OpenGallery,
                    Some("cmd-shift-g"),
                ),
            ],
        ),
        // On macOS Preferences sits in the application menu instead and
        // this menu converts to nothing; the native bar drops menus that
        // end up empty.
        (
            t("menu.view"),
            vec![
                App(t("menu.view.ai_panel"), ToggleAi, Some("cmd-shift-a")),
                Sep,
                App(t("common.preferences"), Preferences, Some("cmd-k")),
            ],
        ),
    ]
}

/// Filters grouped by category, in registration order.
pub(super) fn filter_menu_entries(ws: &Workspace) -> Vec<MenuEntry> {
    // The ids are static strings owned by the plugins; the menu resolves
    // names from the registry at render time. Categories nest, as in
    // Photoshop's Filter menu.
    let mut groups: Vec<MenuEntry> = FILTER_GROUPS
        .iter()
        .map(|(key, ids)| {
            let mut entries: Vec<MenuEntry> = ids.iter().map(|id| MenuEntry::Filter(id)).collect();
            // The Neural Filters need somewhere to fetch their models.
            if *key == "filter.category.neural" {
                entries.push(MenuEntry::Sep);
                entries.push(MenuEntry::App(
                    t("menu.filter.manage_models"),
                    AppItem::ManageModels,
                    None,
                ));
            }
            MenuEntry::Sub(t(key), entries)
        })
        .collect();
    add_photoshop_plugins(ws, &mut groups);
    groups
}

/// Fold Photoshop plug-ins into the category submenus, by the category
/// their own PiPL declares.
///
/// Straight into the Filter menu rather than under a "Photoshop" branch,
/// for two reasons. It is what Photoshop does — a plug-in declaring
/// "Blur" belongs beside the other blurs, and vendors choose their
/// category expecting exactly that. And the menu only nests one level:
/// a submenu inside a submenu cannot be reached with the mouse, so
/// grouping them under a wrapper would have put every plug-in one level
/// past where the pointer can go.
pub(super) fn add_photoshop_plugins(ws: &Workspace, groups: &mut Vec<MenuEntry>) {
    for filter in ws.registry.filters().filter(|f| f.runs_out_of_process()) {
        // A PiPL names its category in English whatever the language
        // of the menu, so "Blur" has to find the heading that reads
        // "Oskärpa": the built-in group whose English name it is, or
        // failing that a group already added under the plug-in's own
        // name.
        let category = FILTER_GROUPS
            .iter()
            .find(|(key, _)| t_in(Locale::En, key) == filter.category())
            .map(|(key, _)| t(key))
            .unwrap_or(filter.category());
        let existing = groups.iter_mut().find_map(|g| match g {
            MenuEntry::Sub(name, entries) if *name == category => Some(entries),
            _ => None,
        });
        match existing {
            Some(entries) => entries.push(MenuEntry::Filter(filter.id())),
            None => groups.push(MenuEntry::Sub(
                category,
                vec![MenuEntry::Filter(filter.id())],
            )),
        }
    }
}

/// The Layer Comps submenu: capture a new one, then the existing comps,
/// each of which applies on click and can be deleted from beside it.
pub(super) fn layer_comp_entries(ws: &Workspace) -> Vec<MenuEntry> {
    let mut out = vec![MenuEntry::App(
        t("menu.layer.new_layer_comp"),
        AppItem::NewLayerComp,
        None,
    )];
    let comps: Vec<String> = ws
        .doc
        .as_ref()
        .map(|d| d.layer_comps.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default();
    if !comps.is_empty() {
        out.push(MenuEntry::Sep);
        for (i, name) in comps.iter().enumerate() {
            out.push(MenuEntry::Dynamic(name.clone(), AppItem::ApplyLayerComp(i)));
        }
        out.push(MenuEntry::Sep);
        for (i, name) in comps.iter().enumerate() {
            out.push(MenuEntry::Dynamic(
                tf!("menu.layer.delete_layer_comp", name = name),
                AppItem::DeleteLayerComp(i),
            ));
        }
    }
    out
}

/// Image ▸ Adjustments: the same list as the Adjust menu, but applied to
/// the pixels rather than as a layer.
pub(super) fn destructive_adjustment_entries() -> Vec<MenuEntry> {
    schist_adjustments::Params::creatable()
        .iter()
        .filter(|k| !matches!(k, schist_core::AdjustmentKind::SolidColor))
        .map(|&k| {
            MenuEntry::App(
                crate::ui::adjustment_name(k),
                AppItem::ApplyAdjustment(k),
                None,
            )
        })
        .collect()
}

/// Menu grouping for the built-in filters: each category's string key
/// (`filters.lang`) and the filters under it.
pub(super) const FILTER_GROUPS: &[(&str, &[&str])] = &[
    // Photoshop's own order, which starts with 3D and puts Other last
    // before the Neural Filters.
    (
        "filter.category.3d",
        &["filter.bump_map", "filter.normal_map"],
    ),
    (
        "filter.category.artistic",
        &[
            "filter.colored_pencil",
            "filter.cutout",
            "filter.dry_brush",
            "filter.film_grain",
            "filter.fresco",
            "filter.neon_glow",
            "filter.paint_daubs",
            "filter.palette_knife",
            "filter.plastic_wrap",
            "filter.poster_edges",
            "filter.rough_pastels",
            "filter.smudge_stick",
            "filter.sponge",
            "filter.underpainting",
            "filter.watercolor",
        ],
    ),
    (
        "filter.category.blur",
        &[
            "filter.average",
            "filter.blur",
            "filter.blur_more",
            "filter.box_blur",
            "filter.gaussian_blur",
            "filter.lens_blur",
            "filter.motion_blur",
            "filter.radial_blur",
            "filter.shape_blur",
            "filter.smart_blur",
            "filter.surface_blur",
        ],
    ),
    (
        "filter.category.blur_gallery",
        &[
            "filter.field_blur",
            "filter.iris_blur",
            "filter.tilt_shift",
            "filter.path_blur",
            "filter.spin_blur",
        ],
    ),
    (
        "filter.category.brush_strokes",
        &[
            "filter.accented_edges",
            "filter.angled_strokes",
            "filter.crosshatch",
            "filter.dark_strokes",
            "filter.ink_outlines",
            "filter.spatter",
            "filter.sprayed_strokes",
            "filter.sumi_e",
        ],
    ),
    (
        "filter.category.distort",
        &[
            "filter.diffuse_glow",
            "filter.displace",
            "filter.glass",
            "filter.ocean_ripple",
            "filter.pinch",
            "filter.polar",
            "filter.ripple",
            "filter.shear",
            "filter.spherize",
            "filter.twirl",
            "filter.wave",
            "filter.zigzag",
        ],
    ),
    (
        "filter.category.noise",
        &[
            "filter.add_noise",
            "filter.despeckle",
            "filter.dust_scratches",
            "filter.median",
            "filter.reduce_noise",
        ],
    ),
    (
        "filter.category.pixelate",
        &[
            "filter.color_halftone",
            "filter.crystallize",
            "filter.facet",
            "filter.fragment",
            "filter.mezzotint",
            "filter.mosaic",
            "filter.pointillize",
        ],
    ),
    (
        "filter.category.render",
        &[
            "filter.flame",
            "filter.picture_frame",
            "filter.tree",
            "filter.clouds",
            "filter.difference_clouds",
            "filter.fibers",
            "filter.lens_flare",
            "filter.lighting_effects",
        ],
    ),
    (
        "filter.category.sharpen",
        &[
            "filter.sharpen",
            "filter.sharpen_edges",
            "filter.sharpen_more",
            "filter.smart_sharpen",
            "filter.unsharp_mask",
        ],
    ),
    (
        "filter.category.sketch",
        &[
            "filter.bas_relief",
            "filter.chalk_charcoal",
            "filter.charcoal",
            "filter.chrome",
            "filter.conte_crayon",
            "filter.graphic_pen",
            "filter.halftone_pattern",
            "filter.note_paper",
            "filter.photocopy",
            "filter.plaster",
            "filter.reticulation",
            "filter.stamp",
            "filter.torn_edges",
            "filter.water_paper",
        ],
    ),
    (
        "filter.category.stylize",
        &[
            "filter.diffuse",
            "filter.emboss",
            "filter.extrude",
            "filter.find_edges",
            "filter.glowing_edges",
            "filter.oil_paint",
            "filter.solarize",
            "filter.tiles",
            "filter.trace_contour",
            "filter.wind",
        ],
    ),
    (
        "filter.category.texture",
        &[
            "filter.craquelure",
            "filter.grain",
            "filter.mosaic_tiles",
            "filter.patchwork",
            "filter.stained_glass",
            "filter.texturizer",
        ],
    ),
    (
        "filter.category.video",
        &["filter.deinterlace", "filter.ntsc_colors"],
    ),
    (
        "filter.category.other",
        &[
            "filter.custom",
            "filter.high_pass",
            "filter.hsb_hsl",
            "filter.maximum",
            "filter.minimum",
            "filter.offset",
        ],
    ),
    (
        "filter.category.neural",
        &[
            "filter.neural.style_transfer",
            "filter.neural.skin_smoothing",
            "filter.neural.jpeg_artifacts",
            "filter.neural.colorize",
            "filter.neural.super_zoom",
            "filter.neural.color_transfer",
            "filter.neural.depth_blur",
            "filter.neural.harmonization",
            "filter.neural.landscape_mixer",
            "filter.neural.photo_restoration",
            "filter.neural.photo_to_sketch",
            "filter.neural.face_to_caricature",
            "filter.neural.smart_portrait",
            "filter.neural.makeup_transfer",
            "filter.neural.sketch_to_portrait",
        ],
    ),
];
fn cloud_menu(enabled: bool, signed_in: bool) -> Option<MenuEntry> {
    use AppItem::*;
    use MenuEntry::*;
    if !enabled {
        return None;
    }
    let entries = if signed_in {
        vec![
            App(t("menu.cloud.browse"), CloudBrowse, None),
            App(t("menu.cloud.generate"), CloudGenerate, None),
            App(t("menu.cloud.upload"), CloudUpload, None),
            App(t("menu.cloud.sign_out"), CloudSignOut, None),
        ]
    } else {
        vec![App(t("menu.cloud.sign_in"), CloudSignIn, None)]
    };
    Some(Sub(t("menu.file.schist_cloud"), entries))
}

#[cfg(test)]
mod feature_flag_tests {
    use super::*;

    #[test]
    fn cloud_menu_is_absent_when_disabled_even_with_a_saved_account() {
        assert!(cloud_menu(false, false).is_none());
        assert!(cloud_menu(false, true).is_none());
    }

    #[test]
    fn cloud_menu_restores_sign_in_and_account_actions_when_enabled() {
        let Some(MenuEntry::Sub(_, signed_out)) = cloud_menu(true, false) else {
            panic!("missing Cloud menu");
        };
        assert!(matches!(
            signed_out.as_slice(),
            [MenuEntry::App(_, AppItem::CloudSignIn, _)]
        ));
        let Some(MenuEntry::Sub(_, signed_in)) = cloud_menu(true, true) else {
            panic!("missing Cloud menu");
        };
        assert!(matches!(
            signed_in.as_slice(),
            [
                MenuEntry::App(_, AppItem::CloudBrowse, _),
                MenuEntry::App(_, AppItem::CloudGenerate, _),
                MenuEntry::App(_, AppItem::CloudUpload, _),
                MenuEntry::App(_, AppItem::CloudSignOut, _),
            ]
        ));
    }
}
