mod component;
mod deserialize_context;
mod fq;
mod reflect_trait;
mod reflect_type;
mod reflect_value;
mod type_uuid;

extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro_derive(Component)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    component::derive_component(input)
}

#[proc_macro_derive(TypeUuid, attributes(uuid))]
pub fn derive_type_uuid(input: TokenStream) -> TokenStream {
    type_uuid::derive_type_uuid(input)
}

#[proc_macro]
pub fn impl_extern_type_uuid(input: TokenStream) -> TokenStream {
    type_uuid::extern_type_uuid(input)
}

#[proc_macro_derive(Reflect, attributes(reflect, reflect_attr, reflect_skip))]
pub fn derive_reflect(input: TokenStream) -> TokenStream {
    reflect_type::derive_reflect(input)
}

#[proc_macro_attribute]
pub fn reflect_trait(args: TokenStream, input: TokenStream) -> TokenStream {
    reflect_trait::reflect_trait(args, input)
}

#[proc_macro]
pub fn impl_reflect_value(input: TokenStream) -> TokenStream {
    reflect_value::impl_reflect_value(input)
}

#[proc_macro_derive(
    DeserializeWithContext,
    attributes(context, use_context, skip_deserialize)
)]
pub fn derive_deserialize(input: TokenStream) -> TokenStream {
    deserialize_context::derive_deserialize_with_context(input)
}
