use proc_macro::TokenStream;
use std::str::FromStr;

use proc_macro2::{Ident, Span};
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{
    parse2, parse_macro_input, parse_quote, parse_str, Error, Expr, ItemStruct, Type, Visibility,
};

use crate::attr::{ContainerAttr, FieldAttr};

mod attr;
mod clause;

#[proc_macro]
pub fn clause(input: TokenStream) -> TokenStream {
    if input.is_empty() {
        return quote!(("", &[] as &[&(dyn sorm::Param + Sync)])).into();
    }
    match clause::expand(parse_macro_input!(input)) {
        Ok(v) => v.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_attribute]
pub fn sorm(attr: TokenStream, input: TokenStream) -> TokenStream {
    let attr = parse_macro_input!(attr as ContainerAttr);
    let mut item = parse_macro_input!(input as ItemStruct);
    match expand(attr, &mut item) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(attr: ContainerAttr, item: &mut ItemStruct) -> syn::Result<proc_macro2::TokenStream> {
    if !item.generics.params.is_empty() {
        return Err(Error::new_spanned(&item.generics, "generic is not allowed"));
    }

    let fields = collect_field(item)?;
    let impl_self = impl_self(item, &fields);
    let impl_model = impl_model(item, &fields, attr.table);
    let impl_from_row = impl_from_row(item, &fields);
    let impl_serialize = attr.serialize.then(|| impl_serialize(item, &fields));
    let impl_deserialize = attr.deserialize.then(|| impl_deserialize(item, &fields));
    modify_item(item, fields.attr_index())?;

    let vis = item.vis.clone();
    item.vis = parse_quote!(pub);
    let ident = &item.ident;
    let module = format_ident!("__sorm_{}", ident.to_string().to_ascii_lowercase());
    Ok(quote! {
        mod #module {
            use super::*;
            #item
            #impl_self
            #impl_model
            #impl_from_row
            #impl_serialize
            #impl_deserialize
        }
        #vis use #module::#ident;
    })
}

fn impl_self(item: &ItemStruct, fields: &Fields) -> proc_macro2::TokenStream {
    let fields_name = fields.names();
    let fields_ident = fields.idents();
    let fields_ident_upper = fields_name
        .iter()
        .map(|v| format_ident!("{}", v.to_ascii_uppercase()))
        .collect::<Vec<_>>();
    let fields_type = fields.types();
    let setter = fields_name
        .iter()
        .map(|v| format_ident!("set_{}", v))
        .collect::<Vec<_>>();
    let taker = fields_name
        .iter()
        .map(|v| format_ident!("take_{}", v))
        .collect::<Vec<_>>();
    let seq = fields.seq();
    let ident = &item.ident;
    let new = gen_new(&fields_ident, &fields_type);
    quote! {
        impl #ident {
            #(pub const #fields_ident_upper: &'static str = #fields_name;)*

            #new

            #(
                #[inline]
                pub fn #fields_ident(&self) -> sorm::Result<&#fields_type> {
                    match (self.__sorm_set >> #seq) & 1 {
                        1 => Ok(&self.#fields_ident),
                        _ => Err(sorm::Error::FieldAbsent(#fields_name)),
                    }
                }

                #[inline]
                pub fn #taker(&mut self) -> sorm::Result<#fields_type> {
                    match (self.__sorm_set >> #seq) & 1 {
                        1 => {
                            let v = std::mem::take(&mut self.#fields_ident);
                            self.__sorm_set &= !(1 << #seq);
                            self.__sorm_update &= !(1 << #seq);
                            Ok(v)
                        }
                        _ => Err(sorm::Error::FieldAbsent(#fields_name)),
                    }
                }

                #[inline]
                pub fn #setter(&mut self, v: #fields_type) {
                    self.#fields_ident = v;
                    self.__sorm_set |= 1 << #seq;
                    self.__sorm_update |= 1 << #seq;
                }
            )*

            pub fn isset(&self, field: &str) -> bool {
                match field {
                    #(#fields_name => self.__sorm_set & (1 << #seq) > 0,)*
                    _ => panic!("field `{}` not exists", field),
                }
            }

            pub fn unset(&mut self, field: &str) {
                match field {
                    #(
                        #fields_name => {
                            if (self.__sorm_set >> #seq) & 1 == 1 {
                                self.__sorm_set &= !(1 << #seq);
                                self.#fields_ident = std::default::Default::default();
                            }
                        }
                    )*
                    _ => panic!("field `{}` not exists", field),
                }
            }
        }
    }
}

fn gen_new(fields_ident: &[&Ident], fields_type: &[&Type]) -> proc_macro2::TokenStream {
    let mut assert = Vec::with_capacity(fields_ident.len());
    for ty in fields_type {
        assert.push(quote_spanned! {ty.span()=>
            { struct _Assert where #ty: std::default::Default; }
        })
    }
    quote! {
        pub fn new() -> Self {
            #(#assert)*
            Self {
                #(
                    #fields_ident: std::default::Default::default(),
                )*
                __sorm_set: 0,
                __sorm_update: 0,
            }
        }
    }
}

fn impl_serialize(item: &ItemStruct, fields: &Fields) -> proc_macro2::TokenStream {
    let fields_name = fields.names();
    let fields_ident = fields.idents();
    let seq = fields.seq();
    let ident = &item.ident;
    let name = ident.to_string();
    quote! {
        impl serde::ser::Serialize for #ident {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::ser::Serializer,
            {
                let mut len = 0;
                let mut n = self.__sorm_set;
                while n > 0 {
                    len += (n & 1) as usize;
                    n >>= 1;
                }

                use serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(#name, len)?;
                #(
                    if (self.__sorm_set >> #seq) & 1 == 1 {
                        state.serialize_field(#fields_name, &self.#fields_ident)?;
                    }
                )*
                state.end()
            }
        }
    }
}

fn impl_deserialize(item: &ItemStruct, fields: &Fields) -> proc_macro2::TokenStream {
    let fields_name = fields.names();
    let fields_ident = fields.idents();
    let seq = fields.seq();
    let ident = &item.ident;
    let name = ident.to_string();
    let expect = format!("struct {}", name);
    let expect_field = fields_name
        .iter()
        .map(|v| format!("`{}`", v))
        .collect::<Vec<_>>()
        .join(" OR ");
    quote! {
        impl<'de> serde::de::Deserialize<'de> for #ident {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::de::Deserializer<'de>,
            {
                #[allow(non_camel_case_types)]
                enum Field {
                    #(#fields_ident,)*
                    __ignore,
                }

                impl<'de> serde::de::Deserialize<'de> for Field {
                    fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
                    where
                        D: serde::de::Deserializer<'de>,
                    {
                        struct FieldVisitor;

                        impl<'de> serde::de::Visitor<'de> for FieldVisitor {
                            type Value = Field;

                            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                                formatter.write_str(#expect_field)
                            }

                            fn visit_str<E>(self, value: &str) -> Result<Field, E>
                            where
                                E: serde::de::Error,
                            {
                                match value {
                                    #(#fields_name => Ok(Field::#fields_ident),)*
                                    _ => Ok(Field::__ignore),
                                }
                            }
                        }

                        deserializer.deserialize_identifier(FieldVisitor)
                    }
                }

                struct Visitor;

                impl<'de> serde::de::Visitor<'de> for Visitor {
                    type Value = #ident;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str(#expect)
                    }

                    fn visit_map<V>(self, mut map: V) -> Result<#ident, V::Error>
                        where
                            V: serde::de::MapAccess<'de>,
                    {
                        let mut v = #ident::new();
                        while let Some(key) = map.next_key()? {
                            match key {
                                #(
                                    Field::#fields_ident => {
                                        if (v.__sorm_set >> #seq) & 1 == 1 {
                                            return Err(serde::de::Error::duplicate_field(#fields_name));
                                        }
                                        v.#fields_ident = map.next_value()?;
                                        v.__sorm_set |= 1 << #seq;
                                    }
                                )*
                                _ => {
                                    map.next_value::<serde::de::IgnoredAny>()?;
                                }
                            }
                        }
                        Ok(v)
                    }
                }

                const FIELDS: &[&str] = &[#(#fields_name),*];
                deserializer.deserialize_struct(#name, FIELDS, Visitor)
            }
        }
    }
}

fn impl_from_row(item: &ItemStruct, fields: &Fields) -> proc_macro2::TokenStream {
    let fields_name = fields.names();
    let fields_ident = fields.idents();
    let fields_type = fields.types();
    let seq = fields.seq();
    let ident = &item.ident;

    let mut assert = Vec::with_capacity(fields_name.len());
    for ty in &fields_type {
        assert.push(quote_spanned! {ty.span()=>
            { struct _Assert where #ty: sorm::sqlx::Type<sorm::Database> + for<'r> sorm::sqlx::Decode<'r, sorm::Database>; }
        })
    }
    quote! {
        impl sorm::sqlx::FromRow<'_, sorm::Row> for #ident {
            fn from_row(row: &sorm::Row) -> sorm::sqlx::Result<Self> {
                #(#assert)*
                use sorm::sqlx::Row;
                let mut model = Self::new();
                #(
                    match row.try_get(#fields_name) {
                        Ok(v) => {
                            model.#fields_ident = v;
                            model.__sorm_set |= 1 << #seq;
                        }
                        Err(sorm::sqlx::Error::ColumnNotFound(_)) => (),
                        Err(err) => return Err(err),
                    }
                )*
                Ok(model)
            }
        }
    }
}

fn impl_model(
    item: &ItemStruct,
    fields: &Fields,
    table: Option<String>,
) -> proc_macro2::TokenStream {
    let fields_name = fields.names();
    let fields_ident = fields.idents();
    let ident = &item.ident;
    let seq = fields.seq();
    let table = table.unwrap_or_else(|| camel_to_snake(&ident.to_string()));

    let primary_key = gen_primary_key(fields);
    let fill_create_default = gen_fill_create_default(fields);
    let fill_update_default = gen_fill_update_default(fields);

    quote! {
        impl std::default::Default for #ident {
            #[inline]
            fn default() -> Self {
                Self::new()
            }
        }

        impl sorm::model::Model for #ident {
            const TABLE: &'static str = #table;

            const COLUMNS: &'static [&'static str] = &[#(#fields_name),*];

            #primary_key

            #[inline]
            fn flush(&mut self) {
                self.__sorm_update = 0;
            }

            #[inline]
            fn is_changed(&self) -> bool {
                self.__sorm_update > 0
            }

            fn collect_filled(&self) -> Vec<(&'static str, &(dyn sorm::Param<'_> + Sync))> {
                if self.__sorm_set == 0 {
                    return Vec::new();
                }

                let mut cap = 0;
                let mut n = self.__sorm_set;
                while n > 0 {
                    cap += n & 1;
                    n >>= 1;
                }
                let mut fields = Vec::with_capacity(cap as _);
                #(
                    if (self.__sorm_set >> #seq) & 1 == 1 {
                        fields.push((#fields_name, &self.#fields_ident as &(dyn sorm::Param + Sync)));
                    }
                )*
                fields
            }

            fn collect_changed(&self) -> Vec<(&'static str, &(dyn sorm::Param<'_> + Sync))> {
                if self.__sorm_update == 0 {
                    return Vec::new();
                }

                let mut cap = 0;
                let mut n = self.__sorm_update;
                while n > 0 {
                    cap += n & 1;
                    n >>= 1;
                }
                let mut fields = Vec::with_capacity(cap as _);
                #(
                    if (self.__sorm_update >> #seq) & 1 == 1 {
                        fields.push((#fields_name, &self.#fields_ident as &(dyn sorm::Param + Sync)));
                    }
                )*
                fields
            }

            #fill_create_default

            #fill_update_default
        }
    }
}

fn gen_primary_key(fields: &Fields) -> proc_macro2::TokenStream {
    match fields.primary_key() {
        Some((field, increment)) => {
            let ty = &field.inner.ty;
            let ident = field.inner.ident.as_ref().unwrap();
            let name = ident.to_string();

            let setter = format_ident!("set_{}", name);
            let increment = if increment {
                let assert = quote_spanned! {ty.span()=>
                    struct _Assert where #ty: sorm::model::Int;
                };

                quote! {
                    const INCREMENT: bool = true;

                    #[inline]
                    fn set_increment_id(&mut self, id: u64) {
                        #assert
                        self.#setter(id as #ty)
                    }
                }
            } else {
                quote! {
                    const INCREMENT: bool = false;

                    #[inline]
                    fn set_increment_id(&mut self, _id: u64) {}
                }
            };
            quote! {
                type PrimaryKey = #ty;

                const PRIMARY_KEY: &'static str = #name;

                #increment

                #[inline]
                fn primary_key(&self) -> sorm::Result<&Self::PrimaryKey> {
                    self.#ident()
                }
            }
        }
        None => quote! {
            type PrimaryKey = sorm::model::HasNoPrimaryKey;

            const PRIMARY_KEY: &'static str = "";

            const INCREMENT: bool = false;

            #[inline]
            fn set_increment_id(&mut self, _id: u64) {}

            #[inline]
            fn primary_key(&self) -> sorm::Result<&Self::PrimaryKey> {
                Err(sorm::Error::NoPrimaryKey)
            }
        },
    }
}

fn gen_fill_create_default(fields: &Fields) -> proc_macro2::TokenStream {
    let mut gen = Vec::new();
    for field in &fields.0 {
        let attr = match field.attr {
            Some(ref attr) => attr,
            _ => continue,
        };

        if let Some(ref default) = attr.default {
            let seq = field.seq;
            let set = format_ident!("set_{}", field.inner.ident.as_ref().unwrap());
            let expr = &default.value;
            gen.push(quote! {
                if self.__sorm_set & (1 << #seq) == 0 {
                    self.#set(#expr);
                }
            });
        } else if let Some(ref create_time) = attr.create_time {
            let seq = field.seq;
            let set = format_ident!("set_{}", field.inner.ident.as_ref().unwrap());
            let expr = &create_time.value;
            gen.push(quote! {
                if self.__sorm_set & (1 << #seq) == 0 {
                    self.#set(#expr);
                }
            })
        } else if let Some(ref update_time) = attr.update_time {
            let seq = field.seq;
            let set = format_ident!("set_{}", field.inner.ident.as_ref().unwrap());
            let expr = &update_time.value;
            gen.push(quote! {
                if self.__sorm_set & (1 << #seq) == 0 {
                    self.#set(#expr);
                }
            });
        }
    }

    quote! {
        fn fill_create_default(&mut self) {
            #(#gen)*
        }
    }
}

fn gen_fill_update_default(fields: &Fields) -> proc_macro2::TokenStream {
    let mut gen = Vec::new();
    for field in &fields.0 {
        let attr = match field.attr {
            Some(ref attr) => attr,
            _ => continue,
        };
        if let Some(ref update_time) = attr.update_time {
            let seq = field.seq;
            let set = format_ident!("set_{}", field.inner.ident.as_ref().unwrap());
            let expr = &update_time.value;
            gen.push(quote! {
                if self.__sorm_update & (1 << #seq) == 0 {
                    self.#set(#expr);
                }
            });
        }
    }

    quote! {
        fn fill_update_default(&mut self) {
            #(#gen)*
        }
    }
}

struct Field<'a> {
    seq: usize,
    inner: &'a syn::Field,
    attr: Option<FieldAttr>,
    attr_index: Option<usize>,
}

struct Fields<'a>(Vec<Field<'a>>);

impl<'a> Fields<'a> {
    fn attr_index(&self) -> Vec<(usize, usize)> {
        let mut index = Vec::new();
        for field in &self.0 {
            if let Some(v) = field.attr_index {
                index.push((field.seq, v));
            }
        }

        index.sort_by(|a, b| {
            if a.0 == b.0 {
                b.1.cmp(&a.1)
            } else {
                a.0.cmp(&b.0)
            }
        });

        index
    }

    fn primary_key(&self) -> Option<(&Field, bool)> {
        for v in &self.0 {
            if let Some(ref attr) = v.attr {
                if let Some(ref increment) = attr.primary_key {
                    return Some((v, increment.value));
                }
            }
        }
        None
    }

    fn idents(&self) -> Vec<&Ident> {
        self.0
            .iter()
            .map(|v| v.inner.ident.as_ref().unwrap())
            .collect()
    }

    fn types(&self) -> Vec<&Type> {
        self.0.iter().map(|v| &v.inner.ty).collect()
    }

    fn names(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|v| v.inner.ident.as_ref().unwrap().to_string())
            .collect()
    }

    fn seq(&self) -> Vec<usize> {
        self.0.iter().map(|v| v.seq).collect()
    }
}

fn collect_field(item: &ItemStruct) -> syn::Result<Fields> {
    match item.fields {
        syn::Fields::Named(ref fields) if !fields.named.is_empty() => {
            let mut vec = Vec::with_capacity(fields.named.len());
            for (seq, field) in fields.named.iter().enumerate() {
                let mut attr = None;
                let mut attr_index = None;
                for (i, v) in field.attrs.iter().enumerate() {
                    if v.path().is_ident("sorm") {
                        match attr {
                            None => {
                                attr = Some(v.parse_args()?);
                                attr_index = Some(i);
                            }
                            _ => return Err(Error::new_spanned(v, "duplicate attribute")),
                        }
                    }
                }
                vec.push(Field {
                    seq,
                    inner: field,
                    attr,
                    attr_index,
                })
            }
            Ok(Fields(vec))
        }
        _ => Err(Error::new_spanned(&item.fields, "expected named fields")),
    }
}

fn modify_item(item: &mut ItemStruct, attr_index: Vec<(usize, usize)>) -> syn::Result<()> {
    match item.fields {
        syn::Fields::Named(ref mut fields) => {
            for field in &mut fields.named {
                field.vis = Visibility::Inherited;
            }

            for (seq, index) in attr_index {
                fields.named[seq].attrs.remove(index);
            }

            let ty: Type = parse_str(size_type(fields.named.len()))?;
            fields.named.push(parse_quote!(__sorm_set: #ty));
            fields.named.push(parse_quote!(__sorm_update: #ty));

            Ok(())
        }
        _ => Err(Error::new_spanned(&item.fields, "expected named fields")),
    }
}

fn size_type(number: usize) -> &'static str {
    match number {
        0..=8 => "u8",
        9..=16 => "u16",
        17..=32 => "u32",
        33..=64 => "u64",
        65..=128 => "u128",
        _ => panic!("fields number exceeds 128"),
    }
}

fn camel_to_snake(camel: &str) -> String {
    if camel.is_empty() {
        return String::new();
    }

    let mut snake = String::with_capacity(camel.len() + 4);
    let camel = camel.as_bytes();
    let mut offset = 0;
    for i in 1..=camel.len() {
        if i == camel.len() || camel[i].is_ascii_uppercase() {
            let segment = std::str::from_utf8(&camel[offset..i])
                .unwrap()
                .to_ascii_lowercase();
            snake.push_str(&segment);
            snake.push_str("_");
            offset = i;
        }
    }
    snake.pop();
    snake
}

fn parse_expr(expr: &str, span: Span) -> syn::Result<Expr> {
    match proc_macro2::TokenStream::from_str(expr) {
        Ok(expr) => parse2(
            expr.into_iter()
                .map(|mut v| {
                    v.set_span(span);
                    v
                })
                .collect(),
        ),
        Err(err) => Err(Error::new(span, err)),
    }
}
