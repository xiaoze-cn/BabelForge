# BabelForge eXecutor

BabelForge eXecutor (`bfx`) is a command-line interface for translating PDF files through BabelDoc

## Install

The Windows installer is published as:

```text
dist/BabelForge-eXecutor-win-Setup.exe

# Quiet installation
.\dist\BabelForge-eXecutor-win-Setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART

# Choose an installation directory
.\dist\BabelForge-eXecutor-win-Setup.exe /DIR="C:\Tools\bfx"
```

## Configuration

By default, BFX stores the user configuration at:

```
%LOCALAPPDATA%\BabelForge\eXecutor\config.toml
```

Example `config.toml`:

```toml
[Models."GPT5.5"]
Model = "gpt-5.5"
URL = "https://api.example.com/v1"
Key = "sk-xxxx"         # env:BFX_API_KEY

[Presets.Default]
Pages = "All"
Language = "en->zh"     # "zh->en"
Format = "Pair"         # "Both" or "Mono"
Destination = "Same"    # "C:\Wiki\Papers"
Watermark = false       # true
```


```powershell
bfx config
bfx config providers
bfx config presets

bfx config model set GPT5.5 --model gpt-5.5 --url https://api.example.com/v1 --key sk-xxxx
bfx config preset set Default --language "en->zh" --format Pair --watermark false --destination Same
```

## Usage

```powershell
# Verify the configured provider and bundled BabelDOC runtime
bfx doctor

# Translate immediately
bfx run paper.pdf --model GPT5.4
bfx run C:/Wiki/Papers --model GPT5.4 --preset Default

# Queue work
bfx submit paper.pdf --model GPT5.4

# Inspect and manage tasks
bfx get --limit 50
bfx check 20260715-153042
bfx stop 20260715-153042
bfx retry 20260715-153042

# JSON output where supported
bfx get --json
bfx check 20260715-153042 --json
bfx run paper.pdf --model GPT5.4 --json
```

## Codex Skill

The repository includes [`skill`](skill). Install that folder into `~/.codex/skills/babelforge-executor` to let Codex install, configure, and operate BFX.

## Build

Install the Pixi environment and Inno Setup 7, then build the Windows x64 installer:

```powershell
just setup
just package
```

`just package` uses `C:\Program Files\Inno Setup 7\ISCC.exe` by default. Override it when needed:

```powershell
$env:BFX_INNO = 'C:\Program Files\Inno Setup 7\ISCC.exe'
just package
```

## License

BabelForge eXecutor is licensed under the GNU Affero General Public License v3.0 or later. See [LICENSE](LICENSE). When distributing a binary release, make the corresponding source for that release available from the same download location.

The bundled BabelDOC 0.6.3 runtime is also licensed under AGPL-3.0-or-later. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for its source and attribution.
