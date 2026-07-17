---
name: babelforge-executor
description: Install configure and operate BabelForge eXecutor bfx for Windows x64 PDF translation with BabelDOC and OpenAI-compatible models
---

# BabelForge eXecutor

`bfx` PDF translation CLI

## Check

- Require Windows x64
- Locate `bfx.exe` with `Get-Command bfx` or an installation path
- Run `bfx info`
- Run `bfx doctor` before provider usage

## Install

GitHub Releases

```text
https://github.com/xiaoze-cn/BabelForge/releases/latest
```

- Select the versioned `BabelForge-eXecutor-<version>-win-Setup.exe` asset from the latest release
- Download it to a temporary directory
- Inspect the Authenticode signature
- Tell the user when the installer is unsigned or invalid
- Report a missing release or asset and request a local installer path
- Do not invent another download URL or build from unrelated source
- Run the installer interactively by default

Unattended installation only when requested

```powershell
& $installer /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
```

After installation run `bfx info` and `bfx doctor`

The installer is per-user and adds its directory to the user `PATH`

## Update

```powershell
bfx update --check
bfx update --check --json
```

- Use this only to check the latest GitHub Release and its versioned installer URL
- It does not download or install a release
- BabelDOC is bundled with BFX and is updated only in a tested BFX release

## Config

```text
%LOCALAPPDATA%\BabelForge\eXecutor\config.toml
```

```powershell
bfx config
bfx config providers
bfx config presets
```

- Ask for provider name model ID base URL and API key before writing a model
- Never read print log or repeat API keys
- Prefer an environment variable reference
- Use a literal key only with explicit approval for local plaintext storage

```powershell
bfx config model set GPT5.5 --model gpt-5.5 --url https://api.example.com/v1 --key env:OPENAI_API_KEY
bfx config preset set Default --language "en->zh" --format Pair --watermark false --destination Same
bfx doctor
```

## Translate

Use `run` for a completed result in the current interaction

```powershell
bfx run "C:\path\to\paper.pdf" --model GPT5.5 --preset Default
bfx run "C:\path\to\paper.pdf" --model GPT5.5 --json
```

- Use `submit` only for an explicit queue request
- Do not start the hidden `worker` command unless explicitly requested
- Ask before choosing language format watermark or destination not specified by the user

## Replace

Use `replace` only after a successful translated output exists

```powershell
bfx replace "C:\path\to\paper.pdf" --keep
bfx replace "C:\path\to\paper.pdf" --remove
bfx replace "C:\path\to\paper.pdf" --undo
```

- Ask whether to keep the original before replacement
- Prefer `--keep`
- Use `--remove` only with explicit user approval
- Report resulting paths

## Tasks

```powershell
bfx get --limit 50
bfx check <task-id> --json
bfx stop <task-id>
bfx retry <task-id>
```

- Report task ID state and output path
- Inspect `bfx check <task-id> --error` before retrying failed tasks

## Troubleshoot

- Missing runtime means run the installed `bfx.exe` or reinstall from Setup
- Missing API key means configure a key or provide the referenced environment variable
- BFX normalizes provider URLs without `/v1`
- Use `bfx info` and `bfx doctor` for setup checks
- Do not translate a document only to test setup without user approval
