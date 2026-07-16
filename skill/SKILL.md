---
name: babelforge-executor
description: Install, configure, and operate BabelForge eXecutor (bfx), a Windows x64 command-line PDF translation tool bundled with BabelDOC. Use when a user asks Codex to install BFX, configure an OpenAI-compatible model provider, translate PDF files, inspect BFX tasks, or diagnose BFX setup.
---

# BabelForge eXecutor

Use `bfx` to translate PDF files through its bundled BabelDOC runtime and an OpenAI-compatible provider.

## Check Availability

1. Require Windows x64. Do not claim support for other platforms.
2. Locate `bfx.exe` with `Get-Command bfx` or a user-supplied installation path.
3. Run `bfx info`. A working installation reports both the BFX and BabelDOC versions.
4. Run `bfx doctor` before starting a translation that may incur provider charges.

## Install

Look for the current installer in the BabelForge GitHub Releases page:

```text
https://github.com/xiaoze-cn/BabelForge/releases/latest
```

Use the stable release asset URL when it exists:

```text
https://github.com/xiaoze-cn/BabelForge/releases/latest/download/BabelForge-eXecutor-win-Setup.exe
```

Download it to a temporary directory, inspect its Authenticode signature, and tell the user if it is unsigned or invalid before proceeding. If the release or matching asset does not exist, report that BFX has not been published yet and ask for a local installer path; do not invent another URL or build from unrelated source.

Run the installer interactively by default. Use this only when the user asks for unattended installation:

```powershell
& $installer /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
```

After installation, open a new shell or invoke the installed `bfx.exe` by its full path, then run `bfx info` and `bfx doctor`.

The installer is per-user. It installs BFX and its runtime, adds the installation directory to the user `PATH`, and registers an uninstall entry in Windows Installed apps.

## Configure a Provider

The default configuration file is:

```text
%LOCALAPPDATA%\BabelForge\eXecutor\config.toml
```

Inspect configuration without exposing secrets:

```powershell
bfx config
bfx config providers
bfx config presets
```

Collect the provider name, model ID, OpenAI-compatible base URL, and API key from the user before writing a model. Do not read, print, log, or repeat the key. Prefer an environment-variable reference when the user has one:

```powershell
bfx config model set GPT5.5 --model gpt-5.5 --url https://api.example.com/v1 --key env:OPENAI_API_KEY
bfx config preset set Default --language "en->zh" --format Pair --watermark false --destination Same
bfx doctor
```

Use a literal `--key` only when the user explicitly supplies and accepts local plaintext storage. BFX stores literal keys in its local configuration file.

## Translate PDFs

Use `bfx run` for a user who wants a completed result in the current interaction. It runs the translation immediately and is the default workflow.

```powershell
bfx run "C:\path\to\paper.pdf" --model GPT5.5 --preset Default
```

Use `--json` when the result needs structured parsing:

```powershell
bfx run "C:\path\to\paper.pdf" --model GPT5.5 --json
```

Use `bfx submit` only when the user explicitly requests queuing. Do not start the hidden `worker` command unless the user explicitly asks to operate a long-running queue worker.

Respect the configured preset unless the user requests an override. Ask before choosing a target language, output format, watermark setting, or output directory that was not specified.

## Inspect and Recover Tasks

```powershell
bfx get --limit 50
bfx check <task-id> --json
bfx stop <task-id>
bfx retry <task-id>
```

Report the task ID, state, and output path. For failed tasks, inspect `bfx check <task-id> --error` before proposing a retry.

## Troubleshoot

- `Cannot locate the BFX runtime`: Run the installed `bfx.exe`, not a copied bare executable. Reinstall from the Setup executable if the sibling `runtime` directory is missing.
- `doctor` reports no available API key: Configure a key or make the referenced environment variable available in the current process.
- A provider URL without `/v1` is accepted by BFX; it normalizes OpenAI-compatible base URLs.
- Do not translate a document merely to test setup. Use `bfx info` and `bfx doctor` unless the user explicitly authorizes a translation.
