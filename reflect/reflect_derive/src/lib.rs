use proc_macro::TokenStream;

mod reflect_type;
mod reflect_trait;
mod reflect_value;
mod fq;

#[proc_macro_derive(Reflect, attributes(reflect))]
pub fn derive_reflect(input: TokenStream) -> TokenStream {
    reflect_type::derive_reflect(input)
}

#[proc_macro_attribute]
pub fn reflect_trait(args: TokenStream, input: TokenStream) -> TokenStream {
    reflect_trait::reflect_trait(args, input)
}

#[proc_macro]
pub fn impl_reflect_value(input: TokenStream) -> TokenStream {
    reflect_value::impl_reflect_value(input)
}