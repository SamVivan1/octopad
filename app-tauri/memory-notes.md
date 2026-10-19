# Memory notes — OctoPad Tauri app

Measurements taken 2026-10-19 on the development machine (Ubuntu 26.10
`stonking`, X11 session, kernel 7.3), using `scripts/mem_probe.sh`.

## How to measure (and why it is not `ps` RSS)

The app runs **nine processes**: the main binary, `WebKitWebProcess`,
`WebKitNetworkProcess`, and the `glycin` / `bwrap` sandbox helpers WebKit uses to
decode SVG images. Summing RSS across them counts every shared page once per
process, so the sum overstates the physical memory used — roughly 1.5x here.

PSS (`/proc/<pid>/smaps_rollup`, `Pss:` line) splits shared pages among their
users and is the honest number. **Compare PSS against PSS.** The legacy Python app
is a single GTK process, so its RSS and PSS are nearly the same, which is why a
naive "sum of RSS" comparison against it is misleading.

```bash
pkill -x octopad-tauri; sleep 2
app-tauri/scripts/mem_probe.sh --label "current" -- app-tauri/src-tauri/target/release/octopad-tauri
```

The probe keeps the **worst** of three samples per run rather than a lucky one,
and repeats the whole cycle.

## Results

| Configuration | RSS sum | PSS sum | Verdict |
| --- | --- | --- | --- |
| Default (no flags) | 515 MiB | 258 MiB | baseline |
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | 440 MiB | 193 MiB | −65 MiB PSS |
| `WEBKIT_DISABLE_COMPOSITING_MODE=1` | 439 MiB | 193 MiB | −65 MiB PSS |
| both flags | 434 MiB | 185 MiB | kept |
| `LIBGL_ALWAYS_SOFTWARE=1` alone | 803 MiB | **526 MiB** | rejected, much worse |
| all three flags | 446 MiB | 194 MiB | software-GL adds nothing |
| **Shipped build (both flags baked into Rust)** | **432 MiB** | **181 MiB** | −77 MiB PSS (−30%) |

Shipped build across three full cycles: 189 / 181 / 176 MiB PSS.

Per-process breakdown of the shipped build (typical):

| Process | RSS | PSS |
| --- | --- | --- |
| `octopad-tauri` | 214 MiB | 113 MiB |
| `WebKitWebProcess` | 217 MiB | 121 MiB |
| `WebKitNetworkProcess` | 51 MiB | 17 MiB |
| `glycin-svg` + `bwrap` helpers | 26 MiB | 5 MiB |

## Rejected: `LIBGL_ALWAYS_SOFTWARE=1`

Setting software GL made the app **2x worse** on its own (PSS 526 MiB): llvmpipe
rasterises into large CPU-side buffers instead of handing them to the GPU. In one
sample of a three-flag run it appeared to help (165 MiB), and three repeats showed
that was noise. The flag is deliberately not set.

`WEBKIT_FORCE_SANDBOX=0` was also measured: no memory effect (185 MiB either way),
so it stays off — it would only weaken isolation.

## Where the flags live

`apply_webview_memory_env()` at the top of `app-tauri/src-tauri/src/lib.rs`,
called as the first statement of `run()`. WebKitGTK reads them once while the
webview initialises, so setting them later silently does nothing. Existing
environment values are respected, so a user can still override them:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=0 octopad-tauri   # opt back into the GPU renderer
```

## Comparison with the legacy app, honestly

The Python app was reported by the owner as **<100 MB**. That figure is a
single-process RSS and therefore also its PSS, so the comparison against our
181 MiB PSS is fair: the webview app uses roughly **1.8x** more physical memory.

This is inherent to WebKitGTK, not a bug in our code: a web-content process and a
GTK process simply cost more than one GTK process. The design spec's "<=100 MB"
target was written before anything was measured and is not achievable with a
web-based UI. The honest statement is: **not lighter than the app it replaces,
but ~30% lighter than its own unoptimised first build.**

If memory ever becomes the deciding factor, the options are a native GTK/Qt UI
(reverting the rewrite's UI layer, losing the shared design) or accepting the
difference. That is a decision for the owner, not something to quietly change in
the spec.
