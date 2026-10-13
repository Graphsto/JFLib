# Z1 — reusable deflate / inflate state, caller buffers, parallel PDN chunks

Problem: `zlib_deflate` / `gzip_encode` mapped and cleared ~340 KB of state (+ the output
buffer, ~600 KB for 256 KB PDN chunks) per call and `inflate_raw` ~10 KB — one
VirtualAlloc/mmap + page faults + free (TLB shootdown) each. History compression scaled
~3.3x on 12 threads (Linux: not at all), PDN / many-strip TIFF files were slow.

## 1. API (all additive; every classic function keeps its signature and its bytes)

deflate.s
```
deflate_state_new() -> rax st | 0      zeroed compressor state (DS_SIZE ~330 KB, mem_alloc);
                                       one thread at a time; also builds the shared tables
deflate_state_free(st)                 (st may be 0)
zlib_deflate_st(st, src, len, level, &outlen) -> rax buffer | 0          = zlib_deflate
deflate_raw_st(st, src, len, level, &outlen, head, tail) -> rax buffer | 0  = deflate_raw_ex
zlib_deflate_to(st, src, len, level, dst, cap) -> rax bytes | ERR_OVERFLOW | ERR_NOMEM
deflate_raw_to(st, src, len, level, dst, cap)  -> rax bytes | ERR_OVERFLOW | ERR_NOMEM
deflate_bound(len) -> rax              len + len/512 + 128: a cap with which the *_to
                                       functions (and gzip_encode_to) never overflow
```
inflate.s
```
inflate_state_new() -> rax st | 0      decoder state (IS_SIZE ~10 KB)
inflate_state_free(st)
inflate_raw_st(st, src, srclen, dst, dstcap, &consumed) -> rax bytes | ERR_*  = inflate_raw_ex
zlib_inflate_st(st, src, srclen, dst, dstcap) -> rax bytes | ERR_*           = zlib_inflate
```
gzip.s
```
gzip_decode_st(st, src, len, dst, dstcap, &consumed) -> rax      (inflate state)
gzip_encode_st(st, src, len, level, &out, &outlen) -> eax 0 / ERR_NOMEM   (deflate state)
gzip_encode_to(st, src, len, level, dst, cap) -> rax member bytes / ERR_OVERFLOW / ERR_NOMEM
```
core/thread.s (minimal additive, marked Z1)
```
thr_self() -> eax slot    0..n-1 on pool worker k, n (= worker count) on any other thread.
                          Per-thread arrays need thr_count() + 1 entries (<= THR_MAX + 1).
                          Workers are identified by their stack range (rsp of the worker's
                          first frame recorded in g_thr_stk; no TLS). All non-worker threads
                          share slot n -> guard it (busy flag) if two may run at once.
```
Rules: `st = 0` everywhere means "temporary state for this call" (the classic behaviour, the
only case with ERR_NOMEM in the *_to functions). A state belongs to one thread at a time.

**Output buffer policy (`*_to`).** No growing: the stream is written into dst[0..cap); the
compressor needs 8 bytes of slack after the data (8-byte stores), which also hold the zlib
Adler-32 / gzip trailer. ERR_OVERFLOW when it does not fit (dst contents undefined) — the
compressor stops at the first block that would end past the limit (block sizes are known
exactly before they are written), so a small cap doubles as a cheap "only if it shrinks"
test (history tiles: cap = ZMAX + 16). Exactly: raw fits iff bytes + 8 <= cap, zlib iff
bytes + 4 <= cap, gzip iff bytes <= cap (and cap >= 8 / 10 / 18).

## 2. How the reuse stays bit-identical

* **Deflate, no re-clearing of the 256 KB hash tables:** head/prev hold *global* positions
  (input offset + `DS_BASE`); every call starts `len + 64 KB` after the previous one. A head
  entry left by an earlier input therefore fails the 32 KB window check at once — exactly like
  the never-set (zero) entries of a fresh state, which also never produce a match (a different
  hash bucket means different first 3 bytes; a chain reaching "0" stops there). prev entries are
  only ever read for positions inserted by the current input. Per call only the block
  counters (LFREQ/DFREQ, 1.3 KB) and scalars are reset; head (128 KB) is cleared only when the
  positions would pass 0xFFFF0000 (about every 4 GB of input). The only code change in the
  matcher: `rsi = src - DS_BASE`, `r12 = DS_BASE`, `r13 = DS_BASE + len`.
* **Inflate:** per call only IS_PAD is reset; IS_FIXED stays valid (the fixed tables are
  built once per state: LIT/DIST are only written by the fixed path, which sets IS_FIXED = 1
  after the build, and the dynamic path, which clears it before); every other table is
  rebuilt before it is read, so an error (corrupt input) never affects the next call (tested:
  1500 valid/corrupt/truncated streams through one state, results = fresh calls).

## 3. Callers switched (marked "Z1")

* **apps/PaintCraft/src/hist_mem.s:** `zc_fn` uses the calling thread's deflate state
  (`hm_zslot`: slot `thr_self()`, busy flag for slot n, state made on first use and kept) and
  `zlib_deflate_to` into a batch buffer `g_zc_out` (ZBATCH x ZC_STRIDE ≈ 3.9 MB, allocated once
  on first compression and kept) with cap ZMAX + 16: ERR_OVERFLOW -> HTF_NOZ as before (too
  big), ERR_NOMEM -> retried later as before; the adoption no longer frees per-tile buffers.
  `ht_load` (2x) and `ld_fn` inflate through `hm_inflate_tile` (per-thread inflate state).
  Persistent memory: up to (threads + 1) x (330 KB + 10 KB) + 3.9 MB.
* **ASMLIB/src/img/pdn.s:** encoder: `pe_payload` compresses the chunks of a layer in batches
  of 2 x (threads + 1) (<= 64) on the pool (`pe_pchunk`: per-thread deflate state and gather
  record in `EC_PZ`, members via `gzip_encode_to` into the batch buffer) and writes them in
  index order; the zero-chunk cache (EC_ZGZ) keeps its semantics. Decoder (bonus): payloads
  with >= 2 chunks go through `px_par`: pass 1 walks the headers in file order exactly like the
  old loop (cancel, spans, index, seen bitmap), pass 2 decodes the recorded chunks in parallel
  (`px_pitem`: per-thread inflate state + scratch, px_scatter); the returned error is the one of
  the first chunk in file order, else the header error, else 0 — identical to the sequential
  loop (tested against the old build with 1000 single/double-fault files). Falls back to the
  sequential loop when memory (ID_MAXMEM, scratch > 16 MB) does not allow it. New context
  fields EC_PZ (EC_SIZE 512), PX_PD (PX_SIZE 176).
* **ASMLIB/src/img/tiff.s:** TC_ZST (inflate state per context, i.e. per parallel slot and
  the main context; made in tc_alloc_slot for Deflate, zeroed in the clones, freed in
  tc_free_slot) -> `inflate_raw_st`. Encoder: one deflate state + one output buffer
  (TE_ZST/TE_ZBUF, deflate_bound of a full strip) for all strips via `zlib_deflate_to`.
* **png.s:** unchanged (one deflate / inflate per image: no win).
* **gzip.s:** `gzip_encode` / `gzip_decode` are wrappers of the *_st versions (st 0).

## 4. Tests (all green, both targets, `--out build/z1`)

* New `tools/zlib_state_check.py` (+ codectest ops 300..304 in `apps/codectest/src/zlib_ops.s`,
  registered in codectest main.s: EXT_OPS 15):
  state 8845 (one state per sequence, 36 random-order sequences of 120 items covering
  6 x all 720 combinations of zlib/raw/gzip x levels -1..10 x
  empty / 1..4 bytes / 64K-boundary / random / text / 150K-200K multi-block / 1 MB inputs,
  both the *_st and the *_to API = fresh calls, Python-decodable), limit 1297 (caps around the
  exact size + tiny caps; outputs end at a guard page), inflate 3000 (1500 streams, 1102 of
  them errors, through one state, = fresh calls, also with st = 0), self 3 (thr_self: no two
  concurrent items on one slot, default / 1 / 3 threads), base 759 (720 zlib/raw/gzip streams
  + 39 PDN / TIFF-Deflate / PNG files byte-identical to the pre-Z1 build `build/z1base`),
  pdndec 1006 (5 samples + 1000 corrupted multi-chunk files: same doc record or error code as
  the pre-Z1 build), bench (below).
  `python tools/zlib_state_check.py --target both --out build/z1 --base build/z1base`
  (--base needs a build of the old sources; without it base/pdndec are skipped).
* codec_check (sums, inflate, fuzz, deflate, png, pngenc, bmp, bmpenc, imgio), png_check,
  pdn_check (samples, roundtrip, flat, legacy, v3, nrbf, fuzz 3853, user, dotnet, perf),
  imgmeta_check gzip/decex/encode/fuzz, tiff_check (gen, encode, meta, errors, par, fuzz,
  perf; earlier also wic/ffmpeg/libtiff with --ext): all PASS on win and linux.
* PaintCraft `tests/history.uis` (budget / deflate / spill / drop paths): exit 0 on win + linux.
* plattest --test threads: 0 failures on both (thread.s changed).
* asmcheck (deflate, inflate, gzip, pdn, tiff, hist_mem, thread, zlib_ops): 0 errors
  (12 BOUNDS warnings, pre-existing table walks; 2 new `assume`s: level 1..9 in defl_run,
  worker index in thr_self).

## 5. Measurements (Ryzen 5 3600, 6C/12T, machine shared with other agents: best of 4-7 runs)

History-style (op 302/303: 2961 16 KB tiles = one 4000x3000 layer, level 1, batches of 256 on
thr_parallel_for, adoption copy into a pool; "old" = zlib_deflate per tile as hist_mem did,
"new" = per-thread state + zlib_deflate_to into the batch buffer as hist_mem does now):

| deflate, ms            | win old | win new | linux old | linux new |
|---|---|---|---|---|
| noisy gradient, 1 thread | 730 | 426 | 1163 | 419 |
| noisy gradient, 12 threads | 319 | 113 | 1082 | 126 |
| random (all too big), 1 / 12 | 768 / 323 | 312 / 74 | 1134 / 1011 | 298 / 97 |
| smooth, 1 / 12 | 274 / 227 | 17 / 5 | 999 / 735 | 23 / 7 |
| inflate all tiles, 1 / 12 | 143 / 55 | 110 / 28 | 211 / 219 | 138 / 37 |

Scaling 1 -> 12 threads (noisy): old 2.3x (win) / 1.1x (linux), new 3.8x / 3.3x on the loaded
machine; the per-call allocation (not the clear) was the killer on Linux (munmap).

PDN (pdn_check perf, `build/z1base` = before):

| | win before | win after | linux before | linux after |
|---|---|---|---|---|
| 4000x3000x10 encode (level 1, 63.1 MB) | 2025-2197 ms | 514-538 ms | 2558-2648 ms | 694-799 ms |
| decode + verify of that file | 740-1074 ms | 118 ms | 1406-1951 ms | 240-270 ms |
| d_big.pdn decode | 153-179 ms | 27-33 ms | 236-250 ms | 35-37 ms |

TIFF 12 MP Deflate, 600 strips (tiff_check perf, best of 4): win 35 -> 27 ms (predictor 29 ->
24), linux 76 -> 35 ms (68 -> 34); one strip unchanged (~170-230 ms, single inflate).
PNG: unchanged code, identical bytes.

## 6. Shared files / integrator

* `ASMLIB/src/core/thread.s`: additive `thr_self` + `g_thr_stk` (worker records its rsp);
  please add to THREADS.md §core/thread.s and GUIDE "Worker threads" (per-thread arrays).
* `apps/codectest/src/main.s`: EXT_OPS 14 -> 15, `zlib_op` appended to ext_ops (marked Z1).
* CODECS.md §2.6: add the API of §1; the codecs (deflate/inflate/gzip) are thread-safe with
  one state per thread; mention `deflate_bound` and the *_to overflow rule.
* Regenerate API.md (`python tools/gendoc.py`): new public functions in deflate.s, inflate.s,
  gzip.s, thread.s.
* No change to jfgui.inc / img.inc / build.py.

## 7. Notes / open

* The level-1 compressor itself runs at ~115 MB/s per thread on noisy data; a faster level-1
  matcher (e.g. no chain walk, 4-byte hash) would be the next step for history compression.
* hist_mem keeps its per-thread states and the 3.9 MB batch buffer for the process lifetime
  (no shutdown hook in hist_mem); an `hm_shutdown` could free them.
* thr_self for non-worker threads is "n" for all of them; callers guard slot n with a busy
  flag (hist_mem: temporary state; pdn: spin, never contended within one encode/decode call).
