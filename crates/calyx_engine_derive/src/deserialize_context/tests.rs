use super::*;
use quote::ToTokens;
use syn::parse_quote;

fn context_of(input: DeriveInput) -> syn::Result<Option<String>> {
    extract_context_type(&input.attrs)
        .map(|ty| ty.map(|ty| ty.to_token_stream().to_string().replace(' ', "")))
}

#[test]
fn reads_the_context_path() {
    let ctx = context_of(parse_quote! { #[context(Ctx)] struct A; }).unwrap();
    assert_eq!(ctx.as_deref(), Some("Ctx"));
    let ctx = context_of(parse_quote! { #[context(a::Ctx)] struct A; }).unwrap();
    assert_eq!(ctx.as_deref(), Some("a::Ctx"));
}

#[test]
fn accepts_a_trailing_comma() {
    let ctx = context_of(parse_quote! { #[context(Ctx,)] struct A; }).unwrap();
    assert_eq!(ctx.as_deref(), Some("Ctx"));
}

#[test]
fn missing_context_is_none() {
    assert_eq!(context_of(parse_quote! { struct A; }).unwrap(), None);
}

#[test]
fn rejects_more_than_one_context() {
    assert!(context_of(parse_quote! { #[context(A, B)] struct A; }).is_err());
}
