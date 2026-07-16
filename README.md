# BabelForge eXecutor

PDF translation CLI powered by BabelDOC and OpenAI-compatible models

## Install

Windows x64 installer

```text
dist/BabelForge-eXecutor-win-Setup.exe
```

Run the installer and follow the wizard

## Config

```text
%LOCALAPPDATA%\BabelForge\eXecutor\config.toml
```

```toml
[Models."GPT5.4"]
Model = "gpt-5.4"
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
bfx doctor
```

## Translate

```powershell
bfx run paper.pdf --model GPT5.4
bfx run paper.pdf --model GPT5.4 --json
bfx submit paper.pdf --model GPT5.4

bfx get --limit 50
bfx check <task-id> --json
bfx stop <task-id>
bfx retry <task-id>
```

## Replace

```powershell
bfx replace paper.pdf --keep
bfx replace paper.pdf --remove
bfx replace paper.pdf --undo
```

## Codex Skill

Copy [`skill`](skill) to

```text
~/.codex/skills/babelforge-executor
```

## Build

```powershell
just setup
just package
```

## License

AGPL-3.0-or-later

See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
