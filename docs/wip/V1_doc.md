# V1: AVX2 kernels + asmcheck VEX support

Copy of ASMLIB/docs/modules/KERNELS.md §2.9 plus the asmcheck changes.

### 2.9 SIMD dispatch (AVX2) — V1

Rules for every kernel with an AVX2 variant:
* **Selection:** `cpu_features() & CPU_AVX2` (core/cpu.s: CPUID + OSXSAVE/XGETBV, cached on
  the first call, computed identically by every thread). `--simd sse2` on the command line
  forces the SSE2 baseline (`--simd avx2` / nothing = detected). The value never changes after
  the first call, so all rows of a job use the same kernels; dispatch per call (a table pointer
  or a tail jump), no mutable global state.
* **Bit-identical to SSE2:** same integer / f32 operations per lane in the same order, no FMA
  contraction (`vfmadd*` rounds once, SSE2 mulps + addps twice), same MXCSR rounding. Fast paths must be exact shortcuts of the formula (then the AVX2 grouping of 4
  pixels instead of 2 cannot change a result). Remainders (n mod 4) go to the SSE2 code.
* **Kept only at >= 1.5x** on the measured path (Ryzen 5 3600, both targets); otherwise SSE2.
* **Code rules:** `vzeroupper` before every call / ret / tail jump and before legacy SSE code
  after 256-bit instructions (asmcheck VZU); ymm stack slots with `vmovdqu` (the stack is only
  16-byte aligned; `vmovdqa` needs operand-width alignment: ALIGN); writing ymm6–15 clobbers the
  callee-saved xmm6–15 (PROLOGX / SAVE); 32-byte constants for ymm memory operands (BOUNDS).
* **Tests:** `python tools/blend_check.py --only simd` runs every gfx kernel with `--simd sse2`
  and with the default dispatch on mixed rows (n = 0..67, 1000, 4099, random alignments, guard
  bytes) and compares the bytes, then prints the speed-up; `--simd sse2` runs any suite on SSE2.

Implemented: **blend modes** (`k_bm_row`, `k_bm_cov_row`, `k_bm_solid_row`, modes 0..13):
the `F_*` / `BM_DIV` / `BM_GEN` macros are written once with `VB` / `VU` helpers (BMV = 0:
legacy 2-operand SSE2 on xmm, unchanged machine code; BMV = 1: VEX 3-operand on ymm) and
instantiated twice; `BM_FUNC_AVX` does 4 pixels per step (one pixel pair per 128-bit lane:
`vpmovzxbw` in, `vpackuswb` + `vpermq 0x08` out) with per-4-pixel fast paths, rows >= 8
pixels. Speed-up (sse2 → avx2, 1 Mpx rows, best of 3): Windows random 1.6–2.0x, opaque
1.6–2.4x, coverage + opacity 1.7–2.0x; Linux 1.4–2.3x (e.g. Normal random 199 → 384 Mpx/s,
opaque 1107 → 2035). BM_OVERWRITE stays SSE2 (AVX2 1.2–1.4x).

Measured and not converted (< 1.5x end to end):
* Gaussian blur r = 20 (fx_conv `fxc_box_h` / `fxc_box_v`, 3-box cascade): AVX2 variants were
  bit-identical, but box_h gained nothing (the running sum is latency bound; the 2-pixel prefix
  form needs lane-crossing shuffles), box_v 1.4x, the whole effect 1.1–1.2x (4000x3000:
  245–270 ms → 228–270 ms); time is spread over load / emit conversions (rs_to_work /
  rs_from_work), FIR/box passes and memory. Removed again.
* Resize (resample.s): profile lanczos3 2:1 = rs_hpass 32 %, rs_to_pairs 22 %, rs_vpass 15 %,
  rs_from_work 8 % (instructions). hpass computes one output pixel per pmaddwd chain from
  pair records (two outputs per ymm need two inserts per tap: no fewer uops); only vpass maps
  well to ymm, too small a share for 1.5x.
* `k_view_row` (zoom display): 256 K px rows: transparent 1414, opaque 1495, random 727 Mpx/s
  — the scalar checkerboard pre-fill (view_fill) costs about as much as the mix, so AVX2 on the
  mix gives at most ~1.3x; a cheaper fill (skip it for opaque groups) would help more.
* `k_premul_row`, `k_lerp_mask_row`, `k_unpremul_row`: off the interactive paths (I/O,
  selection apply after an effect); not converted.

## asmcheck: VEX / AVX (tools/asmcheck)

* semantics.py `_vex`: every `v*` mnemonic is dst <- f(src1, src2, ...): the destination is
  written, not read (VEX.128 zeroes the upper ymm half); exceptions: FMA / VNNI accumulate
  (dst read), gathers (dst + mask read/written), `vzeroupper` (no tracked value changes),
  `vzeroall` (writes xmm0-15 -> SAVE flags it in ABI functions), compares (`vptest`, `vucomis*`
  ...) only read and set flags; zero idioms `vpxor x, y, y` etc.; scalar merges (`vcvtsi2ss`,
  `vsqrtss`, `vmovss` reg form ...) do not read src1 (like the legacy forms); register copies
  (`vmovdqa ymm, ymm`) propagate values. ymm registers share the xmm numbers (a ymm6 write
  clobbers callee-saved xmm6: SAVE); memory operands keep their width (ymmword = 32 bytes for
  FRAME / BOUNDS / UNINIT of stack bytes); VEX spills of saved registers are tracked.
* ALIGN: `vmovdqa/vmovaps/vmovapd/vmovnt*` need operand-width alignment (16 or 32; a 32-byte
  aligned stack access is always an error: the ABI only guarantees 16), other VEX memory
  operands need none (no false positives for `vpaddd ymm0, ymm1, [rsp+72]`).
* New check **VZU** (error): call / ret / tail jump / legacy SSE instruction while the upper ymm
  state may be dirty (a 256-bit VEX instruction ran on some path without `vzeroupper` /
  `vzeroall` since). Error, not warning: the fix is one instruction, the analysis has no false
  positives (it only tracks "ymm register touched"), and the cost is non-local (every later
  legacy SSE instruction -- all SSE2 kernels, other modules, the OS -- pays a state transition /
  false dependency, typically 2-4x slower SSE code until the next vzeroupper), which no local
  profile would reveal. Intentional cases (an AVX helper called with dirty state on purpose):
  `# asmcheck: ok VZU reason`.
* Self test tools/asmcheck/tests/vex.s (3-operand reads, FMA, zero idioms, SAVE of ymm6, VEX
  save/restore, ALIGN 16/32 stack + global, VZU ret / call / legacy SSE / one-path vzeroupper /
  VEX.128 only / suppression).

## Suggested CLAUDE.md addition (section asmcheck / conventions)

* AVX2 code (VEX `v*`, ymm): only behind `cpu_features() & CPU_AVX2` with an SSE2 fallback,
  bit-identical results (no FMA where SSE2 multiplies and adds separately), `--simd sse2`
  forces the baseline; `vzeroupper` before every call / ret / tail jump and before legacy SSE
  instructions after 256-bit code (asmcheck VZU, error); ymm stack slots with `vmovdqu` (the
  stack is only 16-byte aligned); writing ymm6-15 clobbers callee-saved xmm6-15 (PROLOGX).
  Rules and measurements: ASMLIB/docs/modules/KERNELS.md §2.9.
