---
module: agent-desktop
date: 2026-11-01
problem_type: runtime_error
tags: [macos, permissions, sandbox, seatbelt, ps, app-inventory, headless, press]
---

# A macOS app inventory that always fails is not a settled empty inventory

## The symptom

Inside a macOS `sandbox-exec` (Seatbelt) profile that denies execution of setuid binaries, every command that resolves an application through the process inventory fails the same way, even though reads work:

- `list-windows` returns real windows with `pid`, `process_instance` and `bounds`.
- `snapshot --window-id` and `snapshot --app <name> --surface menubar` return a complete tree.
- `list-apps`, and every action that resolves an app by name (`press --app`, `click --app`, `press --window-id`), return `TIMEOUT` with `"kind": "application_inventory_unstable"` and `last_failure_details.failures[0]` = `{"code":"INTERNAL","message":"ps: Operation not permitted (os error 1)","source":"ps"}`.

The blocked binary is `/bin/ps`. It is shipped `-rws` (setuid root), and a Seatbelt profile that denies process execution of setuid binaries turns the spawn into `Operation not permitted` (EPERM) before `ps` runs. Non-setuid tools on the same PATH (`pgrep`, `ls`, `whoami`) run normally, which is the tell: the failure is the setuid bit, not the sandbox denying the process tree in general.

## Why it fails as a timeout, not a clean error

`capture_scoped_apps` in `crates/macos/src/system/app/app_inventory.rs` asks two sources for the app list: the AppKit/NSWorkspace source and the `ps` source. `complete_apps_from_sources` treats both as required. When `ps` fails, `required_sources_failed` builds an `AppUnresponsive` error marked `retryable`, and the stabilization loop in `stabilize_apps_until` keeps retrying it until the deadline, then reports `application_inventory_unstable`. A ps failure that can never succeed is therefore reported as an unstable inventory, and every retry burns the whole deadline.

## The path that still works

Anything that targets an already-known element does not go through app inventory:

- `list-windows` gives window ids without the `ps` source.
- `snapshot --window-id <id>` reads the tree.
- `click @ref`, `get @ref`, `set-value`, `scroll`, and the rest operate on a ref that carries its own retained element identity, so they need no app name.

So read the tree first, then drive it by ref:

```
agent-desktop list-windows
agent-desktop snapshot --window-id w-1234 -i --compact
agent-desktop click @s....:e12
```

Run these from an environment whose state root is writable, because the default `~/.agent-desktop` is outside a workspace-write profile and returns `INTERNAL: Operation not permitted` on its own. Point `AGENT_DESKTOP_HOME` at an absolute, real (not symlinked), user-owned directory with mode `700` inside the writable area.

## What this does not fix

`press --app` and `press --window-id` both resolve the owning app through `resolve_app`/`resolve_app_owning`, which call the app inventory, so neither works under a `ps`-denying profile. There is no environment switch that removes the `ps` source; the binary has a fixed `/bin/ps` dependency and `list-apps --help` exposes only `--timeout-ms`, no source selection. The setuid-execution denial must be lifted (a sandbox that permits setuid binaries, or `danger-full-access`) for name- and app-targeted commands to work. Element-targeted read and click do not need it.

Evidence: reproduced with `agent-desktop 1.0.2` under a workspace-write Seatbelt profile; `list-apps`/`press --app`/`press --window-id` all returned the `ps` `Operation not permitted` failure, while `list-windows` plus `snapshot --window-id` plus `click @ref` completed against Finder.
