set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]

root := justfile_directory()
env_dir := root + "\\env"
verify_home := root + "\\target\\verify-bfx"
inno_compiler := env("BFX_INNO", "C:\\Program Files\\Inno Setup 7\\ISCC.exe")
host_os := os()
host_arch := arch()
installer_target := if host_os == "windows" { if host_arch == "aarch64" { "windows-arm64" } else { "windows-x64" } } else if host_os == "linux" { if host_arch == "aarch64" { "linux-arm64" } else { "linux-x64" } } else { "osx" }

_default:
    @just --unsorted --list

[group('Setup')]
setup:
    pixi install
    pixi run just _toolchain

[group('Rust')]
cargo command *args:
    @$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'; $vsroot = & $vswhere -latest -products * -property installationPath; $vcinclude = (Get-ChildItem "$vsroot\VC\Tools\MSVC\*\include\stdarg.h" | Select-Object -First 1).DirectoryName; cmd.exe /d /c "call `"$vsroot\VC\Auxiliary\Build\vcvarsall.bat`" x64 >nul && set `"INCLUDE=%INCLUDE%;$vcinclude`" && set `"BFX_ROOT={{ root }}`" && set `"BFX_BABELDOC={{ root }}\.pixi\envs\runtime\Scripts\babeldoc.exe`" && pixi run cargo {{ command }} {{ args }}"

[group('Rust')]
check:
    @just cargo check

[group('Rust')]
build:
    @just cargo build

[group('Rust')]
release:
    @just cargo build --release

[group('Rust')]
test:
    @just cargo test

[group('Rust')]
run command *args:
    @just cargo run -- {{ command }} {{ args }}

[group('Rust')]
clean:
    just cargo clean

[group('BFX')]
bfx command *args:
    @just cargo run -- {{ command }} {{ args }}

[group('BFX')]
doctor:
    @just bfx doctor

[group('BFX')]
verify file model="GPT5.4" preset="Default":
    @Get-CimInstance Win32_Process -Filter "Name = 'bfx.exe'" | Where-Object { $_.CommandLine -eq ('"{{ root }}\target\debug\bfx.exe" worker') } | ForEach-Object { Stop-Process -Id $_.ProcessId }
    @just cargo build --quiet
    @$env:BFX_HOME = "{{ verify_home }}"; $env:BFX_CONFIG_PATH = "{{ env_dir }}\\config.toml"; $env:BFX_ROOT = "{{ root }}"; $env:BFX_BABELDOC = "{{ root }}\\.pixi\\envs\\runtime\\Scripts\\babeldoc.exe"; & "{{ root }}\\target\\debug\\bfx.exe" run "{{ file }}" --model "{{ model }}" --preset "{{ preset }}"

[group('Verify')]
fmt:
    @just cargo fmt

[group('Verify')]
fmt-check:
    @just cargo fmt -- --check

[group('Verify')]
pre-commit: fmt-check test

[group('Package')]
package platform=installer_target:
    @if ("{{ platform }}" -ne "windows-x64") { throw "Inno Setup packaging currently supports windows-x64 only." }
    just release
    @& "{{ inno_compiler }}" "{{ root }}\installer\BabelForge.iss"

_toolchain:
    just cargo --version
