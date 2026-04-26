use proc_macro::TokenStream;

use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{parenthesized, parse_macro_input, Path, Token, Type};

use crate::reflect_impl::{
    reflected_type_impl, register_trait_meta_impls, type_name_and_reflect_impls,
};

struct ReflectValueDef {
    type_name: Type,
    traits: Punctuated<Path, Comma>,
}

impl Parse for ReflectValueDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let type_name: Type = input.parse()?;
        let content;
        parenthesized!(content in input);
        let traits = content.parse_terminated(Path::parse, Token![,])?;
        Ok(Self { type_name, traits })
    }
}

pub(crate) fn impl_reflect_value(input: TokenStream) -> TokenStream {
    let def = parse_macro_input!(input as ReflectValueDef);
    let name = &def.type_name;
    let reflect_impls = type_name_and_reflect_impls(name);
    let register_traits_impl =
        register_trait_meta_impls(name, def.traits.into_iter().collect::<Vec<_>>());
    let reflected_type_impl = reflected_type_impl(
        name,
        quote! {
            registry.meta::<#name>();
            #register_traits_impl
        },
        quote! {
            inventory::submit!(engine::reflect::type_registry::TypeRegistrationFn(<#name as engine::reflect::ReflectedType>::register));
        },
    );

    TokenStream::from(quote! {
        #reflect_impls
        #reflected_type_impl
    })
}
