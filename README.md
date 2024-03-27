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

Use Josh-Proxy & Git Branchless as tools.

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
