# Third-party material included in AhaX

## OpenAI Codex fallback instructions

`official-codex-fallback-prompt.md` is an unmodified copy of the fallback coding-agent instructions from the OpenAI Codex source repository.

- Project: OpenAI Codex, copyright 2025 OpenAI.
- Repository: https://github.com/openai/codex
- Source file: `codex-rs/models-manager/prompt.md`
- Pinned commit: `c542fb93ef4b49c06e854e1cde9f892e8d3e8094`
- Commit date: October 3, 2026, 03:20:37 UTC.
- Retrieved: October 3, 2026.
- Source URL: https://raw.githubusercontent.com/openai/codex/c542fb93ef4b49c06e854e1cde9f892e8d3e8094/codex-rs/models-manager/prompt.md
- SHA-256: `ac8ae107a0d72fe3476b430afb161ea4e67da2e446d778aefc44828160559807`
- License: Apache License 2.0, reproduced in `LICENSE.openai-codex`.
- Upstream notice: reproduced without changes in `NOTICE.openai-codex`.

Vela embeds this text in generated Codex model catalogs because an authoritative custom catalog must provide model instructions. A short replacement instruction would remove Codex's normal coding guidance. This vendored fallback preserves the complete upstream fallback text; it is not a claim that every routed third-party model has the capabilities or model-specific instructions of an official model.

The source file, license, and upstream notice are unchanged. Review the upstream model catalog schema and fallback behavior before updating this pinned copy. Do not download replacement instructions at runtime.

## Thread storage dependencies

The thread subsystem uses the installed versions pinned in `Cargo.lock`:

- `rusqlite` 0.40.2, MIT; copyright 2014 The rusqlite developers. License included as `LICENSE.rusqlite`.
- Bundled SQLite through `libsqlite3-sys` 0.38.2. SQLite itself is in the public domain.
- `zstd` 0.13.3 and its Rust bindings, MIT. License included as `LICENSE.zstd-rs`.
- Zstandard 1.5.7 through `zstd-sys` 2.1.0, BSD-3-Clause; copyright Meta Platforms, Inc. and affiliates. License included as `LICENSE.zstandard`.

These libraries provide SQLite transactions/online backup and decoding of compressed local history. AhaX's inventory, snapshot manifests, recovery policy, protocol adapter, and interface are separate implementations.

## Native folder selection

- `tauri-plugin-dialog` 2.8.1 and its `tauri-plugin-fs` 2.6.0 dependency, used under the MIT license; copyright 2017 - Present Tauri Apps Contributors. License included as `LICENSE.tauri-plugins`.
- `rfd` 0.16.0, MIT; copyright 2022 Bartłomiej Maryńczak. License included as `LICENSE.rfd`.

The native folder picker uses the official Tauri dialog plugin. The main window has permission to open the dialog; no general filesystem read or write command permission is granted to the web frontend.
