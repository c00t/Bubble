use quote::format_ident;
use syn::{
    parse::{Parse, ParseStream},
    ExprTuple, Ident,
};

// Structure to hold version information
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

// Parse implementation for Version from tuple
impl Parse for Version {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let version: ExprTuple = input.parse()?;

        if version.elems.len() != 3 {
            return Err(syn::Error::new_spanned(
                version,
                "Expected version in the format (major, minor, patch)",
            ));
        }

        let parse_int = |expr: &syn::Expr| -> syn::Result<u64> {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(lit_int),
                ..
            }) = expr
            {
                lit_int.base10_parse()
            } else {
                Err(syn::Error::new_spanned(
                    expr,
                    "Expected integer literal for version number",
                ))
            }
        };

        let version = (
            parse_int(&version.elems[0])?,
            parse_int(&version.elems[1])?,
            parse_int(&version.elems[2])?,
        );

        Ok(Version {
            major: version.0,
            minor: version.1,
            patch: version.2,
        })
    }
}

pub fn snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.char_indices() {
        if i > 0 && ch.is_uppercase() {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap());
    }
    result
}

/// Add Dyn to the ident, eg. "MyStruct" -> "DynMyStruct"
pub fn dyn_ident(ident: &Ident) -> Ident {
    format_ident!("Dyn{}", ident)
}
