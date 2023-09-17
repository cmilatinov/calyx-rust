extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};
use sha1::Digest;

#[proc_macro_derive(Component)]
pub fn component_derive(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let mut hasher = sha1::Sha1::new();
    hasher.update(name.to_string().as_bytes());
    let hash = hasher.finalize();
    let uuid_bytes = [
        hash[0], hash[1], hash[2], hash[3],
        hash[4], hash[5], hash[6], hash[7],
        hash[8], hash[9], hash[10], hash[11],
        hash[12], hash[13], hash[14], hash[15]
    ];
    let type_uuid = uuid::Uuid::from_bytes(uuid_bytes).to_string();
   
    let attributes: syn::Attribute = syn::parse_quote!(#[derive(Default)]);
    input.attrs.push(attributes);

    let expanded = quote! {
        impl engine::component::TypeUUID for #name {
            fn type_uuid(&self) -> uuid::Uuid {
                uuid::Uuid::parse_str(#type_uuid).unwrap()
            }
        }

        impl engine::component::ComponentInstance for #name {
            fn component_type_id(&self) -> legion::storage::ComponentTypeId {
                legion::storage::ComponentTypeId::of::<#name>()
            }
            fn get_instance<'a>(
                &self, entry: &'a legion::world::EntryRef
            ) -> std::option::Option<&'a dyn engine::component::Component> {
                let instance = entry.get_component::<#name>().ok()?;
                Some(instance)
            }
            fn get_instance_mut<'a>(
                &self, entry: &'a mut legion::world::Entry
            ) -> std::option::Option<&'a mut dyn engine::component::Component> {
                let instance = entry.get_component_mut::<#name>().ok()?;
                Some(instance)
            }
        }
    };

    TokenStream::from(expanded)
}

