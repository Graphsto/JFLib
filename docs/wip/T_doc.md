# T: threads & platform basics — documentation fragment

For `ASMLIB/docs/DESIGN.md` §7 ("As built") and `ASMLIB/docs/GUIDE.md`. Public constants
(`FO_*`, `CPU_*`, `JOB_*`, `JS_*`, `THR_MAX`) live in the new include file
`ASMLIB/inc/thread.inc` (guarded, `=` definitions only, safe to include twice).
Until `jfgui.inc` includes it, users write `.include "thread.inc"` after `jfgui.inc`.

---------------------------------------------------------------------------------------------------
## DESIGN.md §7 — additions

### Platform: paths, environment, files (all thread safe)
```
plat_env(name, buf, cap) -> eax len | -1        UTF-8 value; -1 = not set or longer than cap-1 (buf "")
plat_setenv(name, value) -> eax 0 | -1          value 0 removes; seen by plat_getenv / plat_env.
                                                Linux: builds a new envp array (old ones stay valid,
                                                never freed); calls must not overlap each other
plat_config_dir(buf, cap) -> eax len            Windows %APPDATA%, else %USERPROFILE%/AppData/Roaming;
                                                Linux $XDG_CONFIG_HOME (only if absolute), else
                                                $HOME/.config. UTF-8, '/' separators, no trailing
                                                slash ("/" and "C:/" stay), 0 on failure
plat_temp_dir(buf, cap) -> eax len              GetTempPathW / $TMPDIR (absolute) else /tmp; same form
plat_mkdir(path) -> eax 0 | -1                  creates missing parents too; an existing directory
                                                is ok, an existing file is -1
plat_delete(path) -> eax 0 | -1                 file or empty directory (-1 when missing / not empty)
plat_rename(old, new) -> eax 0 | -1             replaces an existing target (Windows MoveFileExW
                                                REPLACE_EXISTING|COPY_ALLOWED|WRITE_THROUGH, Linux rename)
plat_file_open(path, mode) -> rax h | -1        FO_READ 0 (existing, read only), FO_WRITE 1
                                                (create/truncate, read+write), FO_RW 2 (open or
                                                create, contents kept, read+write)
plat_file_open_temp(dir) -> rax h | -1          new file in dir (0 = plat_temp_dir) that vanishes
                                                when closed or when the process dies: Windows
                                                CREATE_NEW + FILE_FLAG_DELETE_ON_CLOSE ("jfgui-*.tmp",
                                                visible while open), Linux O_TMPFILE, else
                                                O_CREAT|O_EXCL + immediate unlink (drvfs /mnt/c)
plat_file_read_at(h, off, buf, len) -> rax      bytes (short at EOF, 0 past it) | -1  (pread /
                                                ReadFile+OVERLAPPED; 1 GB chunks; no shared file
                                                position, so threads may share one handle)
plat_file_write_at(h, off, buf, len) -> rax     bytes | -1 (the file grows; gaps read as zeros)
plat_file_size(h) -> rax                        bytes | -1
plat_file_sync(h) -> eax 0 | -1                 FlushFileBuffers / fsync
plat_file_close(h)                              (h = -1 is ignored)
plat_mem_total() -> rax                         physical RAM in bytes (GlobalMemoryStatusEx /
                                                sysinfo), 0 unknown
plat_cpu_count() -> eax                         CPUs in the process affinity mask
                                                (GetProcessAffinityMask popcount, fallback
                                                GetSystemInfo / sched_getaffinity), >= 1
```
Files: `plat/win/win_sys.s`, `plat/linux/lx_sys.s` (+ `plat_setenv` in `plat_linux.s`).
Windows converts paths into heap buffers (no static buffers). The older `plat_file_read`,
`plat_file_write`, `plat_file_exists`, `plat_getenv`, `plat_dir_list`, `plat_cwd` of
plat_win.s / win_files.s still use static buffers: **UI thread only**.

### Platform: threads, semaphores, wake-up
```
plat_thread_create(fn, arg, stack) -> rax h|0   fn(arg): Win64 ABI (rcx = arg), rsp aligned like
                                                after a normal call; stack 0 = 1 MB (max 256 MB)
plat_thread_join(h)                             every thread must be joined (frees its stack)
plat_thread_yield()                             SwitchToThread / sched_yield
plat_sem_create(initial) -> rax h|0             counting semaphore
plat_sem_post(h, n)                             n <= 0: nothing
plat_sem_wait(h, ms) -> eax 0 | 1               1 = timeout; ms -1 (any negative) forever, 0 = try
plat_sem_destroy(h)
plat_wake()                                     any thread: the UI thread's plat_wait returns and an
                                                E_WAKE event is queued; bursts are coalesced
```
* Windows (`plat/win/win_thread.s`): CreateThread with a trampoline record; the stack size is
  passed as the *commit* size, so worker stacks are fully committed and functions with frames
  larger than a page need no stack probes on them. Semaphores = CreateSemaphoreW.
* Linux (`plat/linux/lx_thread.s`): mmap stack + PROT_NONE guard page, thread record at the top
  of the mapping, `clone(VM|FS|FILES|SIGHAND|THREAD|SYSVSEM|PARENT_SETTID|CHILD_CLEARTID)`, the
  child calls fn then `exit` (60). Join waits on the tid word with **shared** FUTEX_WAIT (the
  kernel's CHILD_CLEARTID wake is not private), then munmaps. Semaphores: u32 counter + waiter
  count, FUTEX_WAIT_PRIVATE / FUTEX_WAKE_PRIVATE, relative timeouts recomputed from a deadline.
  New threads have no sigaltstack; the crash handler then runs on the thread's stack.
* plat_wake: a flag (`lock xchg`) coalesces. Windows: with a window `PostMessageW(g_hwnd,
  WM_APP_WAKE = WM_APP+1)` handled in the wndproc (also inside modal loops); without one (headless,
  before plat_window_create, after WM_DESTROY) an auto-reset event created at start-up
  (`win_thread_init` from plat_entry); plat_wait checks it on every call and waits on it in
  MsgWaitForMultipleObjectsEx. Linux: a non-blocking eventfd2 created in linux_init
  (`lx_wake_init`) is part of the poll set of wl_plat_wait (last pollfd slot) and x11_plat_wait
  (`plat_wake_poll`); before a display backend is chosen (headless) `plat_wait` is
  `lx_wait_nodisp`, which sleeps on the eventfd until the deadline (it used to return 0 at once).
  Internal helpers: `plat_wake_consume` (UI thread: clear the flag, drain the fd, queue E_WAKE),
  `plat_wake_fd`, `plat_wake_poll(fd, events, ms)`.
* Measured (WSLg / Windows 11, Ryzen 5 3600): E_WAKE arrives 15–400 µs after plat_wake;
  100 plat_wake calls in a row give exactly one E_WAKE.

### core/thread.s — worker pool
```
thr_init(n) -> eax workers          n 0 = plat_cpu_count, max THR_MAX (64), `--threads N` wins;
                                    called lazily by everything below
thr_count() -> eax                  worker threads (the pool is started if needed)
thr_shutdown()                      join the workers (queue must be empty); a later call restarts
thr_parallel_for(fn, ctx, count) -> eax 0 | ERR_CANCEL
                                    fn(ctx, index, job) for 0..count-1 on the workers AND the
                                    caller; returns when all items are done; nested calls allowed
thr_job_start(job) -> eax 0 | -1    background job (fn, ctx, count set by the owner); -1 = running
thr_job_wait(job) -> eax 0 | ERR_CANCEL   blocks; the caller helps with items not yet taken
thr_job_cancel(job)                 items not started are skipped; also callable from inside fn
thr_job_done(job) -> eax            1 finished or never started, 0 queued / running
```
JOB record (`thread.inc`, 80 bytes): `JOB_FN` 0 fn(ctx, index, job), `JOB_CTX` 8, `JOB_COUNT` 16
(u64), `JOB_NEXT` 24 (internal, atomic), `JOB_DONE` 32 (items finished — progress =
DONE/COUNT), `JOB_USER` 40 (owner's), `JOB_CANCEL` 48 (u32, kernels poll it), `JOB_STATE` 52
(`JS_IDLE` 0 / `JS_RUNNING` 1 / `JS_DONE` 2), `JOB_ACTIVE` 56, `JOB_FLAGS` 60, `JOB_LINK` 64
(internal), `JOB_RSV` 72. The record must stay valid until thr_job_done returns 1; a finished
record can be started again.

Design: one run queue under a short spin lock (64 pause rounds, then yield) — parallel_for jobs
at the head, background jobs at the tail. A thread joins a job by incrementing JOB_ACTIVE under
the lock and then claims indexes with `lock xadd` on JOB_NEXT; whoever sees all indexes handed
out (or the cancel flag) unlinks the job; the last participant (JOB_ACTIVE → 0) finishes it:
`JOB_STATE = JS_DONE` (the owner may free the record from then on, nothing touches it again),
`plat_wake()` for background jobs, and every thread sleeping in a job wait is woken (waiters
register first, then check the state, then sleep on a semaphore). Idle workers spin 256 pause
rounds, then sleep on a work semaphore. Workers are started with a fully committed 1 MB stack.
Results depend only on the item index, so `--threads 1` and `--threads N` give identical output.

### core/ini.s — settings files
```
ini_new() -> rax INI|0              ini_load(path) -> rax INI|0 (missing file = empty; 0 = no memory)
ini_save(ini, path) -> eax 0|-1     path.tmp~ + plat_file_sync + plat_rename; plat_mkdir of the
                                    directory part first
ini_free(ini)
ini_get(ini, sec, key) -> rax       NUL-terminated value, valid until the next ini_set*/remove/free
ini_get_int(ini, sec, key, def) -> rax      decimal, 0x hex, true/false/yes/no/on/off; the whole
                                            value must match (overflow -> default)
ini_get_f64(ini, sec, key, def xmm3) -> xmm0
ini_set(ini, sec, key, str) -> eax 0|-1     sec/key/str may point into the same INI (ini_get)
ini_set_int(ini, sec, key, i64) -> eax      ini_set_f64(ini, sec, key, xmm3) -> eax
ini_remove(ini, sec, key) -> eax 0|-1
f64_format(buf, cap, xmm2) -> eax len       shortest text that reads back bit-exact
f64_parse(str) -> xmm0, rdx next            rdx = str when there is no number
```
Deviation from §0 ("floats never returned in xmm by public API"): `ini_get_f64`, `f64_parse`
return a double in xmm0 and `ini_set_f64` / `f64_format` / `ini_get_f64` take one in xmm2/xmm3
(Win64 positional float arguments).
f64 text uses the x87 unit with 64-bit precision (control word saved/restored): parsing keeps 18
significant digits and scales by an exact power of ten (up to 10^27); formatting tries 1..17
digits and keeps the first that parses back to the same bits. Tested: 40 fixed cases (identical
to Python's shortest repr in this format) and 20000 random doubles, bit-exact both ways.

### core/cpu.s
`cpu_features() -> eax` (`CPU_SSE41` 1, `CPU_AVX2` 2, `CPU_FMA` 4, `CPU_POPCNT` 8, `CPU_BMI2` 16;
AVX2/FMA only with OSXSAVE + XGETBV(0) bits 1|2; cached), `--simd sse2` forces 0 (`--simd
avx2` or nothing = detected), `cpu_brand(buf, cap) -> eax len` (CPUID 0x80000002..4, trimmed),
`popcount64(x) -> eax` (SWAR, no POPCNT needed).

### core/mem.s
No global state: `mem_alloc/free/realloc/size` are thread safe (a block must not be freed while
another thread uses it). Arenas are not synchronised — one owner at a time.

### Test app `apps/plattest`
`plattest --test threads|files|ini|cpu|all [--repeat N] [--threads N] [--simd sse2]`; the tests
run in on_init and exit with the number of failures (with a window unless `--shot`/`--script`,
so `--test threads` exercises the real plat_wait of Win32, Wayland and X11). Scratch files in
`build/plattest/<os>-<id>/` (removed afterwards); the config dir is redirected there before use.
`tests/*.uis` (`#!args --test X`) put all modes into `build.py test`.

---------------------------------------------------------------------------------------------------
## GUIDE.md — new sections

### Worker threads
Worker threads may call only code without global state:
* allowed: `mem_*`, the helpers of str.s (`mem_copy`, `str_*`, `utf8_*`, `parse_*`, `hash_fnv64`
  …), `fmt_buf` / `fmt_vbuf` (not `fmt_out` / `fmt_err`: static buffer), `plat_thread_*`,
  `plat_sem_*`, `plat_wake`, the streaming file API (`plat_file_open/_open_temp/_read_at/
  _write_at/_size/_sync/_close`), `plat_env`, `plat_mkdir`, `plat_delete`, `plat_rename`,
  `plat_time_us`, `plat_sleep_ms`, `plat_cpu_count`, `plat_mem_total`, `thr_*` (nested loops,
  jobs), `cpu_*`, `ini_*` and `f64_*` on INI objects that thread owns, pure kernels with explicit
  buffers;
* not allowed: `gfx_*`, `path_*`, `text_*` / fonts / glyph cache, `ui_*`, `evq_*`, the image
  codecs (until audited), `plat_file_read`, `plat_file_write`, `plat_file_exists`, `plat_getenv`,
  `plat_dir_list`, `plat_cwd`, `plat_home` (static buffers on Windows), `plat_setenv`, arenas
  shared with other threads, anything with `.bss` state.

A parallel loop (the caller works too and gets control back when everything is done):
```asm
# fn(ctx, index, job) -- one row per item; results depend only on the index
blur_row:
        PROLOG 0
        cmp     DWORD PTR [r8 + JOB_CANCEL], 0  # long kernels may poll the cancel flag
        jne     .br_out
        ...                                     # rcx = ctx, rdx = row
.br_out:
        EPILOG

        lea     rcx, [rip + blur_row]
        lea     rdx, [rip + g_blur_ctx]
        mov     r8d, [rip + g_height]
        call    thr_parallel_for                # eax 0 or ERR_CANCEL
```
A background job (the UI keeps running; the pool calls `plat_wake()` when it is finished, the
runtime delivers E_WAKE — check `thr_job_done` there, never block the UI thread in
`thr_job_wait` for long):
```asm
        lea     rbx, [rip + g_job]              # JOB_SIZE bytes, e.g. in .bss
        lea     rax, [rip + effect_tile]
        mov     [rbx + JOB_FN], rax
        mov     QWORD PTR [rbx + JOB_CTX], 0
        mov     QWORD PTR [rbx + JOB_COUNT], 256    # tiles
        mov     rcx, rbx
        call    thr_job_start
        ...
        mov     rcx, rbx                        # later, on E_WAKE: finished?
        call    thr_job_done
        # progress: [rbx + JOB_DONE] / [rbx + JOB_COUNT];  cancel: thr_job_cancel(job)
```
Command line: `--threads N` (worker count; 1 for debugging, results must not change),
`--simd sse2` (baseline kernels only).

### Settings files
```ini
; my app                     <- comments and unknown lines survive ini_save unchanged
[Window]
width = 1280                 <- spacing of untouched lines survives too
zoom=1.5
```
```asm
        call    plat_config_dir                 # (buf, cap): e.g. C:/Users/x/AppData/Roaming
        ... append "/MyApp/settings.ini" (str_cat)
        mov     rcx, rsi                        # path
        call    ini_load                        # never 0 unless out of memory
        mov     rbx, rax
        mov     rcx, rbx
        LSTR    rdx, "Window"
        LSTR    r8, "width"
        mov     r9d, 1024                       # default
        call    ini_get_int
        ...
        mov     rcx, rbx
        LSTR    rdx, "View"
        LSTR    r8, "zoom"
        movsd   xmm3, [rip + g_zoom]
        call    ini_set_f64                     # "1.5" -- always '.' as decimal point
        mov     rcx, rbx
        mov     rdx, rsi
        call    ini_save                        # temp file + rename, creates MyApp/
        mov     rcx, rbx
        call    ini_free
```
Rules: sections `[name]`, entries `key=value`, names compare ASCII case-insensitively, blanks
around names/values are trimmed, no quoting and no inline comments (values may contain ';' and
'#'), `sec` 0 / "" = keys before the first section, the first of repeated keys wins, a repeated
section header continues the section. Malformed lines, lines with NUL bytes and lines over 64 KB
are kept verbatim but not interpreted. New keys go after the last key of their section, new
sections to the end. LF / CRLF read; written CRLF on Windows, LF on Linux; a UTF-8 BOM is
skipped. Keys must not contain '=' / CR / LF or start with ';' '#' '['; section names must not
contain ']' / CR / LF; CR/LF in values become blanks.

---------------------------------------------------------------------------------------------------
## Integrator notes
* `jfgui.inc` should `.include "thread.inc"` (end of file) so apps get `JOB_*`, `FO_*`, `CPU_*`.
* Regenerate `ASMLIB/docs/API.md` (`python tools/gendoc.py`): new modules core/thread.s, ini.s,
  cpu.s, plat/win/win_thread.s, win_sys.s, plat/linux/lx_thread.s, lx_sys.s.
* Naming: the Linux files are `lx_thread.s` / `lx_sys.s`, not `thread.s`: build.py archives all
  objects with llvm-ar by basename, and a second `thread.o` would replace core/thread.o.
* W (runtime): E_WAKE → `on_command(UIC_WAKE)`; the platform already guarantees one E_WAKE per
  burst of plat_wake calls, also in headless runs (plat_wait without a display now sleeps until
  its deadline or a wake-up instead of returning 0 at once).
