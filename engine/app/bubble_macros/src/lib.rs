#![recursion_limit = "128"]
#![feature(proc_macro_span)]
use proc_macro::TokenStream;

mod api;
mod interface;
mod singleton_derive;

#[proc_macro_derive(Singleton)]
pub fn derive_singleton(item: TokenStream) -> TokenStream {
    singleton_derive::derive_singleton(item)
}

#[proc_macro_attribute]
pub fn define_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    api::define_api(attr, item)
}

#[proc_macro_attribute]
pub fn define_api_with_id(attr: TokenStream, item: TokenStream) -> TokenStream {
    api::define_api_with_id(attr, item)
}

#[proc_macro_attribute]
pub fn define_interface(attr: TokenStream, item: TokenStream) -> TokenStream {
    interface::define_interface(attr, item)
}

#[proc_macro_attribute]
pub fn define_interface_with_id(attr: TokenStream, item: TokenStream) -> TokenStream {
    interface::define_interface_with_id(attr, item)
}
