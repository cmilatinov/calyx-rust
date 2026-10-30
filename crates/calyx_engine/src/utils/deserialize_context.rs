use serde::de::{DeserializeSeed, Error, SeqAccess, Visitor};
use serde::Deserializer;
use std::marker::PhantomData;

pub struct ContextSeed<'context, C, T> {
    pub context: &'context C,
    marker: PhantomData<T>,
}

impl<'context, C, T> ContextSeed<'context, C, T> {
    pub fn new(context: &'context C) -> Self {
        Self {
            context,
            marker: PhantomData,
        }
    }
}

impl<'context, C, T> Clone for ContextSeed<'context, C, T> {
    fn clone(&self) -> Self {
        Self {
            context: self.context,
            marker: PhantomData,
        }
    }
}

impl<'de, C, T> DeserializeSeed<'de> for ContextSeed<'de, C, Option<T>>
where
    ContextSeed<'de, C, T>: DeserializeSeed<'de, Value = T>,
{
    type Value = Option<T>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptionVisitor<'de, C, T> {
            context: &'de C,
            marker: PhantomData<T>,
        }

        impl<'de, C, T> Visitor<'de> for OptionVisitor<'de, C, T>
        where
            ContextSeed<'de, C, T>: DeserializeSeed<'de, Value = T>,
        {
            type Value = Option<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("option")
            }

            #[inline]
            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(None)
            }

            #[inline]
            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                ContextSeed::<C, T>::new(self.context)
                    .deserialize(deserializer)
                    .map(Some)
            }

            #[inline]
            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(None)
            }
        }

        deserializer.deserialize_option(OptionVisitor::<C, T> {
            context: self.context,
            marker: PhantomData,
        })
    }
}

impl<'de, C, T> DeserializeSeed<'de> for ContextSeed<'de, C, Vec<T>>
where
    ContextSeed<'de, C, T>: DeserializeSeed<'de, Value = T>,
{
    type Value = Vec<T>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VecVisitor<'de, C, T> {
            context: &'de C,
            marker: PhantomData<T>,
        }

        impl<'de, C, T> Visitor<'de> for VecVisitor<'de, C, T>
        where
            ContextSeed<'de, C, T>: DeserializeSeed<'de, Value = T>,
        {
            type Value = Vec<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a sequence")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let capacity = {
                    const MAX_PREALLOC_BYTES: usize = 1024 * 1024;
                    if size_of::<T>() == 0 {
                        0
                    } else {
                        std::cmp::min(
                            seq.size_hint().unwrap_or(0),
                            MAX_PREALLOC_BYTES / size_of::<T>(),
                        )
                    }
                };
                let mut values = Vec::<T>::with_capacity(capacity);
                while let Some(value) =
                    seq.next_element_seed(ContextSeed::<C, T>::new(self.context))?
                {
                    values.push(value);
                }

                Ok(values)
            }
        }

        deserializer.deserialize_seq(VecVisitor::<C, T> {
            context: self.context,
            marker: PhantomData,
        })
    }
}
