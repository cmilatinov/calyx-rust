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
fq_path!(FQReflect => engine::reflect::Reflect);
fq_path!(FQReflectedType => engine::reflect::ReflectedType);
fq_path!(FQTraitMeta => engine::reflect::TraitMeta);
fq_path!(FQTraitMetaFrom => engine::reflect::TraitMetaFrom);
fq_path!(FQAttributeValue => engine::reflect::AttributeValue);
fq_path!(FQTypeName => engine::reflect::TypeName);
fq_path!(FQTypeUuid => engine::utils::TypeUuid);
fq_path!(FQTypeUuidDynamic => engine::utils::TypeUuidDynamic);
fq_path!(FQUuid => engine::utils::Uuid);
fq_path!(FQResource => engine::resource::Resource);
