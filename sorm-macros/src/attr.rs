use proc_macro2::{Ident, Span};
use syn::parse::{Parse, ParseStream};
use syn::{parenthesized, Error, Expr, LitStr, Token};

use crate::parse_expr;

pub struct ContainerAttr {
    pub table: Option<String>,
    pub serialize: bool,
    pub deserialize: bool,
}

impl Parse for ContainerAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut table = None;
        let mut serialize = None;
        let mut deserialize = None;
        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "serialize" if serialize.is_none() => serialize = Some(true),
                "deserialize" if deserialize.is_none() => deserialize = Some(true),
                "table" if table.is_none() => {
                    input.parse::<Token![=]>()?;
                    table = Some(input.parse::<LitStr>()?.value());
                }
                "serialize" | "deserialize" | "table" => {
                    return Err(Error::new_spanned(ident, "duplicate attribute"));
                }
                _ => return Err(Error::new_spanned(ident, "unknown attribute")),
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            table,
            serialize: serialize.unwrap_or(false),
            deserialize: deserialize.unwrap_or(false),
        })
    }
}

pub struct WithSpan<T> {
    pub span: Span,
    pub value: T,
}

#[derive(Default)]
pub struct FieldAttr {
    pub primary_key: Option<WithSpan<bool>>,
    pub default: Option<WithSpan<Expr>>,
    pub create_time: Option<WithSpan<Expr>>,
    pub update_time: Option<WithSpan<Expr>>,
}

impl FieldAttr {
    fn sanity_check(&self) -> syn::Result<()> {
        if let Some(ref primary_key) = self.primary_key {
            if let Some(ref create_time) = self.create_time {
                return Err(Self::conflict_error(primary_key.span, create_time.span));
            }
            if let Some(ref update_time) = self.update_time {
                return Err(Self::conflict_error(primary_key.span, update_time.span));
            }
        }

        if let Some(ref default) = self.default {
            if let Some(ref create_time) = self.create_time {
                return Err(Self::conflict_error(default.span, create_time.span));
            }
            if let Some(ref update_time) = self.update_time {
                return Err(Self::conflict_error(default.span, update_time.span));
            }
        }

        if let Some(ref create_time) = self.create_time {
            if let Some(ref update_time) = self.update_time {
                return Err(Self::conflict_error(create_time.span, update_time.span));
            }
        }

        Ok(())
    }

    fn conflict_error(span1: Span, span2: Span) -> Error {
        let mut err = Error::new(span1, "conflict attribute");
        err.combine(Error::new(span2, "conflict attribute"));
        err
    }
}

impl Parse for FieldAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut attr = FieldAttr::default();
        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "primary_key" if attr.primary_key.is_none() => {
                    let mut increment = false;
                    if !input.is_empty() && !input.peek(Token![,]) {
                        let content;
                        parenthesized!(content in input);
                        let i = content.parse::<Ident>()?;
                        if i != "increment" {
                            return Err(Error::new_spanned(i, "unknown attribute"));
                        }
                        if !content.is_empty() {
                            return Err(Error::new(content.span(), "unexpected token"));
                        }
                        increment = true
                    }

                    attr.primary_key = Some(WithSpan {
                        span: ident.span(),
                        value: increment,
                    });
                }
                "default" if attr.default.is_none() => {
                    let expr = if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        let lit: LitStr = input.parse()?;
                        parse_expr(&lit.value(), lit.span())?
                    } else {
                        parse_expr("std::default::Default::default()", ident.span())?
                    };
                    attr.default = Some(WithSpan {
                        span: ident.span(),
                        value: expr,
                    })
                }
                "create_time" if attr.create_time.is_none() => {
                    let expr = if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        let lit: LitStr = input.parse()?;
                        parse_expr(&lit.value(), lit.span())?
                    } else {
                        parse_expr("crate::current_timestamp()", ident.span())?
                    };
                    attr.create_time = Some(WithSpan {
                        span: ident.span(),
                        value: expr,
                    })
                }
                "update_time" if attr.update_time.is_none() => {
                    let expr = if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        let lit: LitStr = input.parse()?;
                        parse_expr(&lit.value(), lit.span())?
                    } else {
                        parse_expr("crate::current_timestamp()", ident.span())?
                    };
                    attr.update_time = Some(WithSpan {
                        span: ident.span(),
                        value: expr,
                    })
                }
                "primary_key" | "default" | "create_time" | "update_time" => {
                    return Err(Error::new_spanned(ident, "duplicate attribute"));
                }
                _ => return Err(Error::new_spanned(ident, "unknown attribute")),
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        attr.sanity_check()?;
        Ok(attr)
    }
}
