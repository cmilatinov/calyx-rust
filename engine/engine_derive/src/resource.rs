use crate::fq::FQResource;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Path};

fn has_repr_c(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        if !attr.meta.path().is_ident("repr") {
            continue;
        }
        let Ok(path) = attr.parse_args::<Path>() else {
            continue;
        };
        if path.is_ident("C") {
            return true;
        }
    }
    false
}

pub fn derive_resource(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if !matches!(input.data, Data::Struct(_)) {
        panic!("Resource only works on structs");
    }
    if !has_repr_c(&input.attrs) {
        panic!("Resource requires #[repr(C)]");
    }
    let struct_name = &input.ident;
    TokenStream::from(quote! {
        impl #FQResource for #struct_name {}
    })
}
