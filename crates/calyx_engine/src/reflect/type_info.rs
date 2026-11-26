use crate::reflect::Reflect;
use indexmap::IndexMap;
use std::any::TypeId;
use std::collections::HashMap;
use uuid::Uuid;

/// Getter used to read a reflected field by shared reference.
pub type FieldGetter = fn(&dyn Reflect) -> Option<&dyn Reflect>;
/// Getter used to read a reflected field by mutable reference.
pub type FieldGetterMut = fn(&mut dyn Reflect) -> Option<&mut dyn Reflect>;
/// Setter used to assign a reflected field.
pub type FieldSetter = fn(&mut dyn Reflect, Box<dyn Reflect>) -> Option<()>;

/// Supported attribute payloads stored on reflected types and fields.
#[derive(Copy, Clone)]
pub enum AttributeValue {
    /// Marker attribute with no payload.
    None,
    /// Floating-point payload.
    Float(f64),
    /// Integer payload.
    Integer(isize),
    /// String payload.
    String(&'static str),
}
/// Attribute map keyed by attribute name.
pub type AttributeMap = HashMap<&'static str, AttributeValue>;

/// Allows you to introspect structs (and potentially other types in the future)
/// at runtime by listing, querying and setting their fields
/// Runtime metadata for a reflected type.
pub enum TypeInfo {
    /// Struct-like type metadata.
    Struct(StructInfo),
    /// Enum metadata.
    Enum(EnumInfo),
    /// List metadata.
    List(ListInfo),
    /// Option metadata.
    Option(OptionInfo),
    /// Map metadata.
    Map(MapInfo),
    /// Placeholder for registered types without detailed metadata.
    None,
}

/// Minimal type descriptor embedded inside higher-level metadata.
pub struct TypeDescriptor {
    /// Rust `TypeId` for the described type.
    pub type_id: TypeId,
    /// Stable UUID for the described type.
    pub type_uuid: Uuid,
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
}

/// Reflection metadata for a struct-like type.
pub struct StructInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Attributes attached to the type.
    pub attrs: AttributeMap,
    /// Named fields keyed by their Rust field name, in declaration order.
    pub fields: IndexMap<&'static str, NamedField>,
}

impl StructInfo {
    /// Looks up a field by name.
    pub fn field(&self, name: &str) -> Option<&NamedField> {
        self.fields.get(name)
    }

    /// Looks up a type attribute by name.
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
}

/// Reflection metadata for an enum type.
pub struct EnumInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Attributes attached to the enum.
    pub attrs: AttributeMap,
    /// Enum variants in declaration order.
    pub variants: Vec<EnumVariantInfo>,
}

impl EnumInfo {
    /// Looks up a variant by name.
    pub fn variant(&self, name: &str) -> Option<&EnumVariantInfo> {
        self.variants.iter().find(|variant| variant.name == name)
    }

    /// Looks up an enum attribute by name.
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }

    /// Whether every variant is a unit variant, so a value can be set to
    /// any of them by name.
    pub fn is_unit_only(&self) -> bool {
        self.variants.iter().all(EnumVariantInfo::is_unit)
    }
}

/// Reflection metadata for one enum variant.
pub struct EnumVariantInfo {
    /// Variant name.
    pub name: &'static str,
    /// Attributes attached to the variant, such as its display `name`.
    pub attrs: AttributeMap,
    /// Variant fields in declaration order.
    pub fields: Vec<EnumVariantFieldInfo>,
}

impl EnumVariantInfo {
    /// Looks up a variant attribute by name.
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }

    /// Whether the variant carries no data.
    pub fn is_unit(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Reflection metadata for one enum variant field.
pub struct EnumVariantFieldInfo {
    /// Optional field name for named variants.
    pub name: Option<&'static str>,
    /// Rust `TypeId` of the field.
    pub type_id: TypeId,
    /// Stable UUID of the field type.
    pub type_uuid: Uuid,
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
}

/// Reflection metadata for a list-like container.
pub struct ListInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Descriptor for the element type.
    pub element: TypeDescriptor,
}

/// Reflection metadata for an `Option<T>`-like type.
pub struct OptionInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Descriptor for the wrapped value type.
    pub value: TypeDescriptor,
}

/// Reflection metadata for a map-like container.
pub struct MapInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Descriptor for the key type.
    pub key: TypeDescriptor,
    /// Descriptor for the value type.
    pub value: TypeDescriptor,
}

/// Reflection metadata for one named struct field.
pub struct NamedField {
    /// Rust field name.
    pub name: &'static str,
    /// Rust `TypeId` of the field.
    pub type_id: TypeId,
    /// Stable UUID of the field type.
    pub type_uuid: Uuid,
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Field attributes keyed by attribute name.
    pub attrs: AttributeMap,
    /// Doc comment captured from the source field, when available.
    pub doc: Option<&'static str>,
    /// Shared field getter.
    pub getter: FieldGetter,
    /// Mutable field getter.
    pub getter_mut: FieldGetterMut,
    /// Field setter.
    pub setter: FieldSetter,
}

impl NamedField {
    /// Reads the field as a concrete `T`.
    pub fn get<'a, T: Reflect>(&'a self, instance: &'a dyn Reflect) -> Option<&'a T> {
        let value = (self.getter)(instance)?;
        value.downcast_ref::<T>()
    }
    /// Reads the field mutably as a concrete `T`.
    pub fn get_mut<'a, T: Reflect>(&'a self, instance: &'a mut dyn Reflect) -> Option<&'a mut T> {
        let value = (self.getter_mut)(instance)?;
        value.downcast_mut::<T>()
    }
    /// Reads the field as a reflected trait object.
    pub fn get_reflect<'a>(&'a self, instance: &'a dyn Reflect) -> Option<&'a dyn Reflect> {
        (self.getter)(instance)
    }
    /// Reads the field mutably as a reflected trait object.
    pub fn get_reflect_mut<'a>(
        &'a self,
        instance: &'a mut dyn Reflect,
    ) -> Option<&'a mut dyn Reflect> {
        (self.getter_mut)(instance)
    }
    /// Assigns a concrete reflected value to the field.
    pub fn set<T: Reflect + 'static>(&self, instance: &mut dyn Reflect, value: T) -> Option<()> {
        (self.setter)(instance, Box::new(value))
    }
    /// Looks up a field attribute by name.
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
    /// The inspector group named by the field's `group` attribute, if any.
    pub fn group(&self) -> Option<&'static str> {
        match self.attr("group") {
            Some(AttributeValue::String(group)) => Some(group),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
