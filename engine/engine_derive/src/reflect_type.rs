use proc_macro::TokenStream;

use crate::fq::FQAttributeValue;
use crate::reflect_impl::{
    reflected_type_impl, register_trait_meta_impls, type_name_and_reflect_impls,
};
use darling::ast::NestedMeta;
use proc_macro2::{Ident, Span};
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, Fields, Lit, LitStr, Meta, MetaNameValue, Path,
};

#[derive(Debug)]
struct ReflectAttribute {
    name: Ident,
    value: Option<Lit>,
}

impl TryFrom<NestedMeta> for ReflectAttribute {
    type Error = darling::Error;

    fn try_from(value: NestedMeta) -> darling::Result<Self> {
        match value {
            NestedMeta::Meta(Meta::Path(path)) => {
                let Some(name) = path.get_ident().cloned() else {
                    return Err(darling::Error::custom(
                        "reflect_attr keys must be identifiers",
                    ));
                };
                Ok(Self { name, value: None })
            }
            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                path,
                value: Expr::Lit(ExprLit { lit, .. }),
                ..
            })) => {
                let Some(name) = path.get_ident().cloned() else {
                    return Err(darling::Error::custom(
                        "reflect_attr keys must be identifiers",
                    ));
                };
                Ok(Self {
                    name,
                    value: Some(lit),
                })
            }
            _ => Err(darling::Error::custom(
                "reflect_attr entries must be `name` or `name = literal`",
            )),
        }
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

    let mut trait_paths = None;
    let mut reflect_attrs = quote! { [].into() };
    for attr in attrs {
        if attr.path().is_ident("reflect") {
            trait_paths = Some(reflect_trait_paths(attr).unwrap());
        } else if attr.path().is_ident("reflect_attr") {
            reflect_attrs = attribute_map(&reflect_attributes(attr).unwrap());
        }
    }

    let reflect_impls = type_name_and_reflect_impls(name);
    let register_traits_impl = register_trait_meta_impls(name, trait_paths.unwrap_or_default());
    let register_type_info = match &ast.data {
        Data::Struct(s) => {
            if !has_repr_c(attrs) {
                panic!("Reflect requires #[repr(C)]");
            }
            struct_type_info(name, &s.fields, &reflect_attrs)
        }
        Data::Enum(e) => enum_type_info(name, e.variants.iter(), &reflect_attrs),
        _ => panic!("Reflect only works on structs and enums!"),
    };
    let reflected_type_impl = reflected_type_impl(
        name,
        quote! {
            #register_type_info
            #register_traits_impl
        },
        quote! {
            inventory::submit!(
                engine::ReflectRegistrationFn {
                    name: stringify!(#name),
                    function: <#name as engine::reflect::ReflectedType>::register
                }
            );
        },
    );

    TokenStream::from(quote! {
        #reflect_impls
        #reflected_type_impl
    })
}

fn struct_type_info(
    name: &Ident,
    fields: &Fields,
    reflect_attrs: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let mut field_info = Vec::new();
    if let Fields::Named(named) = fields {
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
                        Some(attribute_map(&reflect_attributes(attr).unwrap()))
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

    quote! {
        registry.meta_struct::<#name>(#reflect_attrs)
            #(#add_field_calls)*;
    }
}

fn enum_type_info<'a>(
    name: &Ident,
    variants: impl Iterator<Item = &'a syn::Variant>,
    reflect_attrs: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let add_variant_calls = variants.map(|variant| {
        let variant_name = variant.ident.to_string();
        let variant_name = LitStr::new(variant_name.as_str(), Span::call_site());
        let fields = enum_variant_fields(&variant.fields);
        quote! {
            .variant(#variant_name, vec![#(#fields),*])
        }
    });

    quote! {
        registry.meta_enum::<#name>(#reflect_attrs)
            #(#add_variant_calls)*;
    }
}

fn enum_variant_fields(fields: &Fields) -> Vec<proc_macro2::TokenStream> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let ty = &field.ty;
            let name = field
                .ident
                .as_ref()
                .map(|ident| {
                    let name = ident.to_string();
                    let name = LitStr::new(name.as_str(), Span::call_site());
                    quote! { Some(#name) }
                })
                .unwrap_or_else(|| {
                    let _ = index;
                    quote! { None }
                });
            quote! {
                engine::reflect::EnumVariantFieldInfo {
                    name: #name,
                    type_id: std::any::TypeId::of::<#ty>(),
                    type_uuid: <#ty as engine::utils::TypeUuid>::type_uuid(),
                    type_name: std::any::type_name::<#ty>(),
                }
            }
        })
        .collect()
}

fn reflect_attributes(attr: &Attribute) -> darling::Result<Vec<ReflectAttribute>> {
    let list = attr.meta.require_list().map_err(darling::Error::from)?;
    NestedMeta::parse_meta_list(list.tokens.clone())?
        .into_iter()
        .map(ReflectAttribute::try_from)
        .collect()
}

fn reflect_trait_paths(attr: &Attribute) -> darling::Result<Vec<Path>> {
    let list = attr.meta.require_list().map_err(darling::Error::from)?;
    NestedMeta::parse_meta_list(list.tokens.clone())?
        .into_iter()
        .map(|meta| match meta {
            NestedMeta::Meta(Meta::Path(path)) => Ok(path),
            _ => Err(darling::Error::custom(
                "reflect traits must be paths, for example #[reflect(Default)]",
            )),
        })
        .collect()
}

fn has_repr_c(attrs: &[Attribute]) -> bool {
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
