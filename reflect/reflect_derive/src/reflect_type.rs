use proc_macro2::{Ident, Span};
use proc_macro::{TokenStream};
use quote::quote;
use syn::{DeriveInput, Fields, Path, Token};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use crate::fq::{FQReflect, FQReflectedType};

struct ReflectAttribute {
    traits: Punctuated<Path, Token![,]>,
}

impl Parse for ReflectAttribute {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(ReflectAttribute {
            traits: Punctuated::parse_terminated(input)?,
        })
    }
}

pub(crate) fn derive_reflect(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    let name = &ast.ident;
    let attrs = &ast.attrs;

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
        },
        _ => {}
    }

    let add_field_calls = field_info.iter().map(|(ident, ty)| {
        quote! {
            .field::<#ty>(
                stringify!(#ident),
                |x| {
                    match x.downcast_ref::<#name>() {
                        Some(value) => Some(&value.#ident),
                        None => None
                    }
                },
                |x, v| {
                    if let Some(value) = x.downcast_mut::<#name>() {
                        if let Ok(rv) = v.downcast::<#ty>() {
                            value.#ident = *rv;
                            return Some(());
                        }
                    }
                    None
                }
            )
        }
    });

    let mut trait_paths = None;
    for attr in attrs {
        if attr.path().is_ident("reflect") {
            let reflect_args = attr.parse_args::<ReflectAttribute>().unwrap();
            trait_paths = Some(reflect_args.traits);
            break;
        }
    }

    let mut register_traits_impl = quote!{};
    if let Some(paths) = trait_paths {
        for trait_path in paths {
            let trait_ident = trait_path.segments.last().unwrap().ident.clone();
            let reflect_trait_ident = Ident::new(&format!("Reflect{}", trait_ident), Span::call_site());
            let register_trait_impl = quote!{
                registry.meta_impls::<#name, #reflect_trait_ident>();
            };
            register_traits_impl = quote!{
                #register_traits_impl
                #register_trait_impl
            };
        }
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
                registry.meta_struct::<#name>()
                    #(#add_field_calls)*;
                #register_traits_impl
            }
        }

        inventory::submit!(reflect::registry::TypeRegistrationFn(<#name as #FQReflectedType>::register));
    })
}