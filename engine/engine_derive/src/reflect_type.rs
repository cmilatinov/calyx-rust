use proc_macro::TokenStream;

use proc_macro2::{Ident, Span};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{DeriveInput, Expr, ExprLit, Fields, Lit, LitStr, Meta, MetaNameValue, Path, Token};

use crate::fq::{FQAny, FQAttributeValue, FQBox, FQReflect, FQReflectedType, FQTypeName};

#[derive(Debug)]
struct ReflectAttribute {
    name: Ident,
    value: Option<Lit>,
}

impl Parse for ReflectAttribute {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        let _equals: Option<Token![=]> = input.parse()?;
        let value = input.parse()?;
        Ok(ReflectAttribute { name, value })
    }
}

fn attribute_map(attrs: &[ReflectAttribute]) -> proc_macro2::TokenStream {
    let mut map = quote! {};
    for attr in attrs {
        let name = attr.name.to_string();
        let lit_name = LitStr::new(name.as_str(), Span::call_site());
        let value = match attr.value.as_ref() {
            Some(lit) => match lit {
                Lit::Str(str) => quote! { #FQAttributeValue::String(#str) },
                Lit::Float(float) => quote! { #FQAttributeValue::Float(#float) },
                Lit::Int(int) => quote! { #FQAttributeValue::Integer(#int) },
                _ => quote! { #FQAttributeValue::None },
            },
            None => quote! { #FQAttributeValue::None },
        };
        map = quote! { #map (#lit_name, #value), }
    }
    map = quote! {
        [#map].into()
    };
    map
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
    if let Fields::Named(named) = &fields {
        for field in &named.named {
            let doc = match field
                .attrs
                .iter()
                .filter_map(|attr| match attr.meta {
                    Meta::NameValue(MetaNameValue {
                        value:
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(ref value),
                                ..
                            }),
                        ..
                    }) if attr.path().is_ident("doc") => Some(value),
                    _ => None,
                })
                .next()
            {
                Some(value) => quote! { Some(#value) },
                None => quote! { None },
            };
            let reflect_attrs = field
                .attrs
                .iter()
                .filter_map(|attr| {
                    if attr.path().is_ident("reflect_attr") {
                        let args = attr
                            .parse_args_with(
                                Punctuated::<ReflectAttribute, Comma>::parse_terminated,
                            )
                            .unwrap()
                            .into_iter()
                            .collect::<Vec<_>>();
                        Some(attribute_map(args.as_slice()))
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or_else(|| quote! { [].into() });
            if field
                .attrs
                .iter()
                .all(|attr| !attr.path().is_ident("reflect_skip"))
            {
                if let Some(ident) = &field.ident {
                    let ty = &field.ty;
                    field_info.push((ident, ty, doc, reflect_attrs));
                }
            }
        }
    }

    let add_field_calls = field_info.iter().map(|(ident, ty, doc, attrs)| {
        quote! {
            .field::<#ty>(
                stringify!(#ident),
                #attrs,
                #doc,
                |x| {
                    match x.downcast_ref::<#name>() {
                        Some(value) => Some(&value.#ident),
                        None => None
                    }
                },
                |x| {
                    match x.downcast_mut::<#name>() {
                        Some(value) => Some(&mut value.#ident),
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
    let mut reflect_attrs = quote! { [].into() };
    for attr in attrs {
        if attr.path().is_ident("reflect") {
            trait_paths = Some(
                attr.parse_args_with(Punctuated::<Path, Comma>::parse_terminated)
                    .unwrap(),
            );
        } else if attr.path().is_ident("reflect_attr") {
            let args = attr
                .parse_args_with(Punctuated::<ReflectAttribute, Comma>::parse_terminated)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>();
            reflect_attrs = attribute_map(args.as_slice());
        }
    }

    let mut register_traits_impl = quote! {};
    if let Some(paths) = trait_paths {
        for trait_path in paths {
            let trait_ident = trait_path.segments.last().unwrap().ident.clone();
            let reflect_trait_ident =
                Ident::new(&format!("Reflect{}", trait_ident), Span::call_site());
            let register_trait_impl = quote! {
                registry.meta_impls::<#name, #reflect_trait_ident>();
            };
            register_traits_impl = quote! {
                #register_traits_impl
                #register_trait_impl
            };
        }
    }

    TokenStream::from(quote! {
        #[automatically_derived]
        impl #FQTypeName for #name {
            #[inline]
            fn type_name() -> &'static str { std::any::type_name::<Self>() }
            #[inline]
            fn type_name_short() -> &'static str { stringify!(#name) }
        }

        #[automatically_derived]
        impl #FQReflect for #name {
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
            #[inline]
            fn assign(&mut self, value: #FQBox<dyn #FQReflect>) -> bool {
                if let Ok(value) = value.downcast::<#name>() {
                    *self = *value;
                    true
                } else {
                    false
                }
            }
        }

        #[automatically_derived]
        impl #FQReflectedType for #name {
            fn register(registry: &mut engine::reflect::type_registry::TypeRegistry) {
                registry.meta_struct::<#name>(#reflect_attrs)
                    #(#add_field_calls)*;
                #register_traits_impl
            }
        }

        inventory::submit!(
            crate::ReflectRegistrationFn {
                name: stringify!(#name),
                function: <#name as #FQReflectedType>::register
            }
        );
    })
}
