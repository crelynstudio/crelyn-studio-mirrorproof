# Windows preview validation — 2026-10-29

MirrorProof 0.1.0 is a preview for testing and feedback. The owner authorized public source and preview distribution on 2026-10-29. This record describes the tested build; publication does not establish stable-release readiness.

## Size and performance

All MB values are decimal. One Windows x64 machine was used; timings are local measurements, not performance guarantees.

| Build | Executable bytes | Median scan time |
| --- | ---: | ---: |
| Previous development executable | 16,219,136 | Not compared |
| Release, speed optimization (`3`) | 8,684,544 | 803 ms |
| Release, size optimization (`s`) | 6,531,072 | 772 ms |
| Release, smallest-code optimization (`z`) | 5,493,248 | 1,632 ms |
| Selected: `z`, SHA-256 crate at `3` | 5,500,416 | 812 ms |

The workload contained 501 matching file pairs: one 256 MiB file per folder and 500 small files per folder. Each actual executable ran its windowless scheduled-scan command against isolated generated data. All 501 matches, result counts, full-match flag and 536,897,692 bytes read were checked. After one warm-up, three measured runs were performed without a concurrent build. Times include discovery/hash/final checks recorded by the engine, with warm filesystem caches. The selected build's median process wall time was 863 ms versus 864 ms for the speed build. Small timing differences should not be treated as significant.

The smallest unqualified `z` build was rejected because hashing took about twice as long. Optimizing `sha2` for speed costs 7,168 bytes and restores the measured hashing performance. The selected executable is 66.1% smaller than the old development executable. This is the smallest suitable configuration tested, not a proof of the smallest possible binary. Panic unwinding, error handling, safe path checks, icons, accessibility and licenses were retained.

Other changes: whole-program LTO, one codegen unit, stripped symbols, unused Tauri plugin-command removal and only the Windows library output actually required by the executable. The NSIS installer uses LZMA and current-user installation; the large WebView2 runtime is detected/downloaded separately when missing. The silver/white/graphite palette is unchanged.

## Final preview artifact

- Installer: `MirrorProof_0.1.0-preview_x64-setup.exe`, **1,705,719 bytes**.
- Installer SHA-256: `51f75d07864e56ab875930bb4fa19296ee0403b3b476e677131a66a031576c0e`.
- Installed executable: **5,500,416 bytes**.
- Installed executable SHA-256: `1ca2722fb8136fc1b82299869b44dd6495c9b6917874fd5b2cfa38a4591d17f5`.
- Full runtime/installer license inventory and MIT/third-party notices are installed with the app. The preview is unsigned; checksums do not establish publisher identity.

## Checks performed

- Production TypeScript/Vite build and NSIS package build passed.
- A separate clean source-only copy passed `npm ci`, the production frontend/type-check build and all seven frontend tests. That temporary copy and its dependency/cache outputs were then removed. A second clean Rust rebuild was not repeated after deleting the successfully tested build outputs.
- Optimized Rust library: **26 passed, 1 ignored scale test**. Frontend helper tests: **7 passed**.
- Rust formatting and Clippy with warnings denied passed.
- `npm audit --omit=dev` reported zero known runtime dependency vulnerabilities at the time checked. This is not a comprehensive security guarantee.
- The actual final NSIS package installed silently into an isolated workspace directory without elevation. Publisher/version metadata, installed binary, original icons and legal files were checked.
- The actual installed executable ran a headless comparison and returned exit code 2 for deliberately different folders.
- Its production UI ran on a separate non-foreground Windows desktop: offline reload, history reopening, interactive real comparison, filters, all Tools tabs, About and CSV/JSON/HTML export passed. No page errors or remote asset requests occurred. Source/backup contents and modification times were unchanged. README screenshots came from this installed build using generated example data.
- Silent uninstall removed the application, uninstall entry, shortcut and a real disabled current-user test task. It disabled the stored plan and preserved history and a deliberately added unknown file. Test installation preferences were restored and no test task was left behind.
- Generated build/dependency outputs were removed from the source repository after validation. Source, lockfiles, tests, used branding and legal notices were retained. Local compiler/SDK tools remain outside the repository for future development.

## Remaining checks

Broader user/device evaluation remains open. Windows 10 on separate hardware, real DPI changes, physical removable-drive disconnection, native folder/save dialogs, clipboard/Explorer interaction, installed notifications, SmartScreen reputation and first-time WebView2 download on a machine lacking the runtime remain manual checks. The existing 100,000-file ignored scale test was not repeated for size optimization. No signing certificate, updater, cloud dependency or PowerShell download-and-execute installer was added.
