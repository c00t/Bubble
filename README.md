# Monorepo Template

Use Josh-Proxy & Git Branchless as tools.

## Josh-Proxy

We use josh proxy for sub project management, you can run josh proxy with the following command:

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

then clone sub project using josh proxy:

```powershell
git clone http://localhost:8000/c00t/monorepo-template.git:workspace=project-beta.git project-beta
```

Be Careful! you should use `git joshsync origin HEAD` to sync your local repo,
josh proxy's cache and remote origin(when modify `workspace.josh`).
When using git branchless, there is also a `git sync` command,
its usage is to pull main branch then rebase your
local stack to main. It can't be used as `git push` etc. to push commits to remote.

## Git Branchless

Josh Proxy and Git Branchless can coexist,
only use git branchless for your local usage.
And use `git joshsync origin HEAD` when modify `workspace.josh` and
just `git push origin main` when push a normal commit.
