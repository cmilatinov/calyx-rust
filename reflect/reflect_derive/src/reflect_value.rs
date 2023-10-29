use proc_macro::TokenStream;

use proc_macro2::Span;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{parenthesized, parse_macro_input, Ident, Path, Token, Type};

use crate::fq::{FQAny, FQBox, FQReflect, FQReflectedType};

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
    let traits = &def.traits;

    let mut register_traits_impl = quote! {};
    for trait_path in traits {
        let trait_ident = trait_path.segments.last().unwrap().ident.clone();
        let reflect_trait_ident = Ident::new(&format!("Reflect{}", trait_ident), Span::call_site());
        let register_trait_impl = quote! {
            registry.meta_impls::<#name, #reflect_trait_ident>();
        };
        register_traits_impl = quote! {
            #register_traits_impl
            #register_trait_impl
        };
    }

    TokenStream::from(quote! {
        impl #FQReflect for #name {
            #[inline]
            fn type_name(&self) -> &'static str { std::any::type_name::<Self>() }
            #[inline]
            fn type_name_short(&self) -> &'static str { stringify!(#name) }
            #[inline]
            fn as_any(&self) -> &dyn #FQAny { self }
            #[inline]
            fn as_any_mut(&mut self) -> &mut dyn #FQAny { self }
            #[inline]
            fn as_reflect(&self) -> &dyn #FQReflect { self }
            #[inline]
            fn as_reflect_mut(&mut self) -> &mut dyn #FQReflect { self }
            #[inline]
            fn into_any(self: #FQBox<Self>) -> #FQBox<dyn #FQAny> { self }
        }

        impl #FQReflectedType for #name {
            fn register(registry: &mut reflect::type_registry::TypeRegistry) {
                registry.meta::<#name>();
                #register_traits_impl
            }
        }

        inventory::submit!(reflect::type_registry::TypeRegistrationFn(<#name as #FQReflectedType>::register));
    })
}
