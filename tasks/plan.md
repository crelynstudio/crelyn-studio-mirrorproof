# MirrorProof verification workspace expansion

User approved all ten proposed features and requested right titlebar alignment. English-only UI, metallic finish, background tests, offline operation and unchanged compared files remain required. No installer, updater or GitHub publishing.

## Implementation order

1. Center window controls; add exclusion rules and standalone HTML export to the real engine/report path.
2. Persist completed/partial reports safely outside compared roots. Open/import historical JSON with validation. Compare recorded results from the same roots/scope without presenting old results as current verification.
3. Reveal existing scanned files in Explorer through a bounded backend command. Add problems-only filtering and folder grouping.
4. Execute selected saved pairs sequentially, retaining independent outcomes/history; cancellation stops the queue.
5. Build/verify SHA-256 manifests using pinned read-only file handling. Validate imported paths/digests and preserve incomplete/scoped provenance.
6. Optional completion notifications and daily/weekly Windows Task Scheduler jobs. Schedules use a windowless executable mode, current-user privileges and validated stored plans. No task is registered by default.
7. Test native flows in a hidden app; unit-test parsers, report escaping, scopes, history, manifests, queues and scheduling. Update real screenshots/docs and build the normal development executable.

## Design and safety decisions

- Additional controls live in compact dialogs reached from the existing heading, keeping the primary window usable at its minimum size.
- History uses app-owned report files and lightweight metadata, not browser storage for large scans. Original compared roots are checked before any history write.
- Imported reports are bounded and validated. Status changes describe recorded results, not inferred changes to file contents.
- Excluded items remain visible as skipped and invalidate a whole-folder match claim. Scope travels with JSON/CSV/HTML/history/schedules.
- Native commands validate paths; Explorer only reveals an existing item from the loaded report. No shell interpolation of user paths.
- Manifests store real SHA-256 digests, include incomplete/excluded provenance, and never bypass file-content hashing during verification.
- Scheduled jobs run only for explicitly enabled plans; no elevation, hidden services, network calls or automatic file repair.
- Native notification limitations in an unpackaged Windows build are disclosed. Tests do not display notifications or launch external apps.

## Remaining physical checks

Windows 10 on a separate machine, actual DPI changes and removable-drive disconnection require hardware/manual checks. Emulation and generated filesystem faults are reported separately.

## Size optimization and installer (authorized next increment)

Compare speed-oriented, size-oriented and smallest-code Release profiles using the same actual SHA-256 workload and native executable sizes. Retain correctness/recovery behavior and select the smallest configuration without a material performance regression. Remove unused Tauri plugin commands and unused Windows library output types. Build an English current-user NSIS installer with LZMA compression, existing branding and WebView2 detection/download only when missing. Verify real production artifacts, install/uninstall in a bounded workspace location where possible, retain test evidence and checksums, then remove only known generated files. No publication, account access, ZIP archive or GitHub action before user review.
