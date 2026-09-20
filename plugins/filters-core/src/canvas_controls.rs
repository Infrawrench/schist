//! Canvas metadata lives beside the algorithms; the editor does not match IDs.
use schist_plugin_api::{
    filter_canvas::{FilterCanvasControl as Control, RadiusScale},
    FilterValues,
};
pub fn controls(id: &str, v: &FilterValues) -> Vec<Control> {
    let control = match id {
        "filter.radial_blur" | "filter.lens_flare" => Control::Point { x: "x", y: "y" },
        "filter.iris_blur" | "filter.spin_blur" => Control::Ellipse {
            x: "x",
            y: "y",
            radius: "radius",
            scale: if id == "filter.iris_blur" {
                RadiusScale::ShortSide
            } else {
                RadiusScale::LongSide
            },
            roundness: (id == "filter.iris_blur").then_some("roundness"),
            feather: Some("feather"),
        },
        "filter.field_blur" => Control::Band {
            position: "position",
            angle: "angle",
            normal_offset: 0.0,
            width: None,
            feather: "spread",
        },
        "filter.tilt_shift" => Control::Band {
            position: "position",
            angle: "angle",
            normal_offset: 90.0,
            width: Some("band"),
            feather: "feather",
        },
        "filter.lighting_effects" if v.get("type").round() >= 2.0 => {
            Control::Direction { angle: "angle" }
        }
        "filter.lighting_effects" => Control::Ellipse {
            x: "x",
            y: "y",
            radius: "spread",
            scale: RadiusScale::Diagonal,
            roundness: None,
            feather: None,
        },
        "filter.path_blur" => Control::Direction { angle: "angle" },
        _ => return Vec::new(),
    };
    vec![control]
}
