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
| Field that selects herdr, tmux, or screen | backend | multiplexer type, kind |
| Code that turns a place into an argv | driver | adapter, plugin |
| Durable list of places | catalog | config (alone), database |
| Visible SwiftTerm view | pane | terminal (alone), PTY view |
| Start or reuse the remote session | attach | connect (except user-facing "not connected"), reconnect as a separate verb |
| User-visible name of a place | label | title, alias, nickname |
| Centered icon and error on the pane | overlay | modal, sheet, alert (for mux/host failures) |
| SSH password, host-key, and OTP text | prompt | dialog (for those bytes) |

Attach is the only verb that starts a place. The menu item Reconnect runs attach again.

## Product

rterm is a native Mac app. It supervises detachable remote sessions. It is not a general local-shell terminal.

The remote session is the source of truth. The local SSH process is disposable.

Destinations must already run a detachable session tool: herdr, tmux, or screen.

v1 is a Mac app. Linux, Windows, Iced, and winit ports are out of scope. You may sell a later Mac release. That release still follows these v1 rules until you change this file.

## Place

A place is:

- user (required when the place is remote)
- host (empty, `localhost`, `127.0.0.1`, or `::1` means this Mac)
- backend: `herdr`, `tmux`, or `screen`
- session name: required for `tmux` and `screen`; absent for `herdr`
- label: optional

herdr has no session name in rterm.

An empty host or a loopback host (`localhost`, `127.0.0.1`, `::1`) is a local place. Attach runs the mux on this Mac as the Mac login user. Do not use SSH. Persist no host. If user is empty, use `NSUserName()`. Default label is `user@localhost:herdr`.

If the user leaves the label empty, the default is:

- remote herdr: `user@host:herdr`
- remote tmux or screen: `user@host:tmux(foo)` or `user@host:screen(foo)`
- local herdr: `user@localhost:herdr` (user defaults to the Mac login name)
- local tmux or screen: `user@localhost:tmux(foo)`

Parse form: `user@host` or `user@host:backend(args)`. A missing `:backend` means `herdr`. A missing host means local.

Two places may share a host. The stable identity of a place is a generated `id` in the catalog. The label is not the id.

## Window and tabs

v1 has exactly one window. Always.

The tab strip is the catalog. Every place is a tab. Tab order is catalog list order.

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

If host is set and backend is `tmux` or `screen`, transport is SSH. Use `/usr/bin/ssh`. Do not use libssh.

Force a remote TTY (`ssh -t`). Run the driver command inside a login shell so the remote `PATH` is the user's login `PATH`:

```text
ssh -t -o ServerAliveInterval=5 -o ServerAliveCountMax=2 -o ControlMaster=auto -o ControlPath=<caches>/cm-<place-id> -o ControlPersist=no user@host -- exec "$SHELL" -lc '<driver-command>'
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

The pane is always a SwiftTerm surface.

SSH prompts (password, host key, OTP) stay as bytes in the pane. Do not wrap `SSH_ASKPASS`. Do not put those prompts in a Mac sheet.

ssh-agent, keys, and Touch ID stay outside rterm.

rterm classifies some failures: host unreachable, mux binary missing, or a non-zero exit that is not a clean detach. On those failures, keep the last pane frame. Draw an overlay on it: centered icon and the error text. Click or Reconnect runs attach again.

## Image paste

Image paste writes a PNG the destination can open. It does not draw the image in the pane.

Use the clipboard only when it has image data and no text. A copy that has both is text.

| Gesture | Local place | Remote place |
| --- | --- | --- |
| ⌘V, image only | write `/tmp/rterm-paste-<id>.png` on this Mac and paste that path | upload the same path on the host, then paste it |
| Ctrl+V, image only | send Ctrl+V to the process | same upload and path as ⌘V |
| any gesture, text present | paste text | paste text |

Each paste uses a new `<id>`. Cap the PNG at 10 MiB. A larger image or a failed write beeps. Do not paste a truncated file.

Remote upload uses a second `/usr/bin/ssh` on the attach ControlPath:

```text
ssh -o BatchMode=yes -o ControlPath=<caches>/cm-<place-id> user@host -- cat > '/tmp/rterm-paste-<id>.png'
```

Do not open a TTY. Do not install a remote helper. Do not write into the place working directory. Leave the file in `/tmp`.

Do not implement OSC 5522 or inline graphics for this.

## Catalog

Path: `~/Library/Application Support/rterm/places.toml`

Do not use `~/.config`. Do not use `UserDefaults` as the store. Do not use iCloud in v1.

Format is TOML with a `version` field. List order is tab order. There is no separate order field.

A CRUD GUI in the app reads and writes this file. Add, edit, and delete places there.

If the file changes on disk, reload it.

Each place record has: `id`, `user`, `host`, `backend`, optional `session`, optional `label`.

## Stack

| Layer | Choice |
| --- | --- |
| Chrome | SwiftUI (tabs, CRUD sheet, overlay) |
| Pane | SwiftTerm in an `NSViewRepresentable` |
| SSH | `/usr/bin/ssh` as a child process in the SwiftTerm PTY |
| Bundle id | `br.tec.lew.rterm` |
| Toolchain | Xcode; XcodeGen via project `mise.toml` |

Do not use librio, rio-vt, Iced, or a winit/wgpu shell in v1.

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
