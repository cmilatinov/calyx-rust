use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, parenthesized, Token, parse_macro_input, Path};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use crate::fq::{FQReflect, FQReflectedType};

struct ReflectValueDef {
    type_name: Ident,
    traits: Punctuated<Path, Comma>,
}

impl Parse for ReflectValueDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let type_name: Ident = input.parse()?;
        let content;
        parenthesized!(content in input);
        let traits = content.parse_terminated(Path::parse, Token![,])?;
        Ok(Self {
            type_name,
            traits,
        })
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
            fn as_any(&self) -> &dyn std::any::Any { self }
            #[inline]
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
        }

        impl #FQReflectedType for #name {
            fn register(registry: &mut reflect::registry::TypeRegistry) {
                registry.meta::<#name>();
                #register_traits_impl
            }
        }

        inventory::submit!(reflect::registry::TypeRegistrationFn(<#name as #FQReflectedType>::register));
    })
}