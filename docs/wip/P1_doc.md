# P1 -- Plugin-System (M8): Dokumentation

# PaintCraft plugins (M8, agent P1)

Effects, adjustments, file types and shapes can come from plugin libraries.  Spec: PLUGIN-001..020
(booksAndContext/PaintCraft/paintdotnet_feature_specification.md).  Loader and crash guard:
`ASMLIB/docs/modules/PLUGIN_LOADER.md`.  Include file: `ASMLIB/inc/plugin.inc`.

Files: `src/plugins.s` (loading, validation, registration, thunks, PNAPI, error list, hooks),
`ASMLIB/src/plat/plib.s` + `plat/win/win_lib.s` + `plat/linux/elfload.s` + `plat/linux/lx_guard.s`
(loader, guard), examples in `plugins/` (top level), `tools/plugin_check.py`, tests
`tests/plugins.uis`, `tests/plugins_crash.uis`.

## 1. Folders, build

At start-up (`plug_init`, before `fio_init` / `fx_init`) PaintCraft loads every `*.dll` (Windows) /
`*.so` (Linux) of `<root>/Effects`, `<root>/FileTypes`, `<root>/Shapes` in name order (at most 32
files).  `<root>` = the folder of PaintCraft's executable; `--plugins=DIR` (or `$PDN_PLUGIN_DIR`)
uses DIR instead, `--plugins=@name` the folder `name` next to the executable (tests),
`--no-plugins` loads nothing.  `--plugins DIR` (two arguments) works too, but fileio.s still
takes DIR for a file to open ("Die Datei konnte nicht gefunden werden") until "--plugins" and
"--plugin-fuzz" are added to its `value_opts` table.  Any plugin may bring any kind of descriptor; the folders only sort
them.  The XAML shapes of `<exe>/Shapes/*.xaml` (SHAPES.md) are unaffected.

```
python build.py plugin NAME [--target win|linux|both] [--out build/x]   # plugins/NAME/*.s
python build.py plugin                                                  # every plugins/*
```
Output `build/x/win/plugins/NAME.dll` (PE DLL, no entry point, no imports, export
`pn_plugin_query`) and `build/x/linux/plugins/NAME.so` (ET_DYN, `-Bsymbolic`, no DT_NEEDED, only
RELATIVE relocations).  Plugins are **not** linked against libjfgui (no second copy of global
state): they include `jfgui.inc` + `plugin.inc` for constants and macros only and call the host
through the PNAPI table.  Put every table that holds pointers (PLUGININFO, FXDESC, FXPROP,
FTDESC, shape list) into `.data` -- the loader relocates it; `.rodata` must not contain pointers.

Examples: `swapchan` (effect "Kanäle tauschen", choice property), `sepiastr` (adjustment
"Sepia-Stärke", integer property), `qoi` (file type QOI, decoder + encoder), `pshapes` (two
shapes).  Test-only (never copy them into a real folder): `crash` (render writes to address 0),
`crashpf` (division by zero in a parallel_for item), `crashq` (ud2 in pn_plugin_query),
`toonew` (needs API version 2), `badinfo` (effect list outside the image).

## 2. Entry point and versioning

```
pn_plugin_query(PNAPI* api) -> rax PLUGININFO* | 0      (Win64 ABI on both OSes)
```
Called once on the UI thread.  Keep `api` if later calls need it (a plugin global written here
is fine; render must not write globals).  Return 0 to refuse loading.

* **PNAPI_VERSION = 1** (host).  `PI_APIMIN` = the lowest host version the plugin needs; a
  plugin with `PI_APIMIN > PNAPI_VERSION` (or 0) is rejected ("Das Plugin benötigt die
  Plugin-API-Version N, vorhanden ist M.").
* The PNAPI table starts with `PA_SIZE` (bytes) and `PA_VERSION`; later host versions only
  append entries -- test `PA_SIZE` before using an entry beyond `PA_SIZE_V1`.
* `PI_SIZE` = size of the plugin's PLUGININFO (>= `PI_SIZE_V1` = 88); fields added later are
  read only when `PI_SIZE` covers them.

### PNAPI (host functions, all Win64 ABI)

| offset | function |
|---|---|
| PA_ALLOC 8 | `(size) -> rax` zeroed, 64-byte aligned block or 0 (mem_alloc; thread safe) |
| PA_FREE 16 | `(ptr)` |
| PA_REALLOC 24 | `(ptr, size) -> rax` |
| PA_PARFOR 32 | `(fn, ctx, count) -> eax 0 / ERR_CANCEL / ERR_FAULT`: `fn(ctx, index, job)` for 0..count-1 on the worker threads and the caller; every item runs guarded |
| PA_HASH32 40 | `(x, y, seed, stream) -> eax` deterministic per-pixel random bits (fx_hash32) |
| PA_LOG 48 | `(msg)` UTF-8 line to stderr ("PaintCraft: Plugin <file>: msg"), any thread |
| PA_ROW 56 | `(sf, y) -> rax` pointer to row y of a SURFACE or 0 |
| PA_SURFALLOC 64 | `(sf, w, h, dec) -> eax 0 / ERR_*`: straight FMT_ARGB surface for decoders (limits of the IMGDEC, may be 0) |
| PA_CANCELLED 72 | `(job) -> eax` 1 when the job's cancel flag is set |
| PA_FMT 80 | `(buf, cap, fmt, ...) -> rax` printf subset (fmt_buf) |
| PA_NTHREADS 88 | `() -> eax` worker threads |

The SURFACE record (`jfgui.inc` SF_PX / SF_W / SF_H / SF_STRIDE / SF_FMT) and the FXARGS /
IMGDEC / IMGMETA / IMGOPT / JOB records are part of the ABI of version 1.

## 3. PLUGININFO and descriptors (`plugin.inc`)

```
PLUGININFO name, version, author, url, apimin, nfx, fx, nft, ft, nshape, shapes   (macro)
  PI_MAGIC "PNPI", PI_SIZE, PI_APIMIN, PI_FLAGS 0, PI_NAME (1..63 bytes), PI_VERSION (1..31),
  PI_AUTHOR / PI_URL (or 0), PI_NFX + PI_FX -> FXDESC*[n], PI_NFT + PI_FT -> FTDESC[n],
  PI_NSHAPE + PI_SHAPES -> SHAPEDESC[n]
```
**Effects / adjustments**: `FXDESC` / `FXPROP` exactly as `ASMLIB/inc/fx.inc` (same contract as
the built-in kernels: pure render over ROIs, deterministic, poll `JOB_CANCEL`, straight ARGB).
Plugin specifics: `FXD_ID` is ignored (the host assigns 0x10000 + slot); `FXD_NAME` is the menu
name shown as is (UTF-8, 1..47 bytes, no '=' and no leading ';' '#' '[' or blank; it is also the
settings key, so it must be unique -- duplicates are rejected); `FXF_ADJ` puts it into
Korrekturen, else into Effekte; `FXP_NAME` is the label shown in the generic dialog (common keys
like "radius" are translated); `FP_BLOB` is not available; `FP_CHOICE` needs 1..32 choices and
`0 <= min <= default <= max < choices`; numbers must be finite with `min <= default <= max`.
Up to 16 effects per plugin, 40 in total.

**File types** (`FTDESC name, exts, mime, probe, decode, encode, caps`): same function contract
as `IMGCODEC` in `img.inc` -- `probe(data, len, meta) -> 1/0` (magic check; 0 = only by the
file extension), `decode(data, len, sf, dec) -> 0 / ERR_*` (allocate the result with
`PA_SURFALLOC`, straight or premultiplied ARGB; with `IMGD_PROBE` pixels are optional),
`encode(sf, opt, &buf, &len) -> 0 / ERR_*` (`sf` is always straight FMT_ARGB; the buffer must
come from `PA_ALLOC`, the host frees it).  At least a decoder or an encoder.  `exts` = 1..8
extensions of 1..15 characters [a-z0-9] separated by ';' (no dots); an extension PaintCraft or an
earlier plugin already knows is rejected.  `caps`: IFC_ALPHA / LOSSY / LOSSLESS / ANIM / PAGES /
HDR.  The type appears in Öffnen ("Alle Bildtypen" + its own entry) and Speichern unter (with an
encoder) -- IMGF 14..21 (`img_register_dyn`, at most 8).  No save-options dialog (IOM 0).

**Shapes** (`SHAPEDESC {name, pathdata}`): 1..63-byte name, path data in the Geometry syntax of
paint.net ShapeKit XAML (`F1 M 0,0 L 100,0 ...`, letters, digits, blanks, `,.+-` only).  They are
fed to the XAML loader of the Formen tool (one XAML document per plugin) and appear after the
built-in shapes; up to 32 per plugin, 64 in total.

## 4. Threading rules

* `pn_plugin_query`: UI thread, once.
* render: many worker threads at once (and the UI thread); prepare: one worker; free: any
  thread.  No writable globals, memory only through PNAPI, results independent of the ROI split.
* Codec functions: any thread, possibly concurrently (thumbnails, open).  Thread safe, no globals.
* PNAPI functions are thread safe; `PA_PARFOR` may be nested.

## 5. Validation and errors (PLUGIN-013/014/015/019/020)

Every pointer in the plugin's data must lie inside one readable segment of its image, every
function pointer in an executable one, strings are NUL-terminated inside the segment, without
control characters.  Anything else rejects the plugin with a message, nothing of it is
registered and the library is unloaded.  A fault anywhere in plugin code (or in host code the
plugin called) is caught: the plugin is **disabled** for the rest of the session (its effects
report "Das Plugin ist abgestürzt und wurde deaktiviert …", its menu rows only show a message,
its codecs return errors), the message ("Absturz in render (Zugriffsverletzung bei +0x100a,
Adresse 0x0). …") goes to stderr and the error list.  A broken plugin never prevents the start;
removing a plugin file only removes its entries.

UI: Effekte ▸ "Plugin-Fehler…" (visible while errors exist) opens Einstellungen ▸ Plugin-Fehler:
the error lines, then "Geladene Plugins" (name, version, author, file, counts).  Diagnose lists
"Plugins: N geladen".  Getters: `plug_count`, `plug_errors`, `plug_loaded`, `plug_text(i, buf,
cap) -> PS_*`.

## 6. Minimal effect plugin

```asm
        .intel_syntax noprefix
        .include "jfgui.inc"
        .include "plugin.inc"
        .text
        .globl pn_plugin_query
pn_plugin_query:                                # (PNAPI*) -> PLUGININFO*
        lea     rax, [rip + info]
        ret
render:                                         # (FXARGS*) -> eax 0 / ERR_CANCEL
        ...                                     # dst = f(src) inside [x0,x1) x [y0,y1)
        .data
        .p2align 3
info:   PLUGININFO s_name, s_ver, s_author, 0, 1, 1, fx_list, 0, 0, 0, 0
fx_list: .quad  fx_desc
fx_desc: FXDESC 0, 0, s_fx, 0, render, 0, 1, props, FXC_COLOR
props:  FXPROP  FP_INT, 0, 0, 0, s_amount, 0.0, 100.0, 50.0
        .section .rodata
s_name: .asciz  "Mein Plugin"
...
```
See `plugins/swapchan/swapchan.s` (effect), `plugins/qoi/qoi.s` (file type).

## 7. Tests / hooks

`python tools/plugin_check.py [--target ...] [--out build/p1] [--fuzz N]`: builds, prepares
`<out>/<target>/ptest`, runs `tests/plugins*.uis` (+ `--verify-dirty`), decodes the QOI file the
script wrote with an independent Python codec, fuzzes the loader (1200 ELF mutants on Linux,
1200 DLL mutants on Windows through `--plugin-fuzz DIR`) and loads 120 data mutants through the
full path.  `--fixtures` rewrites `tests/fixtures/p1_plugin.png` / `.qoi` and prints the
expected hashes.  `build.py test` fills `<out>/<target>/ptest` itself (scripts use
`--plugins=@ptest`).

Hooks: cmd 145 i -> #0x82401 text `state|where|kind|effects|filetypes|shapes|name` (PS_: 1
loaded, 2 rejected, 3 crashed; FW_: 0 loader, 1 query, 2 import, 3 prepare, 4 render, 5 free,
6 probe, 7 decode, 8 encode, 9 parallel_for; GK_ of plugin.inc), value = state;
#0x82400 text `loaded,errors,effects,filetypes,shapes`, value = records; #0x82402 fuzz run
`files,loaded,rejected,faults`; cmd 146 need -> #0x82403 text = `img_filter_list(need)` with
'|' between the entries (1 = decoders / Öffnen, 2 = encoders / Speichern unter).  Commands
147..149 are reserved (P1).

## 8. Not supported / limits

* GPU / shader effects (PLUGIN-017/018): no GPU API in the library; PNAPI can grow later.
* No custom dialogs / UI for plugins (generic property dialog only), no plugin settings pages.
* No unloading at run time; a disabled plugin stays mapped (worker threads may still be inside).
* Windows: plugins must not import anything (not even kernel32) and have no DllMain / TLS.
* Linux: no DT_NEEDED, no initialisers, no TLS, RELA relocations RELATIVE / 64 / GLOB_DAT /
  JUMP_SLOT against own symbols only.
* The crash guard is not a sandbox: a plugin runs with the full rights of PaintCraft (see
  PLUGIN_LOADER.md §3).

---

# Plugin loader and crash guard (agent P1)

Platform services to load untrusted plugin libraries without libc / dynamic linker and to call
into them so that a crash does not end the process.  Include `plugin.inc`.  App side (PaintCraft):
`apps/PaintCraft/docs/PLUGINS.md`.

| file | contents |
|---|---|
| `src/plat/plib.s` (both) | `plat_guard_init`, `plat_guard_call`, `plat_guard_tag`, `plat_lib_seg`, thread table |
| `src/plat/win/win_lib.s` | `plat_lib_open/sym/close` (Windows), VEH `guard_veh`, `plat_tid` |
| `src/plat/linux/elfload.s` | `plat_lib_open/sym/close` (own ELF loader) |
| `src/plat/linux/lx_guard.s` | signal handler `guard_sig`, per-thread alternate stacks, `plat_tid` |

None of it depends on the display backend (disp.s); the functions link only into apps that
use them.

## 1. API

```
plat_lib_open(path, &err) -> rax LIBH* | 0        *err = LIBE_IO / FORMAT / DEPS / RELOC / NOMEM /
                                                   INIT / MAP
plat_lib_sym(h, name) -> rax address | 0
plat_lib_close(h)
plat_lib_seg(h, addr) -> rax end of the segment holding addr (0 = outside the image), edx LIBA_R|W|X
plat_guard_init() -> eax 0                         install the handler (once; lazy otherwise)
plat_guard_call(fn, a1, a2, a3, a4, info) -> rax, rdx     fn(a1..a4) guarded; ERR_FAULT (-64)
plat_guard_tag() -> rax                            GI_TAG of this thread's innermost guard
```
`LIBH` (LH_*): base, span, OS handle, segments `{start, end, LIBA_*}` (at most 16), Linux:
.dynsym / .dynstr.  `GUARDINFO` (GI_*): kind (GK_ACCESS / ILLEGAL / ARITH / BUS / BREAK / STACK),
code (NTSTATUS / signal), faulting pc, data address, tag (input).  Use `plat_lib_seg` to check
every pointer a plugin hands over (data: LIBA_R, code: LIBA_X) before dereferencing it.

## 2. Loaders

**Windows** -- `LoadLibraryExW(path, DONT_RESOLVE_DLL_REFERENCES)` (resolved via
GetProcAddress; hard-error boxes off with SetThreadErrorMode): the OS maps and relocates the
image, but no DllMain, no TLS callback and no import binding run.  The mapped headers are then
checked: PE32+, AMD64, IMAGE_FILE_DLL, SizeOfImage <= 256 MB, section table inside the headers,
<= 15 sections; an import, delay-import or TLS directory rejects it (LIBE_DEPS / LIBE_INIT).
Segments = headers (R) + sections (R/W/X from the characteristics).  `plat_lib_sym` parses the
export directory itself (bounds against SizeOfImage, every table / name inside a readable
section -- a section without MEM_READ is PAGE_NOACCESS; forwarders return 0, so no other DLL is
ever loaded).

**Linux** -- `elfload.s`: the file (<= 64 MB) is read, then validated with 64-bit arithmetic
against the file size / the span: ELF64 LE, ET_DYN, x86-64, phentsize 56, 1..64 program headers,
PT_LOAD filesz <= memsz, inside the file, vaddr + memsz <= 256 MB, ascending and page-disjoint,
never W+X; exactly one PT_DYNAMIC inside the file; PT_INTERP / PT_TLS reject (LIBE_INIT).
One anonymous RW mapping of the span, segments copied in; dynamic tags: DT_NEEDED -> LIBE_DEPS,
DT_INIT / non-empty INIT_ARRAY / PREINIT_ARRAY -> LIBE_INIT (nothing of the object runs at load),
DT_REL -> LIBE_RELOC; SYMTAB / STRTAB / STRSZ inside the span, SYMENT / RELAENT 24.  Symbol count:
DT_HASH nchain, else the DT_GNU_HASH buckets + chain ends (all bounds checked, <= 65536).
Relocations (DT_RELA and DT_JMPREL with DT_PLTREL = RELA): NONE, RELATIVE, 64 / GLOB_DAT /
JUMP_SLOT against a *defined* symbol (undefined -> LIBE_DEPS, others -> LIBE_RELOC); every target
lies inside a PT_LOAD.  Then mprotect per segment (R/W/X from p_flags), gaps PROT_NONE,
PT_GNU_RELRO read-only (whole pages only).  `plat_lib_sym` scans .dynsym (defined symbols only,
names bounded by DT_STRSZ).

Robustness (tools/plugin_check.py): 1200 mutated ELF objects (bit flips anywhere, in the first
4 KB, byte values 0/FF/7F/80, truncations) and 1200 mutated DLLs pass through open / sym /
close without a single fault inside the loader; 120 data-only mutants (PLUGININFO and
descriptors) go through `pn_plugin_query` + validation without a crash.

## 3. Crash guard

`plat_guard_call` pushes a guard record on its own frame (rsp / rbp of its body, xmm6-15, MXCSR,
x87 control word, tag) and links it as the innermost guard of the calling thread in a table
{OS thread id -> record} (128 threads; thread id = TEB ClientId on Windows, gettid on Linux --
not thr_self(), which cannot tell non-pool threads apart and does not work on an alternate
signal stack).  Handler (Windows: vectored handler added *first*, before plat_win.s'
crash_handler; Linux: SIGSEGV / SIGBUS / SIGILL / SIGFPE / SIGTRAP with SA_SIGINFO | SA_ONSTACK):
only when the faulting thread has an active guard it unlinks the record (a second fault goes to
the enclosing guard or crashes normally), stores kind / code / pc / address and resumes the
thread at `plat_guard_fault_ip` with rsp / rbp of the guarded frame (Windows: CONTEXT; Linux:
ucontext + rt_sigreturn).  There `fninit`, the saved x87 control word, MXCSR, xmm6-15 and DF
are restored and EPILOG restores the caller's GPRs.  Without a guard the old handler runs
(crash report + exit 3) or the default action is re-raised.  Worker threads get a 64 KB
alternate signal stack on their first guarded call (stack overflows are handled on Linux).
Nesting works (PA_PARFOR items inside a guarded render run under their own guards on the
workers; the tag names the plugin).

### Limits (honest list)

* **Not a sandbox.**  Plugin code runs with all rights of the process; it can overwrite any
  memory (also the guard record, the host's heap blocks and globals), make syscalls, spin
  forever or install its own handlers.  The guard catches *faults*, nothing else.
* **Corrupt state stays corrupt.**  Memory a plugin wrote wrongly before faulting, blocks it
  leaked, a job left half done -- none of it is repaired.  The host drops the plugin's results
  (surface of a crashed decoder: zeroed, not freed; aux of a disabled effect: leaked) and never
  calls the plugin again.  mem_* keeps no global heap state (one OS mapping per block), so a
  crash cannot leave a heap lock behind; other locks held by host code the plugin called
  (thread pool spin lock during a nested PA_PARFOR) could stay held -- in practice they are
  only held for a few instructions without calls into plugin code.
* **Hangs** (endless loops) are not detected.
* **Windows stack overflow**: handled once per thread -- the guard page is not re-armed, a second
  overflow on the same thread terminates the process.
* Faults not raised as SEH exceptions / signals (fail-fast `__fastfail`, heap-corruption
  terminations by the OS, stack-buffer-overrun checks) end the process.
* Faults inside host code called from the plugin are attributed to the plugin (it asked for it).
* Linux: thread ids are reused; a new thread with a reused id gets its alternate stack on its
  first outermost guard anyway (sigaltstack is checked every time).

## 4. Runtime file formats (img, P1)

`IMGF_COUNT` is 22: IMGF 14..21 (`IMG_DYN0`, `IMG_NDYN` in img.inc) are free slots for
`img_register_dyn(name, exts, mime, props, codec) -> IMGF | 0`.  `img_format_info` returns 0 for a
free slot, `img_format_from_path` knows the registered extensions (after the built-in ones),
`img_detect` asks the registered probes after every built-in signature (hint first; a slot
without probe only by the hint), before the TGA heuristic, `img_filter_list` skips free slots.
Loops over `1..IMGF_COUNT-1` with `img_format_info` (fileio.s, layer.s) see the new formats
without changes -- as long as they are registered before the lists are built.

## 5. Menu lists

`MI_LIST base, list, max, ""` (empty placeholder) shows no row at all while the list is empty
(ui_popup.s, P1) -- used for the plugin rows of the Korrekturen / Effekte menus.
