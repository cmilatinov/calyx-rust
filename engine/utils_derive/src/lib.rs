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
        impl #name {
            pub fn type_uuid() -> uuid::Uuid {
                uuid::Uuid::parse_str(#type_uuid).unwrap()
            }
        }

        impl specs::Component for #name {
            type Storage = specs::VecStorage<Self>;
        }
    };

    TokenStream::from(expanded)
}

