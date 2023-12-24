use proc_macro::TokenStream;
use quote::quote;
use sha1::Digest;
use syn::parse::{Parse, ParseStream};
use syn::{
    parse_macro_input, DeriveInput, Expr, ExprLit, Lit, LitInt, LitStr, Meta, MetaNameValue, Path,
    Token,
};
use uuid::Uuid;

fn uuid_from_str(value: &str) -> Uuid {
    let mut hasher = sha1::Sha1::new();
    hasher.update(value.as_bytes());
    let hash = hasher.finalize();
    let mut bytes: uuid::Bytes = [0; 16];
    bytes.copy_from_slice(&hash.as_slice()[0..16]);
    Uuid::from_bytes(bytes)
}

pub fn derive_type_uuid(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let mut uuid = None;
    for attr in input.attrs.iter() {
        if attr.path().is_ident("uuid") {
            if let Meta::NameValue(MetaNameValue {
                value:
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(lit), ..
                    }),
                ..
            }) = &attr.meta
            {
                uuid = Uuid::parse_str(lit.value().as_str()).ok();
            }
        }
    }
    let uuid = uuid.unwrap_or_else(|| uuid_from_str(name.to_string().as_str()));
    let bytes = uuid
        .as_bytes()
        .iter()
        .map(|byte| format!("{:#X}", byte))
        .map(|byte_str| syn::parse_str::<LitInt>(&byte_str).unwrap());
    (quote! {
        impl reflect::TypeUuid for #name {
            const UUID: &'static [u8; 16] = &[
                #( #bytes ),*
            ];
        }
    })
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
    let bytes = uuid
        .as_bytes()
        .iter()
        .map(|byte| format!("{:#X}", byte))
        .map(|byte_str| syn::parse_str::<LitInt>(&byte_str).unwrap());
    (quote! {
        impl crate::TypeUuid for #path {
            const UUID: &'static [u8; 16] = &[
                #( #bytes ),*
            ];
        }
    })
    .into()
}
