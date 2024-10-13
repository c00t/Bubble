mod singleton_derive;

#[proc_macro_derive(Singleton)]
pub fn derive_singleton(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    singleton_derive::derive_singleton(item)
}
