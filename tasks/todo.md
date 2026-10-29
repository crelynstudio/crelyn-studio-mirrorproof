# Accepted expansion tasks

- [x] Center right titlebar controls and verify common vertical center.
- [x] Exclusions: apply to both trees, preserve scope, tests for glob rules and skipped status.
- [x] HTML export: actual full snapshot, escaped content, standalone styles, output protections.
- [x] History + JSON reopening/import: safe persistence, validation, historical timestamp.
- [x] Compare two scan results: same scope/roots, added/resolved/changed recorded outcomes.
- [x] Explorer reveal: bounded existing report item, no arbitrary command execution.
- [x] Problems filter and folder grouping: usable with search/pagination.
- [x] Queue saved pairs: ordered real scans, per-pair outcomes, cancellation stops remaining work.
- [x] SHA-256 manifests: real creation/verification, extra/missing/different/errors, hostile input tests.
- [x] Optional completion notifications: default off, honest development limitations.
- [x] Daily/weekly schedules: current-user native registration/removal, headless runner, no default tasks.
- [x] Hidden native integration, unit tests, build, documentation and real screenshots.

Verification: 26 Rust tests and 7 frontend unit tests passed; Clippy and Rust formatting passed. Hidden native integration covered all feature paths, real queued cancellation and real disabled Task Scheduler registration/removal plus the windowless runner. Test tasks remaining: zero. Real screenshots updated. Normal development executable built without test aliases or hidden-window overrides.

Still awaiting user evaluation. Physical USB disconnection, real Windows DPI changes, Windows 10 on separate hardware, native Explorer/clipboard/dialog interaction and actual notifications remain manual checks. The ignored 100,000-file scale test was not repeated for this increment. No installer or release package was created.

## Release size and installation

- [x] Measure comparable Release profiles and real hashing performance.
- [x] Select and test the smallest suitable production configuration.
- [x] Build branded current-user NSIS setup with LZMA and WebView2 prerequisite handling.
- [x] Validate optimized runtime, installer and uninstall behavior without foreground windows.
- [x] Record exact sizes/checksums, update source and user documentation.
- [x] Remove verified generated dependencies/caches and obsolete test artifacts; preserve sources/assets/tools needed for development.
- [x] Prepare a local preview package for user review; no GitHub upload or final release publication.
- [x] Owner authorization to publish the source and a public preview (2026-10-29).
- [ ] Broader user/device evaluation and stable-release approval.

See `docs/validation.md` for exact sizes, measured profile tradeoffs, optimized test results and actual silent install/uninstall checks. The earlier increment's note about having no installer is superseded by this preview. GitHub distribution was subsequently authorized; this remains an early release rather than a stable version.
