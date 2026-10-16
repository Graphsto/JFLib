# App permissions (`apps/NAME/PERMISSIONS`)

An app can declare which files, programs, URL schemes, plugins and printers it uses. The
platform layer then refuses everything else, in every part of the process: the app's own
code, the library, codecs, plugins that go through the host API, scripts and agent actions.
Apps without a `PERMISSIONS` file keep the old behaviour and may do everything.

The model resembles Tauri's capabilities or OpenBSD's `unveil`. The app states what it
needs. The build checks the syntax. `ui::main!` installs the rules before `App::init`.
From then on the rules can only grow by files the user hands over.

| Part | Where |
| --- | --- |
| Rules, path normalisation, checks, denials | [`lib/plat/policy.rs`](../lib/plat/policy.rs) |
| Checked file functions (`read_file`, `File::open`, ...) | [`lib/plat/guarded.rs`](../lib/plat/guarded.rs), [`lib/plat/fs.rs`](../lib/plat/fs.rs) |
| Install, grants for the user's files, trusted library code | [`lib/ui/perm.rs`](../lib/ui/perm.rs) |
| Syntax check, hand-over to `rustc` | `app_permissions` in [`build_rs.py`](../build_rs.py) |
| Tests | `plattest --test policy` ([`t_policy.rs`](../apps/plattest/t_policy.rs)), [`apps/permtest`](../apps/permtest/main.rs) |

## The manifest

One rule per line; `#` starts a comment; a key may appear more than once.

```text
# apps/MyTool/PERMISSIONS
fs.read  $EXE/templates/**            # read below a folder
fs.write $CONFIG/MyTool/**            # write (and read) below a folder
fs.write $TEMP/mytool-*/**
fs.user                               # files the user chooses: read and write
process  gmic  $EXE/helpers/*.exe     # programs Command::spawn / run_elevated may start
url      https mailto                 # schemes for open_url ("*" = all)
plugins  $EXE/plugins/*.dll $EXE/plugins/*.so
print                                 # plat::print
```

| Key | Arguments | Allows |
| --- | --- | --- |
| `fs.read` | patterns | reading files and listing folders below the patterns |
| `fs.write` | patterns | creating, writing, renaming and deleting below the patterns (includes reading) |
| `fs.user` | – | the files of the file dialog, of drops and of the command line: read and write |
| `process` | patterns or program names | `plat::process::Command::spawn` and `run_elevated`; a name without `/` matches a program looked up in `PATH` |
| `url` | schemes or `*` | `plat::open_url` (`https`, `mailto`, ...) |
| `plugins` | patterns | `plat::lib_open` (native plugin libraries) |
| `print` | – | `plat::print` |

### Patterns

- A pattern is an absolute path or starts with a variable: `$CONFIG` (`config_dir()`),
  `$TEMP` (`temp_dir()`), `$HOME`, `$EXE` (the folder of the executable) or `$FONTS` (every
  system font folder). A variable without a value on this machine drops the pattern.
- `*` and `?` match inside one path segment. A `**` segment matches any number of
  segments. `dir/**` also matches `dir` itself. `**` alone matches every path.
- `X:/...` and `//server/share/...` are Windows paths; `/...` is a Linux path. Each OS
  ignores the other's absolute patterns, so one manifest serves both.
- Windows compares without case.

`python build_rs.py app NAME` rejects unknown keys, relative paths, unknown variables and
bad URL schemes with the file and line number. The app is rebuilt when the manifest changes.

## What is always allowed

- Reading the system font folders (`plat::policy::font_dirs`, the folders fontdb scans) and
  the executable's folder (assets, modules).
- On Linux, reading `/etc/resolv.conf` and `/etc/hosts` (the resolver of `netio`).
- Anonymous temporary files: `File::open_temp(None)`.
- `spawn_self`: a second instance of the same program, with the same manifest.
- The platform layer's own system files: the X11 cookie, ICC profiles, CA bundles, the time
  zone, `PATH` lookups, the portal's and the print dialog's results. These calls use the
  unchecked `*_raw` functions inside `plat`.

## The user's files

With `fs.user` the UI runtime grants read and write access to:

- the path the file dialog returns (system dialog or the library's own),
- every path of a drop (`UIC_DROP`),
- the command-line arguments that are not options (`--name=value` counts with its value),
  for example the file of "Open with".

A granted folder includes everything below it. Without `fs.user` the user can still choose
files, but the app cannot open them.

The library's own file dialog lists folders without checks, because the user is navigating.
`--shot` and the file commands of `.uis` scripts (`shot`, `dump`, `clip file` ...) are also
unchecked: they come from whoever started the process. The key (`plat::policy::Key`) that allows this stays
inside the `ui` crate. App code cannot widen its own rules.

## Refusals

A refused call fails the way a missing or unreadable target fails: `None`, `false`, `0`,
`-1`, `Err(LIBE_IO)`, `ERR_UNSUPPORTED` (print), or `Err(5 / 13)` (access denied, process).
In addition:

- stderr shows `PERMDENY <kind> <target>` once per target, for example
  `PERMDENY fs.write C:/Users/me/Desktop/out.png`;
- `plat::policy::denials()` returns the last 64 refusals (`kind`, `target`) for a log or a
  diagnostics page;
- `plat::capabilities()` drops `CAP_FILE_READ` / `CAP_FILE_WRITE` (no file rules and no
  `fs.user`), `CAP_PRINT` and `CAP_NATIVE_PLUGINS`. App commands and the agent schema then
  report the service as unavailable instead of failing later.

## Limits

- **Lexical checks.** A symbolic link inside an allowed folder can lead outside it. On
  Linux a path with a `..` segment is refused, because the kernel applies `..` after
  symbolic links (`allowed/link/../x`). Windows resolves `..` in the text, as the policy does.
- **Not a sandbox against native code.** Native plugins and app code that call the OS
  directly bypass `plat`. Kernel enforcement (Landlock on Linux, AppContainer / MSIX
  capabilities on Windows) is a later step ([roadmap](ROADMAP.md) §1).
- **Networking is not restricted yet.** Host and port scopes for `plat::net` / `netio`
  follow in the next package.
- **Coverage.** Only the main GUI executable of an app gets the manifest. Programs from
  `apps/NAME/BINS`, `rt::main!` console programs and Android APKs are not covered yet.
- **Agent mode.** An agent that drives the library's file dialog (`--agent-json`,
  `--agent-mcp`) can choose files like a user. Agent mode must be enabled explicitly.
