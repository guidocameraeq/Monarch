# Monarch — personal fork

A personal fork of [**Nuzair46/Monarch**](https://github.com/Nuzair46/Monarch), a Windows display/output manager that attaches and detaches monitors through the Windows CCD API (Tauri 2 — Rust + React).

I forked it because two bugs made it unusable on my setup — a 3-display desktop (two monitors + an HDMI "Smart TV"). Both are fixed and verified in daily use, and the fixes have been contributed back upstream.

## What I fixed

- **Detached displays wouldn't come back.** An HDMI output that had been detached (my Smart TV) could not be re-attached, and displays were lost across reboots. Fixed by seeding connected-but-inactive outputs from `QDC_ALL_PATHS` and recovering with an **explicit attach → topology-extend → DisplaySwitch** escalation — each step dry-run-validated with `SDC_VALIDATE` and rolled back if the topology doesn't settle.
- **The tray and UI froze after sleep or hibernate.** Fixed by moving the display pipeline off the main thread and listening to `WM_POWERBROADCAST` / `WM_DISPLAYCHANGE` on a hidden window, with bounded waits on external helpers.
- **`force_topology_extend` always failed with `ERROR_INVALID_PARAMETER` (87)** because of an illegal `SDC_*` flag combination. Fixed the flags and added `tools/probe-sdc-flags.ps1` to reproduce the validation with `SDC_VALIDATE` (which validates without applying anything).

## Contributed upstream

- [**#43**](https://github.com/Nuzair46/Monarch/pull/43) — the `force_topology_extend` flag fix.
- [**#44**](https://github.com/Nuzair46/Monarch/pull/44) — the full set (detached-display recovery, tray-freeze fix, diagnostics), built on top of the maintainer's PR #30 groundwork.

## Notes

- The working branch is [`personal`](../../tree/personal).
- Verified in the field on a single 3-display machine (single GPU/driver). The recovery paths are guarded at runtime by a mandatory `SDC_VALIDATE` dry-run before any real change, and by re-enumerating to confirm the result instead of trusting the `SetDisplayConfig` return code — never assumed, always checked.
- The display engine from this fork was later migrated into a separate app of mine (**Millennium**) as a native module.

## Credit & license

Original project by [**Nuzair46**](https://github.com/Nuzair46/Monarch). Released under the MIT License — see [`LICENSE`](LICENSE).
