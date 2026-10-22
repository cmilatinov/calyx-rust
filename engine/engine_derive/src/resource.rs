use crate::fq::FQResource;
use crate::repr::require_repr_c;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput};

pub fn derive_resource(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    match &input.data {
        Data::Struct(_) => {}
        Data::Enum(data) => {
            return Err(syn::Error::new_spanned(
                data.enum_token,
                "Resource only works on structs",
            ))
        }
        Data::Union(data) => {
            return Err(syn::Error::new_spanned(
                data.union_token,
                "Resource only works on structs",
            ))
        }
    }
    require_repr_c(input, "Resource")?;
    let struct_name = &input.ident;
    Ok(quote! {
        impl #FQResource for #struct_name {}
    })
}
