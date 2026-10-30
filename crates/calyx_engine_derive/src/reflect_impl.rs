use crate::fq::{FQAny, FQBox, FQReflect, FQReflectedType, FQTypeName};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::Path;

pub(crate) fn type_name_and_reflect_impls(name: &impl ToTokens) -> TokenStream {
    quote! {
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
    }
}

pub(crate) fn reflected_type_impl(
    name: &impl ToTokens,
    body: TokenStream,
    inventory_submit: TokenStream,
) -> TokenStream {
    quote! {
        #[automatically_derived]
        impl #FQReflectedType for #name {
            fn register(registry: &mut calyx_engine::reflect::type_registry::TypeRegistry) {
                #body
            }
        }

        #inventory_submit
    }
}

pub(crate) fn register_trait_meta_impls(
    name: &impl ToTokens,
    traits: impl IntoIterator<Item = Path>,
) -> syn::Result<TokenStream> {
    let mut tokens = TokenStream::new();
    for trait_path in traits {
        let Some(segment) = trait_path.segments.last() else {
            return Err(syn::Error::new_spanned(
                &trait_path,
                "expected a trait path, for example `Default`",
            ));
        };
        let reflect_trait_ident = format_ident!("Reflect{}", segment.ident);
        tokens = quote! {
            #tokens
            registry.meta_impls::<#name, #reflect_trait_ident>();
        };
    }
    Ok(tokens)
}
