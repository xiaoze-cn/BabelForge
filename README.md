# BabelForge eXecutor

Translate PDFs while preserving their layout with a local BabelDOC runtime and an OpenAI-compatible model provider. BFX keeps the source PDF unchanged and writes the translated result beside it by default.

## Quick Start

The Windows installer includes BFX and BabelDOC, adds BFX to the current user's PATH, and registers an uninstall entry. Install it once, configure a model, then translate a PDF.

- Download the versioned `BabelForge-eXecutor-<version>-win-Setup.exe` asset from [GitHub Releases](https://github.com/xiaoze-cn/BabelForge/releases)
- Run the installer
- Configure a model
- Translate a PDF

```powershell
bfx config model set GPT5.5 --model gpt-5.5 --url https://api.example.com/v1 --key sk-xxxx
bfx doctor
bfx update --check
bfx run paper.pdf --model GPT5.5
```

## Common Commands

Use `run` when you want to wait for a translation to finish. Use `submit` when you want BFX to create a task and let its background worker process it.

```powershell
# Run now
bfx run paper.pdf --model GPT5.5
bfx run paper.pdf --model GPT5.5 --json

# Queue work and inspect its task
bfx submit paper.pdf --model GPT5.5
bfx get --limit 50
bfx check <task-id> --json
bfx stop <task-id>
bfx retry <task-id>

# Check the latest BFX release
bfx update --check
bfx update --check --json

# Replace a source PDF after a task has finished
bfx replace paper.pdf --keep
bfx replace paper.pdf --remove
bfx replace paper.pdf --undo
```

## Updates

`bfx update --check` compares the installed BFX version with the latest GitHub Release and prints the versioned Windows installer URL when an update is available. It does not download or install anything, and BabelDOC is updated only as part of a tested BFX release.

## Config

Models and presets are stored locally for the current Windows user, rather than in the installation directory. A literal key is supported, but an environment variable avoids leaving the secret in the config file.

```text
%LOCALAPPDATA%\BabelForge\eXecutor\config.toml
```

Keep API keys private and use an environment variable key when possible

```toml
[Models."GPT5.5"]
Model = "gpt-5.5"
URL = "https://api.example.com/v1"
Key = "sk-replace-with-your-key"

[Presets.Default]
Pages = "All"
Language = "en->zh"
Format = "Pair"
Destination = "Same"
Watermark = false
```

```powershell
bfx config
bfx config providers
bfx config presets
```

To use an environment variable, set it in Windows and pass its name with the `env:` prefix.

```powershell
$env:OPENAI_API_KEY = "sk-xxxx"
bfx config model set GPT5.5 --model gpt-5.5 --url https://api.example.com/v1 --key env:OPENAI_API_KEY
```

## Codex Skill

The bundled Codex skill lets Codex find BFX on GitHub Releases, configure a provider, translate PDFs, and manage tasks through the CLI. Install it by copying the [`skill`](skill) directory to `~/.codex/skills/babelforge-executor`.

## Development

Use Pixi for the development environment and Inno Setup to build the Windows installer.

```powershell
just setup
just package
```

## License

Licensed under [AGPL-3.0-or-later](LICENSE), with BabelDOC attribution and third-party licenses in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
