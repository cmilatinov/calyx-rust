use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{
    parse_macro_input, DeriveInput, Expr, ExprLit, Lit, LitInt, LitStr, Meta, MetaNameValue, Path,
    Token,
};
use uuid::Uuid;

pub fn derive_type_uuid(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
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
    let expanded = if let Some(uuid) = uuid {
        let bytes = uuid
            .as_bytes()
            .iter()
            .map(|byte| format!("{:#X}", byte))
            .map(|byte_str| syn::parse_str::<LitInt>(&byte_str).unwrap());
        quote! {
            #[automatically_derived]
            impl #impl_generics engine::utils::TypeUuid for #name #ty_generics #where_clause {
                fn uuid_bytes() -> [u8; 16] {
                    [
                        #( #bytes ),*
                    ]
                }
            }
        }
    } else {
        quote! {
            #[automatically_derived]
            impl #impl_generics engine::utils::TypeUuid for #name #ty_generics #where_clause {
                fn uuid_bytes() -> [u8; 16] {
                    *engine::utils::uuid_from_str(::core::any::type_name::<Self>()).as_bytes()
                }
            }
        }
    };
    expanded.into()
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
        #[automatically_derived]
        impl engine::utils::TypeUuid for #path {
            fn uuid_bytes() -> [u8; 16] {
                [
                    #( #bytes ),*
                ]
            }
        }
    })
    .into()
}
