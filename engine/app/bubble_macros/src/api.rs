use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    DeriveInput, ExprTuple, FnArg, Ident, ItemTrait, LitBool, Path, Token, TraitItem,
};

use crate::shared::{dyn_ident, snake_case, Version};

struct ApiDefineAttr {
    api_trait_paths: Vec<Path>,
    skip_castable: bool,
}

impl Parse for ApiDefineAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut api_trait_paths = Vec::new();
        let mut skip_castable = false;

        while !input.is_empty() {
            if input.peek(Ident) && input.peek2(Token![=]) {
                let ident: Ident = input.parse()?;
                let _eq_token: Token![=] = input.parse()?;
                match ident.to_string().as_str() {
                    "skip_castable" => {
                        skip_castable = input.parse::<LitBool>()?.value;
                    }
                    _ => return Err(syn::Error::new(ident.span(), "Unknown attribute key")),
                }
            } else {
                api_trait_paths.push(input.parse()?);
            }

            if !input.is_empty() {
                let _: Token![,] = input.parse()?;
            }
        }

        Ok(ApiDefineAttr {
            api_trait_paths,
            skip_castable,
        })
    }
}

pub fn define_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let ApiDefineAttr {
        api_trait_paths,
        skip_castable,
    } = parse_macro_input!(attr as ApiDefineAttr);

    assert_eq!(api_trait_paths.len(), 1, "Only 1 path permitted");

    let struct_name = &input.ident;

    // Get all trait segments for a single make_trait_castable attribute
    let trait_segments: Vec<_> = api_trait_paths
        .iter()
        .map(|path| {
            path.segments
                .last()
                .expect("API trait path should have at least one segment")
                .ident
                .clone()
        })
        .collect();

    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Generate impl_api! calls for all trait paths
    let impl_api_calls = api_trait_paths.iter().map(|path| {
        let last_segment = path
            .segments
            .last()
            .expect("API trait path should have at least one segment");
        quote! {
            impl #impl_generics self::Api for #struct_name #ty_generics #where_clause {
                #[inline]
                fn name(&self) -> &'static str {
                    <dyn #last_segment as ApiConstant>::NAME
                }
                #[inline]
                fn version(&self) -> self::Version {
                    <dyn #last_segment as ApiConstant>::VERSION
                }
            }
        }
    });

    // Conditionally include the make_trait_castable attribute
    let trait_castable = if !skip_castable {
        quote! {
            #[make_trait_castable(Api, #(#trait_segments),*)]
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #trait_castable
        #input

        #(#impl_api_calls)*
    };

    TokenStream::from(expanded)
}

// Structure to hold both version and path
struct ApiDeclarationAttr {
    version: Version,
    path: syn::Path,
}

// Parse implementation for ApiDeclaration
impl Parse for ApiDeclarationAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let version = input.parse()?;
        input.parse::<Token![,]>()?;
        let path = input.parse()?;

        Ok(ApiDeclarationAttr { version, path })
    }
}

pub fn declare_api(args: TokenStream, item: TokenStream) -> TokenStream {
    let ApiDeclarationAttr { version, path } = parse_macro_input!(args as ApiDeclarationAttr);
    let input = parse_macro_input!(item as ItemTrait);
    let trait_ident = &input.ident;
    let major = version.major;
    let minor = version.minor;
    let patch = version.patch;

    let constant_mod_ident = format_ident!(
        "{}",
        format!("{}", snake_case(&format!("{}Constants", trait_ident)))
    );
    let dyn_type_ident = dyn_ident(trait_ident);

    // Generate implementation methods for each trait method
    let methods = input
        .items
        .iter()
        .filter_map(|item| {
            if let syn::TraitItem::Fn(method) = item {
                let sig = &method.sig;
                let name = &sig.ident;
                let inputs = &sig.inputs;

                // Extract argument names for forwarding
                let args = inputs
                    .iter()
                    .skip(1)
                    .filter_map(|arg| {
                        if let syn::FnArg::Typed(pat_type) = arg {
                            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                                Some(&pat_ident.ident)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                let impl_method = quote! {
                    #sig {
                        (**self).#name(#(#args),*)
                    }
                };
                Some(impl_method)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let api_trait_last_path = &path
        .segments
        .last()
        .expect("API trait path should have at least one segment")
        .ident;
    let local_api_handle = format_ident!("{}Handle", api_trait_last_path);

    let expanded = quote! {
        // pub(crate) mod #constant_mod_ident {
        //     use super::Version;
        //     pub const NAME: &'static str = ::std::stringify!(#path);
        //     pub const VERSION: self::Version = self::Version::new(
        //         #major,
        //         #minor,
        //         #patch
        //     );
        // }

        impl ApiConstant for #dyn_type_ident {
            const NAME: &'static str = <#dyn_type_ident as FixedTypeId>::TYPE_NAME;

            const VERSION: self::Version = self::Version::new(
                <#dyn_type_ident as FixedTypeId>::TYPE_VERSION.major,
                <#dyn_type_ident as FixedTypeId>::TYPE_VERSION.minor,
                <#dyn_type_ident as FixedTypeId>::TYPE_VERSION.patch,
            );
        }

        pub type #dyn_type_ident = dyn #trait_ident;

        fixed_type_id_without_version_hash! {
            #[FixedTypeIdVersion((#major, #minor, #patch))]
            dyn #path
        }

        impl<T: #trait_ident + ?Sized> #trait_ident for Box<T> {
            #(#methods)*
        }

        pub struct #local_api_handle<'local> {
            api: LocalApiHandle<'local, #dyn_type_ident>
        }

        impl<'local> From<LocalApiHandle<'local, #dyn_type_ident>> for #local_api_handle<'local> {
            fn from(api: LocalApiHandle<'local, #dyn_type_ident>) -> Self {
                Self { api }
            }
        }

        #input
    };

    TokenStream::from(expanded)
}
