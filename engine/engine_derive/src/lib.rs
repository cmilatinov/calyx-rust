extern crate proc_macro;

use proc_macro::TokenStream;

use quote::quote;
use syn::{parse_macro_input, DeriveInput};

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