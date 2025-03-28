use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, GenericParam, Generics, Lifetime, LifetimeParam,
    LitStr, Type, TypePath, Variant,
};

/// The main entry point for the proc macro
pub fn derive_deserialize_with_context(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the struct
    let type_name = &input.ident;

    // Extract the context type from the struct attributes
    let context_type =
        extract_context_type(&input.attrs).expect("DeserializeWithContext requires a context type");

    // Add a lifetime parameter 'de for trait impl
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut trait_generics = input.generics.clone();
    if trait_generics
        .lifetimes()
        .all(|lt| lt.lifetime.ident != "de")
    {
        trait_generics
            .params
            .push(GenericParam::Lifetime(LifetimeParam::new(Lifetime::new(
                "'de",
                Span::call_site(),
            ))));
    }
    let (impl_generics, _, _) = trait_generics.split_for_impl();

    // Generate implementation based on whether it's a struct or enum
    let implementation = match &input.data {
        Data::Struct(data) => {
            // panic!("{}", quote!(#ty_generics).to_string());
            generate_struct_impl(
                type_name,
                &input.generics,
                &trait_generics,
                &context_type,
                data,
            )
        }
        Data::Enum(data) => generate_enum_impl(
            type_name,
            &input.generics,
            &trait_generics,
            &context_type,
            data.variants.iter().collect(),
        ),
        Data::Union(_) => panic!("DeserializeWithContext does not support unions"),
    };

    // Generate the final implementation
    let expanded = quote! {
        #[automatically_derived]
        impl #impl_generics serde::de::DeserializeSeed<'de>
        for engine::utils::ContextSeed<'de, #context_type, #type_name #ty_generics> #where_clause {
            type Value = #type_name #ty_generics;

            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: serde::Deserializer<'de>
            {
                #implementation
            }
        }
    };

    TokenStream::from(expanded)
}

// Generate implementation for structs
fn generate_struct_impl(
    struct_name: &Ident,
    struct_generics: &Generics,
    trait_generics: &Generics,
    context_type: &Type,
    data: &syn::DataStruct,
) -> proc_macro2::TokenStream {
    // Extract the fields of the struct
    let fields = match &data.fields {
        Fields::Named(fields) => &fields.named,
        Fields::Unnamed(_) => {
            panic!("DeserializeWithContext only supports structs with named fields")
        }
        Fields::Unit => panic!("DeserializeWithContext only supports structs with fields"),
    };
    let (_, struct_ty_generics, _) = struct_generics.split_for_impl();
    let (trait_impl_generics, trait_ty_generics, trait_where_clause) =
        trait_generics.split_for_impl();

    let mut visitor_fields = quote! {};
    let mut visitor_initializers = quote! {};
    for ty in struct_generics.type_params() {
        let ty_ident = &ty.ident;
        let field_name = format_ident!("ty_{}", ty.ident);
        visitor_fields = quote! {
            #visitor_fields
            #field_name: std::marker::PhantomData<#ty_ident>,
        };
        visitor_initializers = quote! {
            #visitor_initializers
            #field_name: std::marker::PhantomData,
        };
    }

    // Process fields to generate field visitor implementations
    let mut field_matchers = Vec::new();
    let mut field_defaults = quote! {};
    let mut field_initializers = quote! {};
    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        let field_missing = LitStr::new(
            format!("Missing field '{}'", field_name).as_str(),
            Span::call_site(),
        );
        if has_skip_attribute(&field.attrs) {
            field_initializers = quote! {
                #field_initializers
                #field_name: Default::default(),
            };
            continue;
        } else {
            field_defaults = quote! {
                #field_defaults
                let mut #field_name: Option<#field_type> = None;
            };
            field_initializers = quote! {
                #field_initializers
                #field_name: #field_name.expect(#field_missing),
            };
        }

        // Check if the field has a #[context] attribute
        if has_context_attribute(&field.attrs) {
            field_matchers.push(quote! {
                stringify!(#field_name) => {
                    let seed = engine::utils::ContextSeed::<#context_type, #field_type>::new(self.context);
                    #field_name = Some(map.next_value_seed(seed)?);
                }
            });
        } else {
            field_matchers.push(quote! {
                stringify!(#field_name) => {
                    #field_name = Some(map.next_value()?);
                }
            });
        }
    }

    // Generate struct visitor implementation
    quote! {
        struct FieldVisitor #trait_ty_generics {
            context: &'de #context_type,
            #visitor_fields
        }

        impl #trait_impl_generics serde::de::Visitor<'de>
        for FieldVisitor #trait_ty_generics #trait_where_clause {
            type Value = #struct_name #struct_ty_generics;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(concat!("struct ", stringify!(#struct_name)))
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                #field_defaults

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        #(#field_matchers)*
                        _ => {
                            let _ = map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                Ok(#struct_name {
                    #field_initializers
                })
            }
        }

        let visitor = FieldVisitor {
            context: self.context,
            #visitor_initializers
        };
        deserializer.deserialize_map(visitor)
    }
}

// Generate implementation for enums
fn generate_enum_impl(
    enum_name: &Ident,
    struct_generics: &Generics,
    trait_generics: &Generics,
    context_type: &Type,
    variants: Vec<&Variant>,
) -> proc_macro2::TokenStream {
    let (_, struct_ty_generics, _) = struct_generics.split_for_impl();
    let (trait_impl_generics, trait_ty_generics, trait_where_clause) =
        trait_generics.split_for_impl();

    let mut enum_kind_values = quote! {};
    let mut variant_names = quote! {};
    let mut variant_kind_matchers = quote! {};
    for variant in variants.iter() {
        let variant_ident = &variant.ident;
        enum_kind_values = quote! {
            #enum_kind_values
            #variant_ident,
        };
        variant_names = quote! {
            #variant_names
            stringify!(#variant_ident),
        };
        variant_kind_matchers = quote! {
            #variant_kind_matchers
            stringify!(#variant_ident) => Ok(EnumKind::#variant_ident),
        }
    }

    // Generate variant matching logic
    let variant_matchers = variants.iter().map(|variant| {
        let variant_name = &variant.ident;
        quote! {
            (EnumKind::#variant_name, value) => value.newtype_variant_seed(
                engine::utils::ContextSeed::<#context_type, #enum_name>::new(self.context)
            ),
        }
    });

    // Generate enum visitor implementation
    quote! {
        enum EnumKind {
            #enum_kind_values
        }

        static VARIANTS: &[&str] = &[#variant_names];

        impl #trait_impl_generics serde::de::Deserialize<'de> for EnumKind {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>
            {
                struct KindVisitor;
                impl #trait_impl_generics serde::de::Visitor<'de> for KindVisitor {
                    type Value = EnumKind;
                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        write!(formatter, "one of {:?}", VARIANTS)
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            #variant_kind_matchers
                            _ => Err(serde::de::Error::unknown_variant(value, VARIANTS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(KindVisitor)
            }
        }

        struct EnumVisitor<'de> {
            context: &'de #context_type,
        }
        impl<'de> serde::de::Visitor<'de> for EnumVisitor #trait_ty_generics #trait_where_clause {
            type Value = #enum_name #struct_ty_generics;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(stringify!(#enum_name))
            }

            fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::EnumAccess<'de>,
            {
                use serde::de::VariantAccess;
                match data.variant()? {
                    #(#variant_matchers)*
                }
            }
        }

        let visitor = EnumVisitor { context: self.context };
        deserializer.deserialize_enum(
            stringify!(#enum_name),
            VARIANTS,
            visitor
        )
    }
}

// Check if field has #[use_context]
fn has_context_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("use_context"))
}

// Check if field has #[skip_deserialize]
fn has_skip_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("skip_deserialize"))
}

// Extract context type from #[context(...)]
fn extract_context_type(attrs: &[syn::Attribute]) -> Option<Type> {
    let attr = attrs.iter().find(|attr| attr.path().is_ident("context"))?;
    let mut type_path = None;
    let _ = attr.parse_nested_meta(|meta| {
        type_path = Some(Type::Path(TypePath {
            qself: None,
            path: meta.path.clone(),
        }));
        Ok(())
    });
    type_path
}
