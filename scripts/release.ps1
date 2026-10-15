# Builds the release artifacts into dist\:
#   Clipvault-Setup.exe   the installer (the same executable; it installs itself when it is named *Setup*.exe)
#   Clipvault.exe         the portable version (also what the auto-updater downloads)
#   clipvault-cli.exe     the command-line tool
#   SHA256SUMS.txt        checksums of the three files
#
#   powershell -ExecutionPolicy Bypass -File scripts\release.ps1             # fmt + clippy + tests + build + dist\
#   powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -Publish    # ... and `gh release create`
#
# If the certificate CN=ahsoggg exists on this machine (scripts\make_cert.ps1) the executables are signed before
# the checksums are computed; otherwise they are released unsigned. GitHub Actions never signs.
param([switch]$Publish, [switch]$SkipChecks, [string]$NotesFile = "")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path crates\app\Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$tag = "v$version"
Write-Host "Clipvault $tag"
if ($NotesFile -eq "") { $NotesFile = "docs\release-notes\$tag.md" }

if (-not $SkipChecks) {
    cargo fmt --check
    if ($LASTEXITCODE -ne 0) { throw "cargo fmt --check failed" }
    cargo clippy --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "clippy failed" }
    cargo test --workspace --release
    if ($LASTEXITCODE -ne 0) { throw "tests failed" }
}
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "build failed" }

$dist = Join-Path $root "dist"
if (Test-Path $dist) { Remove-Item -LiteralPath $dist -Recurse -Force }
New-Item -ItemType Directory $dist | Out-Null
Copy-Item target\release\Clipvault.exe (Join-Path $dist "Clipvault.exe")
Copy-Item target\release\clipvault-cli.exe (Join-Path $dist "clipvault-cli.exe")
$hasCert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -eq "CN=ahsoggg" }
if ($hasCert) {
    & (Join-Path $PSScriptRoot "sign.ps1") (Join-Path $dist "Clipvault.exe") (Join-Path $dist "clipvault-cli.exe")
} else {
    Write-Warning "certificate CN=ahsoggg not found - the executables are UNSIGNED (see scripts\make_cert.ps1)"
}
# the installer is the same file under another name, copied after signing so the signature is included
Copy-Item (Join-Path $dist "Clipvault.exe") (Join-Path $dist "Clipvault-Setup.exe")

$lines = foreach ($f in "Clipvault-Setup.exe", "Clipvault.exe", "clipvault-cli.exe") {
    $h = (Get-FileHash (Join-Path $dist $f) -Algorithm SHA256).Hash.ToLower()
    "$h  $f"
}
[IO.File]::WriteAllText((Join-Path $dist "SHA256SUMS.txt"), (($lines -join "`n") + "`n"), (New-Object Text.UTF8Encoding($false)))
Get-ChildItem $dist | Format-Table Name, Length
Get-Content (Join-Path $dist "SHA256SUMS.txt")

if ($Publish) {
    $assets = Get-ChildItem $dist | ForEach-Object { $_.FullName }
    gh release create $tag @assets --title "Clipvault $version" --notes-file $NotesFile
    if ($LASTEXITCODE -ne 0) { throw "gh release failed" }
}
