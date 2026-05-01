use proc_macro::TokenStream;
use quote::quote;
use sha1::Digest;
use syn::parse::{Parse, ParseStream};
use syn::{
    parse_macro_input, DeriveInput, Expr, ExprLit, Lit, LitInt, LitStr, Meta, MetaNameValue, Path,
    Token,
};
use uuid::Uuid;

use crate::fq::{FQTypeUuid, FQTypeUuidDynamic, FQUuid};

fn uuid_from_str(value: &str) -> Uuid {
    let mut hasher = sha1::Sha1::new();
    hasher.update(value.as_bytes());
    let hash = hasher.finalize();
    let mut bytes: uuid::Bytes = [0; 16];
    bytes.copy_from_slice(&hash.as_slice()[0..16]);
    Uuid::from_bytes(bytes)
}

fn uuid_lits(uuid: Uuid) -> Vec<LitInt> {
    uuid.as_bytes()
        .iter()
        .map(|byte| format!("{:#X}", byte))
        .map(|byte_str| syn::parse_str::<LitInt>(&byte_str).unwrap())
        .collect()
}

fn derive_uuid_attr(input: &DeriveInput) -> Option<Uuid> {
    for attr in &input.attrs {
        if !attr.path().is_ident("uuid") {
            continue;
        }
        if let Meta::NameValue(MetaNameValue {
            value: Expr::Lit(ExprLit {
                lit: Lit::Str(lit), ..
            }),
            ..
        }) = &attr.meta
        {
            return Uuid::parse_str(lit.value().as_str()).ok();
        }
    }
    None
}

pub fn derive_type_uuid(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let uuid = derive_uuid_attr(&input).unwrap_or_else(|| uuid_from_str(name.to_string().as_str()));
    let bytes = uuid_lits(uuid);
    let fq_type_uuid = FQTypeUuid;
    let fq_type_uuid_dynamic = FQTypeUuidDynamic;
    let fq_uuid = FQUuid;
    quote! {
        #[automatically_derived]
        impl #impl_generics #fq_type_uuid for #name #ty_generics #where_clause {
            const UUID: &'static [u8; 16] = &[
                #( #bytes ),*
            ];
        }

        #[automatically_derived]
        impl #impl_generics #fq_type_uuid_dynamic for #name #ty_generics #where_clause {
            fn uuid_bytes(&self) -> &'static [u8; 16] {
                <Self as #fq_type_uuid>::UUID
            }

            fn uuid(&self) -> #fq_uuid {
                <Self as #fq_type_uuid>::type_uuid()
            }
        }
    }
    .into()
}

struct ExternTypeUuidInput {
    path: Path,
    _comma: Token![,],
    uuid_str: LitStr,
}

impl Parse for ExternTypeUuidInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            path: input.parse()?,
            _comma: input.parse()?,
            uuid_str: input.parse()?,
        })
    }
}

pub fn extern_type_uuid(input: TokenStream) -> TokenStream {
    let ExternTypeUuidInput { path, uuid_str, .. } =
        parse_macro_input!(input as ExternTypeUuidInput);
    let uuid = Uuid::parse_str(&uuid_str.value()).expect("Value was not a valid UUID");
    let bytes = uuid_lits(uuid);
    let fq_type_uuid = FQTypeUuid;
    let fq_type_uuid_dynamic = FQTypeUuidDynamic;
    let fq_uuid = FQUuid;
    (quote! {
        #[automatically_derived]
        impl #fq_type_uuid for #path {
            const UUID: &'static [u8; 16] = &[
                #( #bytes ),*
            ];
        }

        #[automatically_derived]
        impl #fq_type_uuid_dynamic for #path {
            fn uuid_bytes(&self) -> &'static [u8; 16] {
                <Self as #fq_type_uuid>::UUID
            }

            fn uuid(&self) -> #fq_uuid {
                <Self as #fq_type_uuid>::type_uuid()
            }
        }
    })
    .into()
}
