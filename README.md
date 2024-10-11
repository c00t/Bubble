# Bubble

Bubble is a game engine written in Rust. It is designed to be a complex engine,
which use many features I want to try in Rust&game engine. It can't be used in production.

Bubble will be developed with a sample galgame named "Gigolo".
So some features will be implemented first,
such as ragdoll, dynamic simulation(fluid, cloth, hair, rigid body),
advanced non-photo-realistic rendering...

Also, different from other game engines,
Bubble will regard both mobile and desktop as tier-0 target platforms.

## Features

- [ ] ECS
- [ ] Dynamic Simulation
- [ ] Advanced Rendering

## Monorepo Usage Guide

Use Josh-Proxy, Git Branchless & Buck2 as tools.

### Josh-Proxy

We use josh proxy for sub project management,
you can run josh proxy with the following command:

```powershell
docker run `
--name josh-proxy `
--detach `
--publish 8000:8000 `
--env JOSH_REMOTE=https://github.com `
--env HTTP_PROXY=host.docker.internal:7890 `
--env HTTPS_PROXY=host.docker.internal:7890 `
--env http_proxy=host.docker.internal:7890 `
--env https_proxy=host.docker.internal:7890 `
--env=JOSH_EXTRA_OPTS=--require-auth `
--volume josh-volume:/data/git `
--restart always `
joshproject/josh-proxy:latest | Out-Null `
```

Then clone sub project using josh proxy:

```powershell
git clone `
http://localhost:8000/c00t/monorepo-template.git:workspace=project-beta.git `
project-beta `
```

Be Careful! You should use `git joshsync origin HEAD` to sync your local repo,
josh proxy's cache and remote origin(when modify `workspace.josh`).
When using git branchless, there is also a `git sync` command,
its usage is to pull main branch then rebase your
local stack to main. It can't be used as `git push` etc. to push commits to remote.

### Git Branchless

Josh Proxy and Git Branchless can coexist,
only use git branchless for your local usage.
And use `git joshsync origin HEAD` when modify `workspace.josh` and
just `git push origin main` when push a normal commit.

### Buck2

Buck2 is a build system maintained by Meta, we use it to build our project. We use the latest version of [Buck2](https://buck2.build/docs/about/getting_started/).

Use following command to install buck2 and some useful tools:

```powershell
# Install buck2
cargo +nightly-YYYY-MM-DD install --git https://github.com/facebook/buck2.git buck2
# Install rust-project to support rust-analyzer, instead of manually sync Cargo.toml
cargo +nightly-YYYY-MM-DD install --git https://github.com/facebook/buck2.git rust-project
# Used for rust dependency management
cargo +nightly-YYYY-MM-DD install --locked --git https://github.com/facebookincubator/reindeer reindeer
```

#### Prelude

We make some customized rules, so we use a [self-maintained prelude](https://github.com/c00t/buck2-prelude/tree/main) to replace the official one.
It have been added to the `prelude` directory as submodule. We also add official prelude as submodule for debugging.
You should always use the self-maintained prelude when possible.

#### Reindeer

We rarely use vendored libraries. They should be managed in separate git repositories and linked through Cargo's git dependency feature.
Therefore, after adding the dependency to `third-party/rust/Cargo.toml`, 
you can generate the `BUCK` file using the following command.

```powershell
# At the root of the project
reindeer --third-party-dir .\third-party\rust\ buckify
```

For specific configurations, please refer to the documentation [here](https://github.com/facebookincubator/reindeer/blob/main/docs/MANUAL.md).

#### IDE & RA

You can use `rust-project` to generate the `rust-project.json` file for rust-analyzer. But I can't get it to work on my Windows machine. So I'm currently using `cargo add deps...` to add dependencies in `Cargo.toml` of both the reindeer and the library.