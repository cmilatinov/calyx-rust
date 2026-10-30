use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

macro_rules! fq_path {
    ($name:ident => $path:path) => {
        pub(crate) struct $name;

        impl ToTokens for $name {
            fn to_tokens(&self, tokens: &mut TokenStream) {
                quote!($path).to_tokens(tokens);
            }
        }
    };
}

fq_path!(FQAny => ::core::any::Any);
fq_path!(FQBox => ::std::boxed::Box);
fq_path!(FQClone => ::core::clone::Clone);
fq_path!(FQOption => ::core::option::Option);
fq_path!(FQResult => ::core::result::Result);
fq_path!(FQReflect => calyx_engine::reflect::Reflect);
fq_path!(FQReflectedType => calyx_engine::reflect::ReflectedType);
fq_path!(FQTraitMeta => calyx_engine::reflect::TraitMeta);
fq_path!(FQTraitMetaFrom => calyx_engine::reflect::TraitMetaFrom);
fq_path!(FQAttributeValue => calyx_engine::reflect::AttributeValue);
fq_path!(FQTypeName => calyx_engine::reflect::TypeName);
fq_path!(FQTypeUuid => calyx_engine::utils::TypeUuid);
fq_path!(FQUuid => calyx_engine::utils::Uuid);
fq_path!(FQResource => calyx_engine::resource::Resource);
