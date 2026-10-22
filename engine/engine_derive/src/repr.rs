use syn::punctuated::Punctuated;
use syn::{Attribute, DeriveInput, Meta, Token};

/// Whether any `#[repr(...)]` attribute lists `C`, alone or alongside other
/// representation hints such as `align(N)` or `packed`.
fn has_repr_c(attrs: &[Attribute]) -> syn::Result<bool> {
    let mut found = false;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("repr")) {
        let hints = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        found |= hints.iter().any(|hint| hint.path().is_ident("C"));
    }
    Ok(found)
}

/// Fails with a compile error at the item when it is not `#[repr(C)]`.
pub(crate) fn require_repr_c(input: &DeriveInput, derive_name: &str) -> syn::Result<()> {
    if has_repr_c(&input.attrs)? {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.ident,
            format!("{derive_name} requires #[repr(C)]"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn repr_c(input: DeriveInput) -> bool {
        has_repr_c(&input.attrs).expect("repr attributes parse")
    }

    #[test]
    fn accepts_repr_c_alone_and_with_other_hints() {
        assert!(repr_c(parse_quote! { #[repr(C)] struct A; }));
        assert!(repr_c(parse_quote! { #[repr(C, align(8))] struct A; }));
        assert!(repr_c(parse_quote! { #[repr(align(8), C)] struct A; }));
        assert!(repr_c(parse_quote! { #[repr(C, packed)] struct A; }));
        assert!(repr_c(
            parse_quote! { #[repr(packed(2))] #[repr(C)] struct A; }
        ));
    }

    #[test]
    fn rejects_items_without_repr_c() {
        assert!(!repr_c(parse_quote! { struct A; }));
        assert!(!repr_c(parse_quote! { #[repr(transparent)] struct A; }));
        assert!(!repr_c(parse_quote! { #[repr(align(8))] struct A; }));
        assert!(!repr_c(parse_quote! { #[derive(Clone)] struct A; }));
    }

    #[test]
    fn missing_repr_c_names_the_derive() {
        let input: DeriveInput = parse_quote! { struct A; };
        let err = require_repr_c(&input, "Resource").unwrap_err();
        assert_eq!(err.to_string(), "Resource requires #[repr(C)]");
    }
}
