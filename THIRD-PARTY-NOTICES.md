# Third-party notices

MirrorProof is developed by Crelyn Studio. Source code is licensed under MIT; Crelyn Studio and third-party names/logos retain their separate brand and trademark rights.

The application uses these direct dependencies. Each remains under its upstream license:

| Component | License | Source |
| --- | --- | --- |
| Tauri and Tauri plugins | MIT OR Apache-2.0 | https://github.com/tauri-apps/tauri and https://github.com/tauri-apps/plugins-workspace |
| React / React DOM | MIT | https://github.com/facebook/react |
| Lucide icons | ISC | https://github.com/lucide-icons/lucide |
| Serde / serde_json | MIT OR Apache-2.0 | https://github.com/serde-rs/serde and https://github.com/serde-rs/json |
| RustCrypto SHA-2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| Simple Icons platform SVGs | CC0-1.0; separate brand rights apply | https://github.com/simple-icons/simple-icons/tree/16.36.0 |

The bundled `ThirdPartyLicenses.html` lists the resolved runtime Rust/JavaScript dependencies and contains their full license texts, together with the NSIS installer and Tauri NSIS utilities licenses. Build/test-only packages are not distributed as application runtime code. Microsoft Edge WebView2 is provided/installed separately under Microsoft's terms and is not bundled in MirrorProof's payload.
