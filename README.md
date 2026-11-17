# Calcite

A fast, native Minecraft launcher for macOS, Windows and Linux.

- **Native.** Rust and GPUI, no webview. Idles at about 0% CPU and a fraction of the RAM of webview launchers.
- **Everything you expect.** Instances, every mod loader, Modrinth modpacks, mods, resource packs and shaders, Microsoft accounts, managed Java, skins and capes with a 3D preview, Modrinth friends, shared instances, and settings kept the same across instances.
- **More than a launcher.** Health checks, a crash doctor, find-the-broken-mod, a live game monitor, snapshots with rollback, world backups, storage cleanup, moving in from other launchers, a server browser, and hosting your own Vanilla, Fabric or Paper server with a live console.
- **No ads, no purchases, no telemetry.**

## Installing

Each [release](https://github.com/ByteV0rtex/calcite/releases) has:

- **macOS:** `Calcite-<version>-macos-arm64.dmg`. Open it and drag Calcite into Applications. Builds aren't notarized yet, so the first time, right-click Calcite and choose Open.
- **Windows:** `Calcite-<version>-windows-x64-setup.exe`, or the portable `.zip`. The portable build keeps everything in a `data` folder next to `calcite.exe` (delete `portable.txt` to use the normal folder).
- **Linux:** `calcite_<version>_amd64.deb`, or the `.AppImage` for other distributions.

All of them open `.mrpack` modpacks with a double-click. Calcite writes a log of
its last run to `calcite.log` in its data folder.

## Building

You need Rust (stable), a JDK (17 or newer, used by the backend's build), and on
macOS the Xcode command line tools.

```bash
scripts/sync-backend.sh          # fetch + patch the launcher backend (version in BACKEND_VERSION)
cargo run -p calcite             # debug build, uses a separate "CalciteDev" data folder
cargo build --release -p calcite # release build, uses the "Calcite" data folder
```

Packaging (installers land in `dist/`):

```bash
scripts/package-macos.sh         # Calcite.app and a .dmg
scripts/package-linux.sh         # .deb, and an AppImage if appimagetool is installed
scripts/package-windows.ps1      # setup.exe (Inno Setup) and a portable .zip
scripts/make-icons.sh            # after changing the app icon SVG
```

Pushing a `v*` tag builds all of them in CI and attaches them to a draft release.

Development helpers:

```bash
cargo run -p calcite --features snapshot -- --snapshot library out.png   # render a page to PNG
CALCITE_DATA_ID=SomeOtherFolder cargo run -p calcite                      # pick a data folder
```

## How it's put together

Calcite's launching, installing and account code comes from Modrinth's
open-source launcher backend (`theseus`, GPL-3.0), which Calcite pins and
patches (`patches/`). The interface, the browsing client and every feature
beyond Modrinth App are Calcite's own.

```
crates/calcite        GPUI app: shell, pages, theme, image cache, backend bridge
crates/calcite-core   UI-independent logic (Modrinth API client, and later health
                      checks, crash doctor, snapshots, transfer from other launchers)
crates/calcite-login  small helper window for Microsoft sign-in
```

## License

GPL-3.0-only. Calcite isn't affiliated with Modrinth, Mojang or Microsoft.
Fonts: Inter and JetBrains Mono (SIL OFL). Icons: Lucide (ISC).
