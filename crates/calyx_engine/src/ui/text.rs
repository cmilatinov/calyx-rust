use super::{UiColor, UiPoint};

/// Bundled HUD faces. These select actual font files, never simulated weight or slant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiFont {
    #[default]
    Bold,
    BoldItalic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAnchor {
    #[default]
    TopLeft,
    TopCenter,
    Center,
    CenterLeft,
    CenterRight,
}

/// An outline is a dilation of the glyph coverage mask in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextOutline {
    pub width: f32,
    pub color: UiColor,
}

/// One shaped, single-line label. Alignment and ellipsis use backend font metrics.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLabel {
    pub position: UiPoint,
    pub text: String,
    pub font: UiFont,
    pub font_size: f32,
    /// Visual animation scale around the anchor; outlined glyph masks remain reusable.
    pub scale: f32,
    pub color: UiColor,
    pub anchor: TextAnchor,
    pub max_width: Option<f32>,
    pub outline: Option<TextOutline>,
}
