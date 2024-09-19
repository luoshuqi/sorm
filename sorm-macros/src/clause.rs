use std::str::from_utf8;

use proc_macro2::{Ident, Span};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Error, Expr, LitStr, Token};

use crate::parse_expr;

pub struct Args {
    clause: LitStr,
    param_ident: Option<Ident>,
    sql_ident: Option<Ident>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let clause: LitStr = input.parse()?;
        let mut args = Args {
            clause,
            param_ident: None,
            sql_ident: None,
        };

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            args.param_ident = input.parse()?;

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
                args.sql_ident = input.parse()?;
            }
        }
        Ok(args)
    }
}

pub fn expand(args: Args) -> syn::Result<proc_macro2::TokenStream> {
    let clause = args.clause.value();
    let clause = clause.trim();
    if clause.is_empty() {
        if let Some(ref ident) = args.param_ident {
            return Err(Error::new_spanned(ident, "unexpected token"));
        }
        return Ok(quote!(("", &[] as &[&(dyn sorm::Param + Sync)])));
    }

    let parts = split_clause(clause.as_bytes(), args.clause.span())?;
    if parts.len() == 1 && parts[0].is_ok() {
        let (sql, param) = parse_clause(clause.as_bytes(), args.clause.span())?;
        if let Some(ref ident) = args.sql_ident {
            return Err(Error::new_spanned(ident, "unexpected token"));
        }
        return match args.param_ident {
            Some(ref ident) => Ok(quote! {
                {
                    #[allow(unused_imports)]
                    use sorm::Lend;
                    #ident = vec![#((#param).lend() as &(dyn sorm::Param + Sync)),*];
                    (#sql, &*#ident)
                }
            }),
            None => Ok(quote! {
                {
                    #[allow(unused_imports)]
                    use sorm::Lend;
                    (#sql, &[#((#param).lend() as &(dyn sorm::Param + Sync)),*] as &[&(dyn sorm::Param + Sync)])
                }
            }),
        };
    }

    let mut sql_cap = 0;
    let mut sql_gen = Vec::new();
    let mut params_cap = 0;
    let mut params_gen = Vec::new();
    for part in parts {
        match part {
            Ok(part) => {
                let (sql, param) = parse_clause(part, args.clause.span())?;
                sql_cap += sql.len();
                params_cap += param.len();
                sql_gen.push(quote! {
                    __sorm_sql.push_str(#sql);
                });
                params_gen.push(quote! {
                     __sorm_params.extend_from_slice(&[#((#param).lend() as &(dyn sorm::Param + Sync)),*]);
                })
            }
            Err(part) => {
                let expr = parse_expr(from_utf8(part).unwrap(), args.clause.span())?;
                sql_gen.push(quote! {
                    let param = (#expr).lend();
                    __sorm_sql.reserve(param.len() * 2);
                    for v in param {
                        __sorm_sql.push_str("?,");
                    }
                    if param.len() > 0 {
                        __sorm_sql.pop();
                    }
                });
                params_gen.push(quote! {
                    let __sorm_param = (#expr).lend();
                    __sorm_params.reserve(__sorm_param.len());
                    for v in __sorm_param {
                        __sorm_params.push(v);
                    }
                });
            }
        }
    }

    let sql = match args.sql_ident {
        Some(ref ident) => quote! {
            {
                let mut __sorm_sql = String::with_capacity(#sql_cap);
                #(#sql_gen)*
                #ident = __sorm_sql;
                &*#ident
            }
        },
        None => quote! {
            &*{
                let mut __sorm_sql = String::with_capacity(#sql_cap);
                #(#sql_gen)*
                __sorm_sql
            }
        },
    };

    match args.param_ident {
        Some(ref ident) => Ok(quote! {
            {
                #[allow(unused_imports)]
                use sorm::Lend;
                let mut __sorm_params = Vec::<&(dyn sorm::Param + Sync)>::with_capacity(#params_cap);
                #(#params_gen)*
                #ident = __sorm_params;
                (#sql, &*#ident)
            }
        }),
        None => Ok(quote! {
            {
                #[allow(unused_imports)]
                use sorm::Lend;
                (#sql, &*{
                    let mut __sorm_params = Vec::<&(dyn sorm::Param + Sync)>::with_capacity(#params_cap);
                    #(#params_gen)*
                    __sorm_params
                })
            }
        }),
    }
}

fn split_clause<'a>(clause: &[u8], span: Span) -> syn::Result<Vec<Result<&[u8], &[u8]>>> {
    let mut parts = Vec::with_capacity(1);
    let mut brace_count = 0;
    let mut offset = 0;
    let mut i = 0;
    while i < clause.len() {
        match clause[i] {
            b'{' => brace_count += 1,
            b'#' if brace_count & 1 == 1 => {
                if i - 1 > offset {
                    parts.push(Ok(&clause[offset..i - 1]));
                }
                let mut right = None;
                let mut j = i + 1;
                while j < clause.len() {
                    if clause[j] == b'}' {
                        if j + 1 < clause.len() && clause[j + 1] == b'}' {
                            j += 1
                        } else {
                            right = Some(j);
                            break;
                        }
                    }
                    j += 1
                }

                match right {
                    Some(right) if right > i + 1 => {
                        parts.push(Err(&clause[i + 1..right]));
                        brace_count = 0;
                        offset = right + 1;
                        i = right;
                    }
                    Some(end) => {
                        return Err(Error::new(
                            span,
                            format!("unexpected `}}` at position {}", end),
                        ));
                    }
                    None => {
                        return Err(Error::new(
                            span,
                            format!("unclosed `{{` at position {}", i - 1),
                        ));
                    }
                }
            }
            _ => brace_count = 0,
        }
        i += 1
    }

    if offset < clause.len() {
        parts.push(Ok(&clause[offset..]));
    }
    Ok(parts)
}

fn parse_clause(clause: &[u8], span: Span) -> syn::Result<(String, Vec<Expr>)> {
    macro_rules! unexpected {
        ($s:expr, $pos:expr) => {{
            return Err(Error::new(
                span,
                format!("unexpected `{}` at position {}", $s, $pos),
            ));
        }};
    }

    let mut sql = Vec::with_capacity(clause.len());
    let mut params = Vec::<Expr>::new();
    let mut left = None;
    let mut i = 0;
    while i < clause.len() {
        match clause[i] {
            b'{' if left.is_some() || i == clause.len() - 1 => unexpected!("{", i),
            b'{' if clause[i + 1] == b'{' => {
                sql.push(b'{');
                i += 1;
            }
            b'{' => left = Some(i),
            b'}' if i < clause.len() - 1 && clause[i + 1] == b'}' => {
                sql.push(b'}');
                i += 1;
            }
            b'}' if left.is_none() => unexpected!("}", i),
            b'}' => {
                let expr = &clause[left.unwrap() + 1..i];
                if expr.is_empty() {
                    unexpected!("}", i);
                }
                let expr = from_utf8(expr).unwrap();
                params.push(parse_expr(expr, span)?);
                sql.push(b'?');
                left = None;
            }
            _ if left.is_none() => sql.push(clause[i]),
            _ => (),
        }
        i += 1;
    }

    if let Some(left) = left {
        return Err(Error::new(
            span,
            format!("unclosed `{{` at position {}", left),
        ));
    }

    Ok((String::from_utf8(sql).unwrap(), params))
}
