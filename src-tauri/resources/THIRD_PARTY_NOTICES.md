# Third-party material included in Vela

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
