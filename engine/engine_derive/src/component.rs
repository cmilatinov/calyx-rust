use crate::fq::{FQBox, FQOption, FQReflect};
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let expanded = quote! {
        #[automatically_derived]
        impl engine::component::ComponentInstance for #name {
            fn component_type_id(&self) -> legion::storage::ComponentTypeId {
                legion::storage::ComponentTypeId::of::<#name>()
            }
            fn get_instance<'a>(
                &self, entry: &'a legion::world::EntryRef
            ) -> #FQOption<&'a dyn engine::component::Component> {
                let instance = entry.get_component::<#name>().ok()?;
                Some(instance)
            }
            fn get_instance_mut<'a>(
                &self, entry: &'a mut legion::world::Entry
            ) -> #FQOption<&'a mut dyn engine::component::Component> {
                let instance = entry.get_component_mut::<#name>().ok()?;
                Some(instance)
            }
            fn take_instance(
                &self, entry: &mut legion::world::Entry
            ) -> #FQOption<#FQBox<dyn engine::component::Component>> {
                let value = entry.get_component::<#name>().ok()
                    .and_then(|c| serde_json::to_value(c).ok())?;
                entry.remove_component::<#name>();
                let instance: #name = serde_json::from_value(value).ok()?;
                Some(#FQBox::new(instance))
            }
            fn put_back_instance(
                &self,
                entry: &mut legion::world::Entry,
                instance: #FQBox<dyn engine::component::Component>
            ) {
                // Round-trip through serialization to recover the concrete type.
                // The derive macro knows the type is #name but we only have dyn Component.
                if let Some(value) = instance.serialize() {
                    if let Ok(concrete) = serde_json::from_value::<#name>(value) {
                        entry.add_component(concrete);
                    }
                }
            }
            fn bind_instance(
                &self,
                entry: &mut legion::world::Entry,
                instance: #FQBox<dyn #FQReflect>
            ) -> bool {
                if let Ok(instance) = instance.downcast::<#name>() {
                    entry.add_component(*instance);
                    true
                } else {
                    false
                }
            }
            fn remove_instance(
                &self, entry: &mut legion::world::Entry
            ) {
                entry.remove_component::<#name>();
            }
            fn serialize(&self) -> #FQOption<serde_json::Value> {
                serde_json::to_value(self).ok()
            }
            fn deserialize(&self, value: serde_json::Value) -> #FQOption<#FQBox<dyn #FQReflect>> {
                serde_json::from_value::<#name>(value).ok().map(|v| {
                    let value: Box<dyn Reflect> = Box::new(v);
                    value
                })
            }
        }
    };
    TokenStream::from(expanded)
}
