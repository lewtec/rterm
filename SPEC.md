# rterm specification

Status: locked decisions (2026-08-19).  
Reader: anyone who implements rterm.  
After you read this file you can implement v1 without inventing product rules.

This file records decisions. It is not a tutorial.

## Terminology

Use the approved term. Do not invent a synonym.

| Concept | Approved term | Do not use |
| --- | --- | --- |
| Saved attach target | place | session (alone), connection, bookmark, target, row, ref |
| UI control for one place | tab | row (in v1) |
| Machine a place attaches to | node | server, box |
| Field that selects herdr, tmux, or screen | backend | multiplexer type, kind |
| Code that turns a place into an argv | driver | adapter, plugin |
| Durable list of places | catalog | config (alone), database |
| Visible terminal surface | pane | terminal (alone), PTY view |
| Start or reuse the remote session | attach | connect (except user-facing "not connected"), reconnect as a separate verb |
| User-visible name of a place | label | title, alias, nickname |
| Centered icon and error on the pane | overlay | modal, sheet, alert (for mux/host failures) |
| SSH password, host-key, and OTP text | prompt | dialog (for those bytes) |

Attach is the only verb that starts a place. The menu item Reconnect runs attach again.

## Product

rterm is a desktop app for macOS and Linux. It supervises detachable remote sessions. It is not a general local-shell terminal.

The remote session is the source of truth. The local process is disposable.

Destinations must already run a detachable session tool: herdr, tmux, or screen.

v1 runs on macOS 14 and on Linux. Windows is out of scope. A later release still follows these rules until you change this file.

## Place

A place is:

- user (optional; empty means the ssh login)
- host (the first endpoint; empty, a loopback name, or this machine's hostname means local; otherwise used verbatim, and it may contain `@`)
- backend: `herdr`, `tmux`, or `screen`
- session name: required for `tmux` and `screen`; absent for `herdr`
- label: optional; replaces the hostname part of the tab name
- fingerprint: optional SSH host key fingerprint (`SHA256:…`)
- routes: zero or more `kind`, `host`, optional `port` records

herdr has no session name in rterm.

An empty host, a loopback host (`localhost`, `127.0.0.1`, `::1`), or this machine's hostname is a local place. Attach runs the mux on this machine as the login user. Do not use SSH. Persist no host for a loopback name. Keep this machine's hostname in the catalog. If user is empty, use the login name. Default label is `user@localhost:herdr`.

The routes are the source of truth for how to reach a node; the endpoint is only the first route. If the user leaves the label empty, the hostname is the route in use (the last one that worked, else the first saved route, else the endpoint), and the default name is:

- remote herdr: `user@hostname:herdr`
- remote tmux or screen: `user@hostname:tmux(foo)` or `user@hostname:screen(foo)`
- local herdr: `user@localhost:herdr` (user defaults to the Mac login name)
- local tmux or screen: `user@localhost:tmux(foo)`

A label replaces only the hostname: label `work` on `user@host:herdr` shows `user@work:herdr`.

Parse form: `user@host` or `user@host:backend(args)`. A missing `:backend` means `herdr`. A missing host means local. The login is the first `@` component and cannot contain `@`; the host may contain one (ShellHub addresses are `namespace.device@cloud.shellhub.io`), so parse at the first `@`.

The user and the endpoint are separate fields. Never move an `@` from the endpoint into the user: `lucao.whiterun@cloud.shellhub.io` is a whole address, and with user `lucasew` rterm runs `ssh lucasew@lucao.whiterun@cloud.shellhub.io` (`ssh` itself splits at the last `@`).

Two places may share a host. The stable identity of a place is a generated `id` in the catalog. The label is not the id. The key of a tab is the login, the fingerprint and the access: places with the same login and fingerprint are tabs of one node and differ by backend and session. Another login on the same machine is another tab.

A tab opens one access to one node. The node's SSH identity is the pinned host key fingerprint, not a hostname. `host` is the name the user knows; `routes` are the ways to reach it, and they belong to the node, so every tab of it keeps the same list. One place may keep several routes and use any of them.

## Routes

A route is `kind`, `host`, and an optional `port`.

| kind | source |
| --- | --- |
| `direct` | a saved name or IP, or the place host |
| `mdns` | `<short>.local` through the system mDNS resolver |
| `tailscale` | a MagicDNS name or IP from `tailscale status --json` |

Try routes in this order, dropping duplicates: the route marked last, routes saved in the catalog (the endpoint is one of them), then mDNS and Tailscale peers discovered from the hostname. Ask the Tailscale daemon through the `tailscale` binary. A missing binary or stopped daemon only removes Tailscale routes.

The connection assistant probes each route before attaching:

```text
ssh -o BatchMode=yes -o ConnectTimeout=2 -o ConnectionAttempts=1 \
    -o PreferredAuthentications=none -o StrictHostKeyChecking=accept-new \
    -o GlobalKnownHostsFile=/dev/null -o UserKnownHostsFile=<temp> \
    -o HostKeyAlias=rterm-<place-id> -o HostName=<route-host> [-p <port>] \
    <user>@<place-host> true
```

Host key exchange happens before authentication, so a route that presents a host key is reachable even when the server refuses the `none` method. A password or OTP still belongs to the pane.

When the place has no fingerprint, the first verified route teaches it. Persist that fingerprint before the attach starts. A route that presents another key is another node: never attach through it. If every route fails, fail the attach with the attempts.

A place remembers the route that worked last (`last = true` on its route). The attach probes only that route. When it fails, the pane lists the other routes and the user retries with one of them; the one that works becomes the last route. A place with no remembered route probes its candidates once to establish one. The tab menu has **Reconnect as**: it lists the candidates with a check on the endpoint in use (the session's route, else the last route, else the first candidate) and reconnects through the one picked.

Keep the probe output as `<caches>/hostkeys-<place-id>`. The attach uses the same `HostKeyAlias`, `StrictHostKeyChecking=yes`, and `HostName=<route>` when the route differs from the place's endpoint, so every route shares one known_hosts identity while an SSH config alias still resolves its own `HostName`. Do not write the user's `known_hosts`.

Add Place asks only for **user** and an **endpoint**. The endpoint may be a hostname, an IP, an SSH config alias, or an SSH address; it is kept verbatim, and an `@` inside it stays in the address. An empty user means your login. rterm never reads or writes `~/.ssh/config`; the `ssh` it runs resolves an alias the usual way. The SSH host key fingerprint is the node identity. rterm probes the endpoint once, reads its host key, and adds the endpoint to the node that already owns that fingerprint, on every tab of it; when no node owns it, rterm creates one tab with that fingerprint and the endpoint. The endpoint is never a match key, so the same node reached through another address joins the existing tabs. No backend, session, label, route list, or fingerprint field in Add. The full form (including routes and **Find routes**) remains in Edit.

**New session** in the tab menu opens another access to the node the tab belongs to: another backend or tmux/screen session name, sharing fingerprint, user, and routes. Choosing an access that already exists focuses its tab instead of duplicating it.

The list holds one tab per access. A verified endpoint that reaches a place without a pin completes that place with the fingerprint instead of adding a second tab, and keeps its label and session. The duplicate key is login + fingerprint + backend + session: tabs with another access (or another login) stay apart and share the machine's routes. Loading folds duplicates the same way: by that key first, then an unpinned place into the pinned node it reaches, then unpinned copies by switch key.

A place that already has its pin is never asked to confirm it again. A route that presents another key is another node and is never attached.

Places loaded from a v1 `places.toml` join the same in-memory model with no routes and no pin; `host` is still tried as the direct route. Loading does not rewrite or delete the v1 file. Migrating v1 on disk is pending an explicit decision.

Discovery runs without attaching. It lists reachable routes with their fingerprint, keeps the routes that match the pin, and saves the pin.

## Window and tabs

v1 has exactly one window. Always.

The tab strip is the catalog. Every place is a tab; the same node may own several tabs, one per access. Tab order is catalog list order.

Tabs do not close. An unused tab is idle. The user selects a tab to show that place.

Do not add a sidebar in v1. Do not add a second window. Do not add splits. Splits belong to herdr or tmux on the remote.

## Lifecycle

Select a tab. If the place is not attached, attach it.

The local connection is disposable. A drop is normal.

On system sleep (or any system hook that means connections will die), tear down every local SSH process. Each tab becomes idle.

On wake, do not attach anything. Attach when the user selects a tab that is not attached, or when the user chooses Reconnect.

Reconnect runs the same attach path as the first attach.

## Attach

If host is empty, attach is local. Start the process in the user's home directory, not `/`. Run the driver command in a login shell so `PATH` matches the user's shell:

```text
$SHELL -lc '<driver-command>'
```

If backend is `herdr`, attach by running the **same binary as the running herdr server** on that machine (local or remote). Resolve it from the process that owns `herdr.sock` (`/proc/<pid>/exe` or `ps`). Fall back to `herdr` on `PATH` if no server is running.

Do not use `herdr --remote`. SSH + a PATH `herdr` can be a different binary than the server (wrapper, old pin, other channel).

When the place names a host and the backend is `tmux` or `screen`, use SSH. Call `/usr/bin/ssh`. Do not use libssh.

Force a remote TTY (`ssh -t`). Run the driver command inside a login shell so the remote `PATH` is the user's login `PATH`:

```text
ssh -t -o ServerAliveInterval=5 -o ServerAliveCountMax=2 -o ControlMaster=auto -o ControlPath=<caches>/cm-<place-id> -o ControlPersist=no -o HostKeyAlias=rterm-<place-id> -o UserKnownHostsFile=<caches>/hostkeys-<place-id> -o StrictHostKeyChecking=yes -o HostName=<route-host> [-p <port>] user@<place-host> -- exec "$SHELL" -lc '<driver-command>'
```

`$SHELL` is the user's shell (local or remote as SSH resolves it). Do not hard-code bash or zsh.

`<caches>` is `~/Library/Caches/rterm`. One ControlPath per place id. The attach `ssh` is the master. Sleep still kills that process. `ControlPersist=no` removes the socket with it.

A dead path must make `ssh` exit. ServerAlive options do that. The tab dot is grey when idle, green when the process is alive, and red when attach failed. Do not show a latency color. Local places have no ServerAlive options and no ControlMaster.

If a connecting headline (`Connecting…` or `Connecting to …`) is idle for 5 seconds with no new SSH trace, run Reconnect. Trace output resets that idle timer. After 3 idle attempts, fail with `connection timed out`. Later headlines (Connected, Authenticating, Offering key) are not this hang.

Driver commands:

| backend | command |
| --- | --- |
| herdr | same binary as the running `herdr server`, else `herdr` |
| tmux | `tmux new-session -A -s <session>` |
| screen | `screen -d -R <session>` |

If the named tmux or screen session is missing, create it (the commands above). If herdr needs to start its remote server, let `herdr` do that.

If the herdr, tmux, or screen binary is missing on the host, attach fails. Do not install packages on the remote.

`~/.ssh/config` is transport only: keys, `Host` aliases, `ProxyJump`. It is not the catalog. A later Add-place picker may list `Host` names. That picker is not in v1.

## Prompts and failures

The pane is the terminal surface for the attached process.

SSH prompts (password, host key, OTP) stay as bytes in the pane. Do not wrap `SSH_ASKPASS`. Do not put those prompts in a Mac sheet.

ssh-agent, keys, and Touch ID stay outside rterm.

rterm classifies some failures: host unreachable, mux binary missing, or a non-zero exit that is not a clean detach. On those failures, hide the pane. Draw an overlay: the error text and "Click to attach again". Click or Reconnect runs attach again.

## Image paste

Image paste writes a PNG the destination can open. It does not draw the image in the pane.

Use the clipboard only when it has image data and no text. A copy that has both is text.

| Gesture | Local place | Remote place |
| --- | --- | --- |
| ⌘V, image only | write `/tmp/rterm-paste-<id>.png` on this machine and paste that path | upload that path on the host, then paste it |
| ⌘V, text present | paste the text into the process | paste the text into the process |
| Ctrl+V, image only | send Ctrl+V to the process | same upload and path as ⌘V |
| Ctrl+V, text present | send Ctrl+V to the process | send Ctrl+V to the process |

Each paste uses a new UUIDv7 `<id>`. Cap the PNG at 10 MiB. A larger image or a failed write beeps. Do not paste a truncated file.

While write or upload runs, show a corner overlay on the pane. Show a spinner, Writing image… or Uploading image…, and a bar for bytes written. The bar counts the local file or ssh stdin. On failure, keep the beep and show Paste failed for 1.5 seconds. The overlay must not steal clicks. Do not draw the image.

Remote upload uses a second `/usr/bin/ssh` on the attach ControlPath:

```text
ssh -o BatchMode=yes -o ControlPath=<caches>/cm-<place-id> -o HostKeyAlias=rterm-<place-id> -o UserKnownHostsFile=<caches>/hostkeys-<place-id> -o StrictHostKeyChecking=yes -o HostName=<route-host> [-p <port>] user@<place-host> -- cat > '/tmp/rterm-paste-<id>.png'
```

Do not open a TTY. Do not install a remote helper. Do not write into the place working directory. Leave the file in `/tmp`.

Do not implement OSC 5522 or inline graphics for this.

## Catalog

Path: `~/Documents/rterm/*.rterm-place`

Each place is one TOML file. Filename sort is tab order (`00-ada@whiterun-herdr.rterm-place`). There is no separate order field. Drag tabs to reorder. Save rewrites every prefix from `00`.

A CRUD GUI in the app reads and writes these files. Add, edit, delete, and reorder places there. **Find routes** in that sheet discovers and probes routes.

If a file in that directory changes on disk, reload.

On first launch, read `~/Library/Application Support/rterm/places.toml` into the same in-memory model. Do not rewrite or delete that file until the user approves a migration.

Each place record has: `id`, `user`, `host`, `backend`, optional `session`, optional `label`, optional `fingerprint`, and zero or more `[[place.route]]` records. A route has `kind`, `host`, optional `port`, and optional `last` (the route that worked most recently).

```toml
version = 2

[[place]]
id = "0192F3C1-7A4E-7C3A-9B2D-1F0A2B3C4D5E"
user = "ada"
host = "whiterun"
backend = "herdr"
fingerprint = "SHA256:C8olMcLxERkizT4dlzO1z85UVxdq/COQVR0T3I9IEK8"

[[place.route]]
kind = "mdns"
host = "whiterun.local"

[[place.route]]
kind = "tailscale"
host = "whiterun.tailnet.ts.net"
last = true
```

Version 1 files have no routes or fingerprint. They decode with no routes and no pin; `host` is still tried as the direct route. Such a place adopts the fingerprint once a verified endpoint reaches it, so it never has to be added twice. Write version 2 on the next save.

Do not use `~/.config`. Do not use `UserDefaults` as the store.

## Stack

| Layer | Choice |
| --- | --- |
| Chrome | egui (eframe) for tabs, the editor, and overlays |
| Pane | vt100 screen on a PTY |
| SSH | `/usr/bin/ssh` as a child of that PTY. If that path is missing, `ssh` on `PATH` |
| Bundle id | `br.tec.lew.rterm` |
| Toolchain | Rust 1.93.0 via mise. Linux local builds use conda gcc and binutils |
| Release | GoReleaser links with zig for its default Rust targets |

Do not use librio or rio-vt.

Inline images (sixel, Kitty, iTerm) are not a requirement.

## Out of scope for v1

- More than one window
- Sidebar catalog
- Flattening herdr or tmux UI into native Mac chrome
- Creating remote users or installing mux binaries
- mosh or Eternal Terminal
- Drag and drop of image files
- OSC 5522 clipboard protocol
- App display name beyond the repo name `rterm`
- Pricing and store listing
