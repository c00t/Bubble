## Bubble Tests

Because Bubble engine using plugin architecture, it's hard to test individual api or plugin's behavior. This crate create a simple test skeleton to test plugin's behavior.

### Preloaded Apis

- `ApiRegistryApi`: registry api, used to register api to engine
- `TaskApi`: task api, used to create task
- `PluginApi`: plugin api, used to load plugin

### Enable Features

- `bubble-log`: enable log feature
