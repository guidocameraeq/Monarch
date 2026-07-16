<center>
  <h1 align="center">Monarch</h1>
  <h4 align="center">Detach, restore, and switch monitor layouts without touching cables.</h4>
  <h5 align="center">Built for fast display switching, standby behavior, and safe rollback if something goes wrong</h5>
  <p align="center">
    <a href="https://github.com/Nuzair46/Monarch/releases">
      <img src="src-tauri/icons/icon.png" alt="Monarch logo" width="180" />
    </a>
  </p>
</center>

<p align="center">
  <a href="https://github.com/Nuzair46/Monarch/actions/workflows/ci-build-release.yml"><img alt="Release Build and Publish" src="https://github.com/Nuzair46/Monarch/actions/workflows/ci-build-release.yml/badge.svg" /></a>
  <img alt="Downloads" src="https://img.shields.io/github/downloads/Nuzair46/Monarch/total.svg" />
  <img alt="Latest Release" src="https://img.shields.io/github/v/release/Nuzair46/Monarch?display_name=tag" />
  <img alt="Platform" src="https://img.shields.io/badge/Platform-Windows%2010%2F11-0078D4?logo=windows&logoColor=white" />
</p>

<p align="center">
  <a href="https://github.com/guidocameraeq/Monarch/releases"><strong>Download Latest Release</strong></a>
  ·
  <a href="#quick-start"><strong>Quick Start</strong></a>
  ·
  <a href="#if-something-goes-wrong"><strong>Recovery</strong></a>
</p>

---

> ## 🔱 This is a personal fork
>
> Fork of **[Nuzair46/Monarch](https://github.com/Nuzair46/Monarch)** (MIT) that fixes the two bugs
> that made it unusable on my setup. Same app, same UI — it identifies itself as
> **Monarch (personal)**, version **51.0.0**, so it can never be confused with upstream's 1.5.x.
>
> **This branch is upstream `main` + the author's own [PR #30](https://github.com/Nuzair46/Monarch/pull/30) (still open upstream) + my fixes.**
> Credit where it belongs: PR #30 already moved half the tray pipeline off the main thread, added
> the `QDC_DATABASE_CURRENT` enrichment that makes a detached display visible at all, and wrote the
> diagnostics log that every fix below came out of. This fork stands on it.
>
> **Two bugs, both fixed and confirmed on real hardware:**
>
> 1. **The tray froze after sleep** — the entire display pipeline ran on the main event-loop
>    thread and held the global state mutex across `SetDisplayConfig`, child-process waits and
>    out-of-process COM. One stall and the app was gone; only Task Manager got you out.
> 2. **A detached display was lost after a reboot** — and the recovery meant to rescue it,
>    `force_topology_extend()`, called `SetDisplayConfig` with an **invalid flag combination**,
>    returning `87` (`ERROR_INVALID_PARAMETER`). Measured on both machines I could test (a
>    single-display laptop and a 3-display desktop); the probe's differential — `EXTEND|ALLOW` → 87
>    while `EXTEND` alone → 31, and only a request that *was* evaluated against the hardware can
>    return 31 — says the 87 is a parameter-level rejection that never reaches the driver, so it
>    should fail identically anywhere. That rescue had, as far as I can tell, never run. Offered
>    upstream as [#43](https://github.com/Nuzair46/Monarch/pull/43); the fix here goes further and
>    attaches the target explicitly instead of asking Windows to extend.
>
> Along the way we found that **Microsoft's documentation is wrong**: it states `SDC_ALLOW_CHANGES`
> *"is allowed with any other valid combination"*, and it is not — it is rejected with every
> `SDC_TOPOLOGY_*` flag. Run [`tools/probe-sdc-flags.ps1`](tools/probe-sdc-flags.ps1) to see it
> yourself; it uses `SDC_VALIDATE`, so it applies nothing and touches no displays.
>
> **Why each line is the way it is:** [`docs/DECISIONS.md`](docs/DECISIONS.md).
> **Where we left off, and what is *not* verified:** [`docs/SESSION_HANDOFF.md`](docs/SESSION_HANDOFF.md).
>
> Everything below is upstream's original README.

---

## What Is Monarch?

Monarch lets you:

- Detach a monitor in software (no cable unplugging)
- Reattach it later
- Save display layouts as profiles
- Restore the previous layout quickly
- Recover automatically with a confirmation timeout if a layout change goes wrong
- Easy apply with hotkeys

It uses Windows display topology APIs (`DisplayConfig`) to change which outputs are active.

## Download & Install (End Users)

1. Open the [Releases page](https://github.com/guidocameraeq/Monarch/releases) (this fork)
2. Download the latest `.msi` installer
3. **Quit any running Monarch from the tray first.** It starts hidden with Windows and holds a
   single-instance lock: a running old copy will take over the new one's window, and you will
   think you are testing the new build when you are not.
4. Run the installer
5. Launch `Monarch` from Start Menu or Desktop, and check the header says **`MONARCH (personal)
   v51.0.0`**. If it does not, you are running a different build.

## Quick Start

1. Open `Monarch`
2. In the `Monitors` section, click `Detach` on the display you want to turn off
3. Confirm the layout change (or it auto-rolls back)
4. Click `Attach` later to bring the display back
5. Use `Save Current Layout` in `Profiles` to store common setups

## Command-Line Profile Switch (Automation)

You can launch Monarch and ask it to apply a specific profile immediately:

```powershell
monarch-desktop.exe -profile "ProfileName"
```

Also supported:

```powershell
monarch-desktop.exe --profile "ProfileName"
monarch-desktop.exe --profile="ProfileName"
```

Notes:

- Useful for tools like Playnite scripts (before/after game launch)
- If Monarch is already running, the new command forwards the profile request to the running instance
- CLI profile argument takes precedence over the configured launch profile in Settings

## Safety Features

- Confirmation timer after layout changes
- Automatic rollback if you do not confirm in time
- `Restore Last Layout` action
- Prevents disabling the last active display

## If Something Goes Wrong

Try these in order:

1. Use Monarch tray menu: `Restore Displays`
2. Reopen Monarch and use `Restore Last Layout`
3. Use Windows shortcut `Win + P` and choose `Extend` or `PC screen only`
4. Reboot Windows (usually restores a usable display state)

## Notes (Important)

- Windows only
- Monarch changes display topology, not monitor power directly
- Most monitors enter standby when Windows stops sending signal
- If you change HDR/SDR mode in Windows, Monarch auto-reapplies calibration in the background (best effort)

## Troubleshooting

### The app opens but I can't see the window

- Check the system tray for the Monarch icon
- Double-click the tray icon or use `Open App`

### A layout change made the screen unusable

- Wait for the confirmation timer to expire (auto rollback)
- Or use `Win + P`

### My display arrangement in the UI looks outdated

- Refocus the app window (Monarch auto-refreshes)
- Wait a few seconds for the background refresh poll to update the layout

### Color calibration looks wrong after detaching a display

- Known issue on some systems with custom calibration (ICC / SDR / HDR calibration profiles)
- In testing, this can be triggered when:
  - a display is detached in Monarch, and then
  - Windows `Settings > System > Display` is opened
- The detach itself may look fine until Windows Display Settings is opened
- Workaround: reattach the detached display (this often restores the remaining display calibration)
- If needed, also reapply your calibration using your normal calibration tool / workflow

## FAQ

### Does Monarch physically power off the monitor?

No. It detaches the display output in Windows. Many monitors then enter standby automatically.

### Is it safe to test?

Yes, but test on a non-critical setup first. Monarch includes rollback protection, and `Win + P` / reboot are reliable fallbacks.

### Can I use it with NVIDIA / AMD / Intel?

Yes. Monarch is designed to work through Windows display APIs, not vendor-specific GPU control panels.

### Is color calibration perfectly preserved in every Windows display-settings scenario?

Not yet. Monarch handles many calibration cases (including common HDR/SDR transitions), but Windows Display Settings can still cause calibration resets on some systems after topology changes. See `Troubleshooting` for the current known issue and workaround.

## For Developers

<details>
  <summary>Build / Dev / CI details</summary>

### Project Layout

- `src/` Rust core library (layouts, profiles, rollback safety, persistence)
- `src-tauri/` Tauri desktop app + Windows backend
- `web/` React UI
- `.github/workflows/` Windows release workflow

### Build Locally (Windows)

Requirements:

- Node.js 20+
- `yarn`
- Rust (stable)
- Visual Studio Build Tools 2022 + Windows SDK (`rc.exe`)

Commands:

```bash
yarn install
rustup target add x86_64-pc-windows-msvc
yarn tauri dev
```

Build MSI:

```bash
yarn tauri build --bundles msi
```

Output:

- `src-tauri/target/release/bundle/msi/`

### CI / Release

- Workflow: `.github/workflows/ci-build-release.yml`
- Manual release workflow runs via `workflow_dispatch` and takes a version input
- Release pipeline updates these files together before building:
  - `Cargo.toml`
  - `src-tauri/Cargo.toml`
  - `package.json`
  - `src-tauri/tauri.conf.json`
- Release pipeline commits the version bump, creates tag `vX.Y.Z`, builds the Windows installer, and publishes the GitHub Release

Release process:

1. Make sure your release commit is on `main`.
2. Open `Actions` -> `Release Build and Publish` -> `Run workflow`.
3. Enter a version (example: `0.2.0`) or bump kind (`patch`, `minor`, `major`).
4. Run the workflow.
5. The workflow will bump all version files, commit the change, create the tag, build Windows artifacts, and publish the GitHub Release.

  </details>
