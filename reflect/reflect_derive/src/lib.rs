use proc_macro::TokenStream;
use std::fmt::Debug;
use quote::quote;
use syn::{DeriveInput, Fields};

#[proc_macro_derive(Reflect)]
pub fn reflect(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    let name = &ast.ident;
    let fields = match &ast.data {
        syn::Data::Struct(s) => &s.fields,
        _ => panic!("Reflect only works on structs!"),
    };

    let mut field_info = Vec::new();
    match &fields {
        Fields::Named(named) => {
            for field in &named.named {
                if let Some(ident) = &field.ident {
                    let ty = &field.ty;
                    field_info.push((ident, ty));
                }
            }
        }
        _ => panic!("Reflect requires named fields!"),
    }

    let add_field_calls = field_info.iter().map(|(ident, ty)| {
        quote! {
            .add_field(stringify!(#ident), stringify!(#ty), std::any::TypeId::of::<#ty>())
        }
    });

    let output = quote! {
        impl #name {
            pub fn register(registry: &mut reflect::registry::TypeRegistry) {
                let info = reflect::registry::TypeInfoBuilder::new::<#name>(stringify!(#name))
                #(#add_field_calls)*
                .build();

                registry.register::<#name>(info);
            }
        }

        inventory::submit!(reflect::registry::TypeRegistrationFn {
            register: #name::register
        });
    };

    output.into()
}
