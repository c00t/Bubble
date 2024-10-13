# bubble-tasks

A thread-per-core Rust runtime with IOCP/io_uring/polling, fork from [compio](https://github.com/compio-rs/compio/). With dynamic shared library support, you can setup it with a simple `init` function.

The plugin can be used as a dynamic plugin, but it is normally used as a static plugin.

## Quick Start

Suppose you are writing a shared plugin of `Bubble`, you can use the following code to initialize the runtime, and use async funcions:

```rust

```

## Credits

This library is a fork of [compio-runtime](https://github.com/compio-rs/compio/compio-runtime) and [compio-dispatcher](https://github.com/compio-rs/compio/compio-dispatcher).