use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

pub(crate) struct FQAny;
pub(crate) struct FQBox;
pub(crate) struct FQClone;
pub(crate) struct FQDefault;
pub(crate) struct FQOption;
pub(crate) struct FQResult;
pub(crate) struct FQSend;
pub(crate) struct FQSync;

pub(crate) struct FQReflect;

pub(crate) struct FQTraitMeta;

pub(crate) struct FQTraitMetaFrom;

impl ToTokens for FQAny {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::core::any::Any).to_tokens(tokens);
    }
}

impl ToTokens for FQBox {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::std::boxed::Box).to_tokens(tokens);
    }
}

impl ToTokens for FQClone {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::core::clone::Clone).to_tokens(tokens);
    }
}

impl ToTokens for FQDefault {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::core::default::Default).to_tokens(tokens);
    }
}

impl ToTokens for FQOption {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::core::option::Option).to_tokens(tokens);
    }
}

impl ToTokens for FQResult {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::core::result::Result).to_tokens(tokens);
    }
}

impl ToTokens for FQSend {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::core::marker::Send).to_tokens(tokens);
    }
}

impl ToTokens for FQSync {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::core::marker::Sync).to_tokens(tokens);
    }
}

impl ToTokens for FQReflect {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::reflect::Reflect).to_tokens(tokens)
    }
}

impl ToTokens for FQTraitMeta {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::reflect::TraitMeta).to_tokens(tokens)
    }
}

impl ToTokens for FQTraitMetaFrom {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(::reflect::TraitMetaFrom).to_tokens(tokens)
    }
}