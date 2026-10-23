use crate::fq::{FQBox, FQOption, FQReflect, FQResult};
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
                #FQOption::Some(instance)
            }
            fn get_instance_mut<'a>(
                &self, entry: &'a mut legion::world::Entry
            ) -> #FQOption<&'a mut dyn engine::component::Component> {
                let instance = entry.get_component_mut::<#name>().ok()?;
                #FQOption::Some(instance)
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
            fn replace_instance(
                &self,
                entry: &mut legion::world::Entry,
                value: #FQBox<dyn #FQReflect>
            ) -> #FQResult<#FQBox<dyn engine::component::Component>, #FQBox<dyn #FQReflect>> {
                let value = value.downcast::<#name>()?;
                match entry.get_component_mut::<#name>() {
                    #FQResult::Ok(instance) => {
                        #FQResult::Ok(#FQBox::new(::std::mem::replace(instance, *value)))
                    }
                    #FQResult::Err(_) => #FQResult::Err(value),
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
            fn deserialize(
                &self, value: &serde_json::Value
            ) -> #FQResult<#FQBox<dyn #FQReflect>, serde_json::Error> {
                let value: #name = serde::Deserialize::deserialize(value)?;
                let value: #FQBox<dyn #FQReflect> = #FQBox::new(value);
                #FQResult::Ok(value)
            }
        }
    };
    TokenStream::from(expanded)
}
