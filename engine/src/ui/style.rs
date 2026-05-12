use std::collections::HashMap;

use crate as engine;
use crate::reflect::ReflectDefault;
use crate::utils::TypeUuid;
use engine_derive::Reflect;
use serde::{Deserialize, Serialize};

use super::geometry::EdgeInsets;

/// RGBA color stored independently from any rendering backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "79124a2c-87b6-40bb-a974-1ec15c830c70"]
#[reflect(Default)]
#[repr(C)]
pub struct UiColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl UiColor {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const WHITE: Self = Self::rgba(255, 255, 255, 255);
    pub const BLACK: Self = Self::rgba(0, 0, 0, 255);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn with_alpha(self, opacity: f32) -> Self {
        Self {
            a: ((self.a as f32) * opacity.clamp(0.0, 1.0)).round() as u8,
            ..self
        }
    }

    pub fn is_visible(self) -> bool {
        self.a > 0
    }
}

/// Length unit used by layout.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "53a35f1e-66be-446c-a1f4-84b7fe3b4b51"]
#[repr(C)]
pub enum UiLength {
    Auto,
    Px(f32),
    Percent(f32),
    Fill,
}

impl Default for UiLength {
    fn default() -> Self {
        Self::Auto
    }
}

/// Per-corner radius for rounded backgrounds and borders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "74660455-08dd-410b-927f-42fd97f218c8"]
#[reflect(Default)]
#[repr(C)]
pub struct CornerRadius {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadius {
    pub const fn all(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }

    pub const fn top(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: 0.0,
            bottom_left: 0.0,
        }
    }

    pub const fn left(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: 0.0,
            bottom_right: 0.0,
            bottom_left: value,
        }
    }

    pub const fn none() -> Self {
        Self::all(0.0)
    }

    pub fn is_rounded(self) -> bool {
        self.top_left > 0.0
            || self.top_right > 0.0
            || self.bottom_right > 0.0
            || self.bottom_left > 0.0
    }
}

/// Border style for UI rectangles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "38d31f12-096e-41bf-a0ed-90fd6d5c454e"]
#[reflect(Default)]
#[repr(C)]
pub struct Border {
    pub color: UiColor,
    pub width: f32,
}

impl Border {
    pub const fn none() -> Self {
        Self {
            color: UiColor::TRANSPARENT,
            width: 0.0,
        }
    }

    pub const fn solid(color: UiColor, width: f32) -> Self {
        Self { color, width }
    }

    pub fn is_visible(self) -> bool {
        self.width > 0.0 && self.color.is_visible()
    }
}

/// Whether a node participates in pointer hit testing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "c1acddbe-c29c-49bb-b1fb-a7d88b42ac7a"]
#[repr(C)]
pub enum PointerEvents {
    #[default]
    Auto,
    None,
}

/// Alignment on a cross axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "b321416d-aacd-4fe2-ae60-632b92e3bfca"]
#[repr(C)]
pub enum AlignItems {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

/// Alignment on a main axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "21f68979-c70b-4cb0-beaf-f2e9f6df9a33"]
#[repr(C)]
pub enum JustifyContent {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

/// Responsive screen class for layout/style overrides.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TypeUuid, Reflect,
)]
#[uuid = "e845c9eb-2eec-4f01-bc76-24ff48819b10"]
#[repr(C)]
pub enum ScreenClass {
    #[default]
    Compact,
    Medium,
    Wide,
}

impl ScreenClass {
    pub fn from_width(width: f32) -> Self {
        if width >= 1024.0 {
            Self::Wide
        } else if width >= 640.0 {
            Self::Medium
        } else {
            Self::Compact
        }
    }
}

/// Fully resolved UI style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "b9af0caa-5d30-4d6c-9240-68e4b1f4d9d0"]
#[reflect(Default)]
#[repr(C)]
pub struct Style {
    pub width: UiLength,
    pub height: UiLength,
    pub min_width: UiLength,
    pub min_height: UiLength,
    pub max_width: UiLength,
    pub max_height: UiLength,
    pub margin: EdgeInsets,
    pub padding: EdgeInsets,
    pub gap: f32,
    pub background: UiColor,
    pub text_color: UiColor,
    pub border: Border,
    pub radius: CornerRadius,
    pub opacity: f32,
    pub font_size: f32,
    pub pointer_events: PointerEvents,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: UiLength,
    pub align_items: AlignItems,
    pub justify_content: JustifyContent,
    pub clip: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            width: UiLength::Auto,
            height: UiLength::Auto,
            min_width: UiLength::Auto,
            min_height: UiLength::Auto,
            max_width: UiLength::Auto,
            max_height: UiLength::Auto,
            margin: EdgeInsets::ZERO,
            padding: EdgeInsets::ZERO,
            gap: 0.0,
            background: UiColor::TRANSPARENT,
            text_color: UiColor::WHITE,
            border: Border::none(),
            radius: CornerRadius::none(),
            opacity: 1.0,
            font_size: 16.0,
            pointer_events: PointerEvents::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: UiLength::Auto,
            align_items: AlignItems::Stretch,
            justify_content: JustifyContent::Start,
            clip: false,
        }
    }
}

/// Sparse style override used for inline styles, state styles, and responsive overrides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StylePatch {
    pub width: Option<UiLength>,
    pub height: Option<UiLength>,
    pub min_width: Option<UiLength>,
    pub min_height: Option<UiLength>,
    pub max_width: Option<UiLength>,
    pub max_height: Option<UiLength>,
    pub margin: Option<EdgeInsets>,
    pub padding: Option<EdgeInsets>,
    pub gap: Option<f32>,
    pub background: Option<UiColor>,
    pub text_color: Option<UiColor>,
    pub border: Option<Border>,
    pub radius: Option<CornerRadius>,
    pub opacity: Option<f32>,
    pub font_size: Option<f32>,
    pub pointer_events: Option<PointerEvents>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub flex_basis: Option<UiLength>,
    pub align_items: Option<AlignItems>,
    pub justify_content: Option<JustifyContent>,
    pub clip: Option<bool>,
}

impl StylePatch {
    pub fn merge_from(&mut self, other: StylePatch) {
        if other.width.is_some() {
            self.width = other.width;
        }
        if other.height.is_some() {
            self.height = other.height;
        }
        if other.min_width.is_some() {
            self.min_width = other.min_width;
        }
        if other.min_height.is_some() {
            self.min_height = other.min_height;
        }
        if other.max_width.is_some() {
            self.max_width = other.max_width;
        }
        if other.max_height.is_some() {
            self.max_height = other.max_height;
        }
        if other.margin.is_some() {
            self.margin = other.margin;
        }
        if other.padding.is_some() {
            self.padding = other.padding;
        }
        if other.gap.is_some() {
            self.gap = other.gap;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.text_color.is_some() {
            self.text_color = other.text_color;
        }
        if other.border.is_some() {
            self.border = other.border;
        }
        if other.radius.is_some() {
            self.radius = other.radius;
        }
        if other.opacity.is_some() {
            self.opacity = other.opacity;
        }
        if other.font_size.is_some() {
            self.font_size = other.font_size;
        }
        if other.pointer_events.is_some() {
            self.pointer_events = other.pointer_events;
        }
        if other.flex_grow.is_some() {
            self.flex_grow = other.flex_grow;
        }
        if other.flex_shrink.is_some() {
            self.flex_shrink = other.flex_shrink;
        }
        if other.flex_basis.is_some() {
            self.flex_basis = other.flex_basis;
        }
        if other.align_items.is_some() {
            self.align_items = other.align_items;
        }
        if other.justify_content.is_some() {
            self.justify_content = other.justify_content;
        }
        if other.clip.is_some() {
            self.clip = other.clip;
        }
    }

    pub fn apply_to(&self, style: &mut Style) {
        if let Some(value) = self.width {
            style.width = value;
        }
        if let Some(value) = self.height {
            style.height = value;
        }
        if let Some(value) = self.min_width {
            style.min_width = value;
        }
        if let Some(value) = self.min_height {
            style.min_height = value;
        }
        if let Some(value) = self.max_width {
            style.max_width = value;
        }
        if let Some(value) = self.max_height {
            style.max_height = value;
        }
        if let Some(value) = self.margin {
            style.margin = value;
        }
        if let Some(value) = self.padding {
            style.padding = value;
        }
        if let Some(value) = self.gap {
            style.gap = value;
        }
        if let Some(value) = self.background {
            style.background = value;
        }
        if let Some(value) = self.text_color {
            style.text_color = value;
        }
        if let Some(value) = self.border {
            style.border = value;
        }
        if let Some(value) = self.radius {
            style.radius = value;
        }
        if let Some(value) = self.opacity {
            style.opacity = value;
        }
        if let Some(value) = self.font_size {
            style.font_size = value;
        }
        if let Some(value) = self.pointer_events {
            style.pointer_events = value;
        }
        if let Some(value) = self.flex_grow {
            style.flex_grow = value;
        }
        if let Some(value) = self.flex_shrink {
            style.flex_shrink = value;
        }
        if let Some(value) = self.flex_basis {
            style.flex_basis = value;
        }
        if let Some(value) = self.align_items {
            style.align_items = value;
        }
        if let Some(value) = self.justify_content {
            style.justify_content = value;
        }
        if let Some(value) = self.clip {
            style.clip = value;
        }
    }

    pub fn width(mut self, value: UiLength) -> Self {
        self.width = Some(value);
        self
    }
    pub fn height(mut self, value: UiLength) -> Self {
        self.height = Some(value);
        self
    }
    pub fn min_width(mut self, value: UiLength) -> Self {
        self.min_width = Some(value);
        self
    }
    pub fn min_height(mut self, value: UiLength) -> Self {
        self.min_height = Some(value);
        self
    }
    pub fn margin(mut self, value: EdgeInsets) -> Self {
        self.margin = Some(value);
        self
    }
    pub fn padding(mut self, value: EdgeInsets) -> Self {
        self.padding = Some(value);
        self
    }
    pub fn gap(mut self, value: f32) -> Self {
        self.gap = Some(value);
        self
    }
    pub fn background(mut self, value: UiColor) -> Self {
        self.background = Some(value);
        self
    }
    pub fn text_color(mut self, value: UiColor) -> Self {
        self.text_color = Some(value);
        self
    }
    pub fn border(mut self, value: Border) -> Self {
        self.border = Some(value);
        self
    }
    pub fn radius(mut self, value: CornerRadius) -> Self {
        self.radius = Some(value);
        self
    }
    pub fn font_size(mut self, value: f32) -> Self {
        self.font_size = Some(value);
        self
    }
    pub fn pointer_events(mut self, value: PointerEvents) -> Self {
        self.pointer_events = Some(value);
        self
    }
    pub fn flex_grow(mut self, value: f32) -> Self {
        self.flex_grow = Some(value);
        self
    }
    pub fn flex_basis(mut self, value: UiLength) -> Self {
        self.flex_basis = Some(value);
        self
    }
    pub fn align_items(mut self, value: AlignItems) -> Self {
        self.align_items = Some(value);
        self
    }
    pub fn justify_content(mut self, value: JustifyContent) -> Self {
        self.justify_content = Some(value);
        self
    }
    pub fn clip(mut self, value: bool) -> Self {
        self.clip = Some(value);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "e76be5f4-13a4-4cb6-a164-ac3e801eaa41"]
#[reflect(Default)]
#[repr(C)]
pub struct ColorTokens {
    pub surface: UiColor,
    pub surface_translucent: UiColor,
    pub outline: UiColor,
    pub text: UiColor,
    pub accent: UiColor,
    pub hover: UiColor,
    pub pressed: UiColor,
}

impl Default for ColorTokens {
    fn default() -> Self {
        Self {
            surface: UiColor::rgba(18, 22, 24, 255),
            surface_translucent: UiColor::rgba(18, 22, 24, 210),
            outline: UiColor::rgba(105, 124, 132, 255),
            text: UiColor::rgba(232, 238, 241, 255),
            accent: UiColor::rgba(88, 166, 255, 255),
            hover: UiColor::rgba(46, 56, 61, 255),
            pressed: UiColor::rgba(61, 74, 80, 255),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "8c9a32f8-3040-4ddc-8fa2-2aaefbc68d4f"]
#[reflect(Default)]
#[repr(C)]
pub struct SpaceTokens {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

impl Default for SpaceTokens {
    fn default() -> Self {
        Self {
            xs: 2.0,
            sm: 4.0,
            md: 8.0,
            lg: 16.0,
            xl: 24.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "4516a5f5-c042-4763-9fdd-3058d25787f9"]
#[reflect(Default)]
#[repr(C)]
pub struct FontTokens {
    pub body: f32,
    pub label: f32,
    pub title: f32,
}

impl Default for FontTokens {
    fn default() -> Self {
        Self {
            body: 16.0,
            label: 13.0,
            title: 22.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "26862421-b65e-466f-aeeb-82d929b9d58f"]
#[reflect(Default)]
#[repr(C)]
pub struct RadiusTokens {
    pub none: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

impl Default for RadiusTokens {
    fn default() -> Self {
        Self {
            none: 0.0,
            sm: 4.0,
            md: 8.0,
            lg: 12.0,
        }
    }
}

/// Reflectable theme tokens. Named style presets are intentionally kept in a side registry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "a179de6e-7818-4ecf-9214-159d9d4c6038"]
#[reflect(Default)]
#[repr(C)]
pub struct Theme {
    pub colors: ColorTokens,
    pub spacing: SpaceTokens,
    pub fonts: FontTokens,
    pub radius: RadiusTokens,
    pub default_style: Style,
}

impl Default for Theme {
    fn default() -> Self {
        let colors = ColorTokens::default();
        let fonts = FontTokens::default();
        Self {
            colors: colors.clone(),
            spacing: Default::default(),
            fonts,
            radius: Default::default(),
            default_style: Style {
                text_color: colors.text,
                font_size: fonts.body,
                ..Default::default()
            },
        }
    }
}

/// Named style presets, kept outside reflected theme data so the MVP avoids a CSS selector engine.
#[derive(Clone, Debug, Default)]
pub struct StyleRegistry {
    presets: HashMap<String, StylePatch>,
}

impl StyleRegistry {
    pub fn insert(&mut self, name: impl Into<String>, style: StylePatch) {
        self.presets.insert(name.into(), style);
    }

    pub fn get(&self, name: &str) -> Option<&StylePatch> {
        self.presets.get(name)
    }
}
