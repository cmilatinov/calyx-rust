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

    pub fn lerp(self, other: Self, amount: f32) -> Self {
        Self {
            r: lerp_u8(self.r, other.r, amount),
            g: lerp_u8(self.g, other.g, amount),
            b: lerp_u8(self.b, other.b, amount),
            a: lerp_u8(self.a, other.a, amount),
        }
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

    pub fn lerp(self, other: Self, amount: f32) -> Self {
        Self {
            top_left: lerp_f32(self.top_left, other.top_left, amount),
            top_right: lerp_f32(self.top_right, other.top_right, amount),
            bottom_right: lerp_f32(self.bottom_right, other.bottom_right, amount),
            bottom_left: lerp_f32(self.bottom_left, other.bottom_left, amount),
        }
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

    pub fn lerp(self, other: Self, amount: f32) -> Self {
        Self {
            color: self.color.lerp(other.color, amount),
            width: lerp_f32(self.width, other.width, amount),
        }
    }
}

/// Screen-space fake 3D transform applied by backends that support transformed quads.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "67850a50-61f7-4eba-92f7-b9cb3f672f0f"]
#[reflect(Default)]
#[repr(C)]
pub struct UiTransform {
    pub rotate_x: f32,
    pub rotate_y: f32,
    pub rotate_z: f32,
    pub scale: f32,
    pub perspective: f32,
}

impl Default for UiTransform {
    fn default() -> Self {
        Self {
            rotate_x: 0.0,
            rotate_y: 0.0,
            rotate_z: 0.0,
            scale: 1.0,
            perspective: 800.0,
        }
    }
}

impl UiTransform {
    pub fn identity() -> Self {
        Self::default()
    }

    pub fn tilt_degrees(rotate_x: f32, rotate_y: f32) -> Self {
        Self {
            rotate_x: rotate_x.to_radians(),
            rotate_y: rotate_y.to_radians(),
            ..Default::default()
        }
    }

    pub fn is_identity(self) -> bool {
        self.rotate_x.abs() < f32::EPSILON
            && self.rotate_y.abs() < f32::EPSILON
            && self.rotate_z.abs() < f32::EPSILON
            && (self.scale - 1.0).abs() < f32::EPSILON
    }

    pub fn lerp(self, other: Self, amount: f32) -> Self {
        Self {
            rotate_x: lerp_f32(self.rotate_x, other.rotate_x, amount),
            rotate_y: lerp_f32(self.rotate_y, other.rotate_y, amount),
            rotate_z: lerp_f32(self.rotate_z, other.rotate_z, amount),
            scale: lerp_f32(self.scale, other.scale, amount),
            perspective: lerp_f32(self.perspective, other.perspective, amount),
        }
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
    pub transform: UiTransform,
    pub transition_duration: f32,
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
            transform: UiTransform::identity(),
            transition_duration: 0.0,
        }
    }
}

impl Style {
    pub fn lerp(from: &Self, to: &Self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        Self {
            width: pick_end(from.width, to.width, amount),
            height: pick_end(from.height, to.height, amount),
            min_width: pick_end(from.min_width, to.min_width, amount),
            min_height: pick_end(from.min_height, to.min_height, amount),
            max_width: pick_end(from.max_width, to.max_width, amount),
            max_height: pick_end(from.max_height, to.max_height, amount),
            margin: lerp_edge_insets(from.margin, to.margin, amount),
            padding: lerp_edge_insets(from.padding, to.padding, amount),
            gap: lerp_f32(from.gap, to.gap, amount),
            background: from.background.lerp(to.background, amount),
            text_color: from.text_color.lerp(to.text_color, amount),
            border: from.border.lerp(to.border, amount),
            radius: from.radius.lerp(to.radius, amount),
            opacity: lerp_f32(from.opacity, to.opacity, amount),
            font_size: lerp_f32(from.font_size, to.font_size, amount),
            pointer_events: pick_end(from.pointer_events, to.pointer_events, amount),
            flex_grow: lerp_f32(from.flex_grow, to.flex_grow, amount),
            flex_shrink: lerp_f32(from.flex_shrink, to.flex_shrink, amount),
            flex_basis: pick_end(from.flex_basis, to.flex_basis, amount),
            align_items: pick_end(from.align_items, to.align_items, amount),
            justify_content: pick_end(from.justify_content, to.justify_content, amount),
            clip: pick_end(from.clip, to.clip, amount),
            transform: from.transform.lerp(to.transform, amount),
            transition_duration: lerp_f32(from.transition_duration, to.transition_duration, amount),
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
    pub transform: Option<UiTransform>,
    pub transition_duration: Option<f32>,
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
        if other.transform.is_some() {
            self.transform = other.transform;
        }
        if other.transition_duration.is_some() {
            self.transition_duration = other.transition_duration;
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
        if let Some(value) = self.transform {
            style.transform = value;
        }
        if let Some(value) = self.transition_duration {
            style.transition_duration = value.max(0.0);
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
    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.flex_shrink = Some(value);
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
    pub fn transform(mut self, value: UiTransform) -> Self {
        self.transform = Some(value);
        self
    }
    pub fn transition_duration(mut self, value: f32) -> Self {
        self.transition_duration = Some(value);
        self
    }
}

fn lerp_f32(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn lerp_u8(from: u8, to: u8, amount: f32) -> u8 {
    lerp_f32(from as f32, to as f32, amount)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn lerp_edge_insets(from: EdgeInsets, to: EdgeInsets, amount: f32) -> EdgeInsets {
    EdgeInsets {
        top: lerp_f32(from.top, to.top, amount),
        right: lerp_f32(from.right, to.right, amount),
        bottom: lerp_f32(from.bottom, to.bottom, amount),
        left: lerp_f32(from.left, to.left, amount),
    }
}

fn pick_end<T: Copy>(from: T, to: T, amount: f32) -> T {
    if amount >= 1.0 {
        to
    } else {
        from
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
