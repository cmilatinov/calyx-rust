use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn derive_post_process_effect(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        #[automatically_derived]
        impl #impl_generics calyx_engine::component::PostProcessEffect
            for #name #ty_generics #where_clause
        {
            fn copy_from(&mut self, source: &dyn calyx_engine::component::PostProcessEffect) {
                let source = source.as_any().downcast_ref::<Self>()
                    .expect("post-process effects must have the same concrete type");
                calyx_engine::render::postprocess::BlendEffect::copy_in_place(self, source);
            }

            fn blend_into(
                &mut self,
                target: &dyn calyx_engine::component::PostProcessEffect,
                factor: f32,
            ) {
                let target = target.as_any().downcast_ref::<Self>()
                    .expect("post-process effects must have the same concrete type");
                calyx_engine::render::postprocess::BlendEffect::blend_in_place(
                    self, target, factor.clamp(0.0, 1.0),
                );
            }

            fn reset_neutral(&mut self) -> bool {
                calyx_engine::render::postprocess::BlendEffect::reset_neutral_in_place(self)
            }
        }
    }
    .into()
}
