use proc_macro::TokenStream;

use proc_macro2::{Ident, Span};
use quote::quote;
use syn::parse::Parse;
use syn::{parse_macro_input, Attribute, ItemTrait, Token};

use crate::fq::{
    FQBox, FQClone, FQOption, FQReflect, FQResult, FQTraitMeta, FQTraitMetaFrom, FQTypeUuid,
};

#[derive(Debug)]
pub(crate) struct TraitInfo {
    item_trait: ItemTrait,
}

impl Parse for TraitInfo {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![pub]) || lookahead.peek(Token![trait]) {
            let mut item_trait: ItemTrait = input.parse()?;
            item_trait.attrs = attrs;
            Ok(TraitInfo { item_trait })
        } else {
            Err(lookahead.error())
        }
    }
}

pub(crate) fn reflect_trait(_args: TokenStream, input: TokenStream) -> TokenStream {
    let trait_info = parse_macro_input!(input as TraitInfo);
    let item_trait = &trait_info.item_trait;
    let trait_ident = &item_trait.ident;
    let trait_vis = &item_trait.vis;
    let reflect_trait_ident =
        Ident::new(&format!("Reflect{}", item_trait.ident), Span::call_site());
    TokenStream::from(quote! {
        #item_trait

        #[derive(#FQClone, #FQTypeUuid)]
        #trait_vis struct #reflect_trait_ident {
            get_func: fn(&dyn #FQReflect) -> #FQOption<&dyn #trait_ident>,
            get_mut_func: fn(&mut dyn #FQReflect) -> #FQOption<&mut dyn #trait_ident>,
            get_boxed_func: fn(#FQBox<dyn #FQReflect>) -> #FQResult<#FQBox<dyn #trait_ident>, #FQBox<dyn #FQReflect>>,
        }

        impl #FQTraitMeta for #reflect_trait_ident {}

        impl #reflect_trait_ident {
            pub fn get<'a>(&self, value: &'a dyn #FQReflect) -> #FQOption<&'a dyn #trait_ident> {
                (self.get_func)(value)
            }
            pub fn get_mut<'a>(&self, value: &'a mut dyn #FQReflect) -> #FQOption<&'a mut dyn #trait_ident> {
                (self.get_mut_func)(value)
            }
            pub fn get_boxed(&self, value: #FQBox<dyn #FQReflect>) -> #FQResult<#FQBox<dyn #trait_ident>, #FQBox<dyn #FQReflect>> {
                (self.get_boxed_func)(value)
            }
        }

        impl<T: #trait_ident + #FQReflect> #FQTraitMetaFrom<T> for #reflect_trait_ident {
            fn trait_meta() -> Self {
                Self {
                    get_func: |reflect_value| {
                        <dyn #FQReflect>::downcast_ref::<T>(reflect_value).map(|value| value as &dyn #trait_ident)
                    },
                    get_mut_func: |reflect_value| {
                        <dyn #FQReflect>::downcast_mut::<T>(reflect_value).map(|value| value as &mut dyn #trait_ident)
                    },
                    get_boxed_func: |reflect_value| {
                        <dyn #FQReflect>::downcast::<T>(reflect_value).map(|value| value as #FQBox<dyn #trait_ident>)
                    }
                }
            }
        }
    })
}
