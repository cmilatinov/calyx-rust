use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

pub(crate) struct FQAny;

pub(crate) struct FQBox;

pub(crate) struct FQClone;

pub(crate) struct FQOption;

pub(crate) struct FQResult;

pub(crate) struct FQReflect;

pub(crate) struct FQReflectedType;

pub(crate) struct FQTraitMeta;

pub(crate) struct FQTraitMetaFrom;

pub(crate) struct FQAttributeValue;

pub(crate) struct FQTypeName;

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

impl ToTokens for FQReflect {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(engine::reflect::Reflect).to_tokens(tokens)
    }
}

impl ToTokens for FQReflectedType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(engine::reflect::ReflectedType).to_tokens(tokens)
    }
}

impl ToTokens for FQTraitMeta {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(engine::reflect::TraitMeta).to_tokens(tokens)
    }
}

impl ToTokens for FQTraitMetaFrom {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(engine::reflect::TraitMetaFrom).to_tokens(tokens)
    }
}

impl ToTokens for FQAttributeValue {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(engine::reflect::AttributeValue).to_tokens(tokens)
    }
}

impl ToTokens for FQTypeName {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(engine::reflect::TypeName).to_tokens(tokens)
    }
}
