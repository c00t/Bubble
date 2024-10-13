# bubble-alloc

For game engine, memory management is important. It's seldom to use system allocator directly. You can use custom allocator in rustlang easily. But Bubble provides most of its functionality (in debug mode) using dynamic plugins, some plugins can only be used in dynamic plugin mode. It's not idiomatic in rustlang. To be specific, when you compile your rust plugin with standard library, you should use `#[global_allocator]` to override the global allocator, but it will result that each plugin has its own global allocator (with its own static or thread local data), it's dangerous to pass data between plugins.

So for a dynamic plugin system, allocator should be a plugin too. In c++ or c, you can use `alloc` `free` function pointers from dynamic library to alloc memory from a single allocator. While in rust, you need to use `#[global_allocator]` to tell compiler to use your custom allocator, `allocator_api` is not enough to track memory usage from other rust plugins which don't use it.

## Use your own allocator as global allocator

To use your own allocator as global allocator, you need to create a library like `bubble-alloc` and compiled to a `cdylib`, named it `bubble-alloc.so` or `bubble-alloc.dll`. You can also specify the name of the dynamic library to be linked by specifying the `BUBBLE_ALLOC` environment variable at compile time.