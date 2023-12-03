extern crate proc_macro;

use proc_macro::TokenStream;

use quote::quote;
use sha1::Digest;
use syn::{parse_macro_input, DeriveInput, Expr, ExprLit, Lit, LitInt, Meta, MetaNameValue};
use uuid::Uuid;

fn uuid_from_str(value: &str) -> Uuid {
    let mut hasher = sha1::Sha1::new();
    hasher.update(value.as_bytes());
    let hash = hasher.finalize();
    let mut bytes: uuid::Bytes = [0; 16];
    bytes.copy_from_slice(&hash.as_slice()[0..16]);
    Uuid::from_bytes(bytes)
}

#[proc_macro_derive(TypeUuid, attributes(uuid))]
pub fn derive_type_uuid(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let mut uuid = None;
    for attr in input.attrs.iter() {
        if attr.path().is_ident("uuid") {
            if let Meta::NameValue(MetaNameValue {
                value:
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(lit), ..
                    }),
                ..
            }) = &attr.meta
            {
                uuid = Uuid::parse_str(lit.value().as_str()).ok();
            }
        }
    }
    let uuid = uuid.unwrap_or_else(|| uuid_from_str(name.to_string().as_str()));
    let bytes = uuid
        .as_bytes()
        .iter()
        .map(|byte| format!("{:#X}", byte))
        .map(|byte_str| syn::parse_str::<LitInt>(&byte_str).unwrap());
    (quote! {
        impl engine::utils::TypeUuid for #name {
            const UUID: &'static [u8; 16] = &[
                #( #bytes ),*
            ];
        }
    })
    .into()
}

#[proc_macro_derive(Component)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let expanded = quote! {
        impl engine::component::ComponentInstance for #name {
            fn component_type_id(&self) -> engine::legion::storage::ComponentTypeId {
                engine::legion::storage::ComponentTypeId::of::<#name>()
            }
            fn get_instance<'a>(
                &self, entry: &'a engine::legion::world::EntryRef
            ) -> std::option::Option<&'a dyn engine::component::Component> {
                let instance = entry.get_component::<#name>().ok()?;
                Some(instance)
            }
            fn get_instance_mut<'a>(
                &self, entry: &'a mut engine::legion::world::Entry
            ) -> std::option::Option<&'a mut dyn engine::component::Component> {
                let instance = entry.get_component_mut::<#name>().ok()?;
                Some(instance)
            }
            fn bind_instance(
                &self,
                entry: &mut engine::legion::world::Entry,
                instance: std::boxed::Box<dyn reflect::Reflect>
            ) {
                if let Ok(instance) = instance.downcast::<#name>() {
                    entry.add_component(*instance);
                }
            }
            fn remove_instance(
                &self, entry: &mut engine::legion::world::Entry
            ) {
                entry.remove_component::<#name>();
            }
        }
    };
    TokenStream::from(expanded)
}
