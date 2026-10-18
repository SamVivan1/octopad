# OctoPad Tauri

Tauri 2 + Svelte + TypeScript rewrite of OctoPad.

## M0 Scaffold

This is the M0 scaffold - see `docs/superpowers/plans/2026-10-18-tauri-rewrite-m0.md` for full details.

## Development

```bash
pnpm install
pnpm tauri dev
```

## Build

```bash
pnpm tauri build
```

## Core API

- `octopad-core::version()` - Get core crate version
- `octopad-core::config_paths::legacy_config_dir()` - Get legacy config directory path
- `octopad-core::proto::*` - Protocol UUID constants
