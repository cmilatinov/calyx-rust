use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use sha1::Digest;
use syn::parse::{Parse, ParseStream};
use syn::{
    parse_macro_input, DeriveInput, Expr, ExprLit, Lit, LitInt, LitStr, Meta, MetaNameValue, Path,
    Token,
};
use uuid::Uuid;

use crate::fq::{FQTypeUuid, FQUuid};

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
        .map(|byte| LitInt::new(&format!("{byte:#X}"), Span::call_site()))
        .collect()
}

fn derive_uuid_attr(input: &DeriveInput) -> syn::Result<Option<Uuid>> {
    for attr in &input.attrs {
        if !attr.path().is_ident("uuid") {
            continue;
        }
        return match &attr.meta {
            Meta::NameValue(MetaNameValue {
                value:
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(lit), ..
                    }),
                ..
            }) => Uuid::parse_str(lit.value().as_str())
                .map(Some)
                .map_err(|err| syn::Error::new_spanned(lit, format!("invalid uuid: {err}"))),
            meta => Err(syn::Error::new_spanned(
                meta,
                "expected `#[uuid = \"...\"]` with a string literal",
            )),
        };
    }
    Ok(None)
}

pub fn derive_type_uuid(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let uuid = match derive_uuid_attr(&input) {
        Ok(uuid) => uuid.unwrap_or_else(|| uuid_from_str(name.to_string().as_str())),
        Err(err) => return err.to_compile_error().into(),
    };
    let bytes = uuid_lits(uuid);
    let fq_type_uuid = FQTypeUuid;
    let fq_uuid = FQUuid;
    quote! {
        #[automatically_derived]
        impl #impl_generics #fq_type_uuid for #name #ty_generics #where_clause {
            fn type_uuid() -> #fq_uuid {
                #fq_uuid::from_bytes([ #( #bytes ),* ])
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
    let uuid = match Uuid::parse_str(&uuid_str.value()) {
        Ok(uuid) => uuid,
        Err(err) => {
            return syn::Error::new_spanned(&uuid_str, format!("invalid uuid: {err}"))
                .into_compile_error()
                .into()
        }
    };
    let bytes = uuid_lits(uuid);
    let fq_type_uuid = FQTypeUuid;
    let fq_uuid = FQUuid;
    (quote! {
        #[automatically_derived]
        impl #fq_type_uuid for #path {
            fn type_uuid() -> #fq_uuid {
                #fq_uuid::from_bytes([ #( #bytes ),* ])
            }
        }
    })
    .into()
}
