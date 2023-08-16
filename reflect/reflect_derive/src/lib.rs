use proc_macro::TokenStream;
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
            .field::<#ty>(
                stringify!(#ident),
                Box::new(|x| {
                    match x.downcast_ref::<#name>() {
                        Some(value) => Some(&value.#ident),
                        None => None
                    }
                }),
                Box::new(|x, v| {
                    match x.downcast_mut::<#name>() {
                        Some(value) => match v.downcast::<#ty>() {
                            Ok(rv) => {
                                value.#ident = *rv;
                                Some(())
                            },
                            _ => None
                        },
                        _ => None
                    }
                })
            )
        }
    });


    let output = quote! {
        impl #name {
            pub fn register(registry: &mut reflect::registry::TypeRegistry) {
                registry.new_struct::<#name>()
                    #(#add_field_calls)*;
            }
        }

        inventory::submit!(reflect::registry::TypeRegistrationFn {
            register: #name::register
        });
    };

    output.into()
}
