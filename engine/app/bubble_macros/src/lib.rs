#![recursion_limit = "128"]
#![feature(proc_macro_span)]
use proc_macro::TokenStream;

mod api;
mod interface;
mod plugin_export;
mod shared;

#[proc_macro_attribute]
pub fn plugin_export(attr: TokenStream, input: TokenStream) -> TokenStream {
    plugin_export::plugin_export(attr, input)
}

/// Defines an API implementation struct and generates necessary boilerplate code.
///
/// This macro takes a trait path as an argument and generates:
/// - Implementation of the Api trait
/// - You should implement the trait yourself
/// - Trait casting infrastructure
/// - Registration helper functions
///
/// Example:
/// ```ignore,no_compile
/// #[define_api(my_crate::MyApi)]
/// pub struct MyApiImpl {
///     // fields
/// }
/// ```
#[proc_macro_attribute]
pub fn define_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    api::define_api(attr, item)
}

/// Defines an interface implementation struct and generates necessary boilerplate code.
///
/// This macro takes a trait path as an argument and generates:
/// - Implementation of the Interface trait
/// - You should implement the trait yourself
/// - Trait casting infrastructure
/// - Registration helper functions
///
/// Example:
/// ```ignore,no_compile
/// #[define_interface(my_crate::MyInterface)]
/// pub(crate) struct MyInterfaceImpl {
///     // fields
/// }
/// ```
#[proc_macro_attribute]
pub fn define_interface(attr: TokenStream, item: TokenStream) -> TokenStream {
    interface::define_interface(attr, item)
}

/// Declares an API trait with version information and generates necessary boilerplate code.
///
/// This macro takes a version tuple and path(it will be used as the identifier name of the api trait) as arguments, and generates:
/// - A module containing version constants (e.g. `MyApi` -> `my_api_constant`)
/// - Implementation of ApiConstant for the dynamic trait type
/// - Unique ID generation
/// - Box<T> delegation implementation
///
/// Example:
/// ```ignore,no_compile
/// // make sure the path is a unique name
/// #[declare_api((1,0,0), my_crate::MyApi)]
/// pub trait MyApi: Api {
///     fn my_method(&self);
/// }
/// ```
#[proc_macro_attribute]
pub fn declare_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    api::declare_api(attr, item)
}

/// Declares an interface trait with version information and generates necessary boilerplate code.
///
/// This macro takes a version tuple and path(it will be used as the identifier name of the interface trait) as arguments, and generates:
/// - A module containing version constants (e.g. `MyInterface` -> `my_interface_constant`)
/// - Implementation of InterfaceConstant for the dynamic trait type
/// - Unique ID generation
/// - Box<T> delegation implementation
///
/// Example:
/// ```ignore,no_compile
/// // make sure the path is a unique name
/// #[declare_interface((1,0,0), my_crate::MyInterface)]
/// pub trait MyInterface: Interface {
///     fn my_method(&self);
/// }
/// ```
#[proc_macro_attribute]
pub fn declare_interface(attr: TokenStream, item: TokenStream) -> TokenStream {
    interface::declare_interface(attr, item)
}
