use crate as engine;
use crate::reflect::ReflectDefault;
use crate::utils::TypeUuid;
use engine_derive::Reflect;
use serde::{Deserialize, Serialize};

/// 2D point in logical UI coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "0d4378a1-5f8e-4514-8f76-1f5ca7f692d4"]
#[reflect(Default)]
#[repr(C)]
pub struct UiPoint {
    pub x: f32,
    pub y: f32,
}

impl UiPoint {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 2D size in logical UI coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "f81f5f61-38c8-46fd-ad98-14a5ce067703"]
#[reflect(Default)]
#[repr(C)]
pub struct UiSize {
    pub width: f32,
    pub height: f32,
}

impl UiSize {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Axis-aligned rectangle in logical UI coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "f2da5c94-73d2-4055-9419-ac5e01bfb329"]
#[reflect(Default)]
#[repr(C)]
pub struct UiRect {
    pub min: UiPoint,
    pub max: UiPoint,
}

impl UiRect {
    pub const ZERO: Self = Self {
        min: UiPoint::ZERO,
        max: UiPoint::ZERO,
    };

    pub fn from_min_size(min: UiPoint, size: UiSize) -> Self {
        Self {
            min,
            max: UiPoint::new(min.x + size.width.max(0.0), min.y + size.height.max(0.0)),
        }
    }

    pub fn size(self) -> UiSize {
        UiSize::new(self.width(), self.height())
    }

    pub fn width(self) -> f32 {
        (self.max.x - self.min.x).max(0.0)
    }

    pub fn height(self) -> f32 {
        (self.max.y - self.min.y).max(0.0)
    }

    pub fn contains(self, point: UiPoint) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    pub fn inset(self, edge: EdgeInsets) -> Self {
        Self {
            min: UiPoint::new(self.min.x + edge.left, self.min.y + edge.top),
            max: UiPoint::new(
                (self.max.x - edge.right).max(self.min.x + edge.left),
                (self.max.y - edge.bottom).max(self.min.y + edge.top),
            ),
        }
    }
}

/// Four-sided edge spacing used for margin and padding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "99d8d3d8-83e8-4dbc-a0fa-4ba13e30f226"]
#[reflect(Default)]
#[repr(C)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl EdgeInsets {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    pub fn horizontal(self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}
