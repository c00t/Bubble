#![recursion_limit = "128"]
use proc_macro::TokenStream;

mod api;
mod singleton_derive;

#[proc_macro_derive(Singleton)]
pub fn derive_singleton(item: TokenStream) -> TokenStream {
    singleton_derive::derive_singleton(item)
}

#[proc_macro_attribute]
pub fn define_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    api::define_api(attr, item)
}
