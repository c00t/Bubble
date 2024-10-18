use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::Parse, parse::ParseStream, parse_macro_input, DeriveInput, ExprTuple, Path, Token,
};

struct ApiAttr {
    version: (u64, u64, u64),
    api_trait_path: Path,
}

impl Parse for ApiAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let version: ExprTuple = input.parse()?;
        input.parse::<Token![,]>()?;
        let api_trait_path: Path = input.parse()?;

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

        Ok(ApiAttr {
            version,
            api_trait_path,
        })
    }
}

fn snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.char_indices() {
        if i > 0 && ch.is_uppercase() {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap());
    }
    result
}

pub fn define_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let ApiAttr {
        version: (version_major, version_minor, version_patch),
        api_trait_path,
    } = parse_macro_input!(attr as ApiAttr);

    let struct_name = &input.ident;
    let api_trait_last_path = &api_trait_path.segments.last().unwrap().ident;
    let get_api_fn_name = format_ident!("get_{}", snake_case(&api_trait_last_path.to_string()));

    let expanded = quote! {
        // #[repr(C)]
        #[make_trait_castable(Api, #api_trait_last_path)]
        #input

        unique_id! {
            #[UniqueTypeIdVersion((#version_major, #version_minor, #version_patch))]
            dyn #api_trait_path
        }

        impl_api!(#struct_name, #api_trait_last_path, (#version_major, #version_minor, #version_patch));

        pub fn #get_api_fn_name() -> ApiHandle<dyn #api_trait_last_path> {
            #struct_name::new()
        }
    };

    TokenStream::from(expanded)
}
