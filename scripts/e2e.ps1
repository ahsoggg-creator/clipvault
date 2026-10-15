# End-to-end check on a real Windows desktop: the real clipboard, the real hotkey, a real paste.
# Uses its own data folder (CLIPVAULT_DATA_DIR), so the user's history is never touched.
#
#   powershell -ExecutionPolicy Bypass -File scripts\e2e.ps1 [-Exe target\release\Clipvault.exe] [-Cli target\release\clipvault-cli.exe]
param(
    [string]$Exe = "target\release\Clipvault.exe",
    [string]$Cli = "target\release\clipvault-cli.exe"
)
$ErrorActionPreference = "Stop"
if ([Threading.Thread]::CurrentThread.GetApartmentState() -ne "STA") {
    # the clipboard and WinForms need a single-threaded apartment
    $self = (Get-Process -Id $PID).Path
    & $self -NoProfile -STA -ExecutionPolicy Bypass -File $PSCommandPath -Exe $Exe -Cli $Cli
    exit $LASTEXITCODE
}
Set-Location (Split-Path -Parent $PSScriptRoot)
$Exe = (Resolve-Path $Exe).Path
$Cli = (Resolve-Path $Cli).Path
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace E2E -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
'@

$script:fail = 0
function Check($name, [bool]$ok, $detail = "") {
    if ($ok) { Write-Host "  ok    $name" } else { Write-Host "  FAIL  $name $detail"; $script:fail++ }
}
function Press([byte[]]$mods, [byte]$key) {
    foreach ($m in $mods) { [E2E.Win]::keybd_event($m, 0, 0, [UIntPtr]::Zero) }
    [E2E.Win]::keybd_event($key, 0, 0, [UIntPtr]::Zero)
    [E2E.Win]::keybd_event($key, 0, 2, [UIntPtr]::Zero)
    foreach ($m in $mods) { [E2E.Win]::keybd_event($m, 0, 2, [UIntPtr]::Zero) }
}
function Popup-Visible($title = "Clipvault") { $h = [E2E.Win]::FindWindow([NullString]::Value, $title); $h -ne [IntPtr]::Zero -and [E2E.Win]::IsWindowVisible($h) }

$data = Join-Path $env:TEMP ("clipvault-e2e-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
$env:CLIPVAULT_DATA_DIR = $data
$marker = "E2EMARK" + (Get-Random -Minimum 100000 -Maximum 999999)
$saved = $null
try { $saved = Get-Clipboard -Raw -ErrorAction SilentlyContinue } catch {}
Get-Process Clipvault -ErrorAction SilentlyContinue | Stop-Process -Force
$app = Start-Process $Exe -ArgumentList "--background" -PassThru
Start-Sleep 3
try {
    Write-Host "capture"
    Set-Clipboard -Value "$marker first entry"
    Start-Sleep -Milliseconds 700
    Set-Clipboard -Value "#1E90FF"
    Start-Sleep -Milliseconds 700
    $img = New-Object System.Drawing.Bitmap 32, 24
    for ($i = 0; $i -lt 5; $i++) { try { [System.Windows.Forms.Clipboard]::SetImage($img); break } catch { Start-Sleep -Milliseconds 200 } }
    Start-Sleep -Milliseconds 900
    Set-Clipboard -Value "https://example.com/$marker"
    Start-Sleep -Milliseconds 700
    Set-Clipboard -Value "$marker first entry"   # duplicate: must bump to the top, not add a copy
    Start-Sleep -Milliseconds 900
    $list = & $Cli list 2>&1 | Out-String
    $rows = ($list -split "`n" | Where-Object { $_.Trim() }).Count
    Check "four distinct entries (duplicate merged)" ($rows -eq 4) "rows=$rows`n$list"
    Check "types detected" (($list -match "link") -and ($list -match "color") -and ($list -match "image") -and ($list -match "text")) $list
    Check "search finds by fuzzy text" ((& $Cli search "frst entry" 2>&1 | Out-String) -match $marker)

    Write-Host "disk"
    $leak = Get-ChildItem $data -Recurse -File | Where-Object {
        [IO.File]::ReadAllText($_.FullName, [Text.Encoding]::Latin1) -match "E2EMARK"
    }
    Check "no plaintext marker in any file of the data folder" (-not $leak) ($leak.Name -join ",")
    Check "log has no marker" (-not ((Get-Content "$data\logs\clipvault.log" -Raw) -match "E2EMARK"))

    Write-Host "popup and paste"
    # The test steals the focus and types keys: if somebody touches the keyboard meanwhile it can miss, so it gets up to 3 tries.
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        if (Popup-Visible) { Press @() 0x1B; Start-Sleep -Milliseconds 600 }   # a failed attempt may leave the popup open
        $script:visible = $false; $script:after = $false; $script:text = ""
        $form = New-Object System.Windows.Forms.Form
        $form.Text = "e2e target"; $form.Width = 500; $form.Height = 200; $form.StartPosition = "CenterScreen"
        $tb = New-Object System.Windows.Forms.TextBox
        $tb.Dock = "Fill"; $tb.Multiline = $true
        $form.Controls.Add($tb)
        $script:step = 0
        $timer = New-Object System.Windows.Forms.Timer
        $timer.Interval = 1300
        $timer.Add_Tick({
            switch ($script:step) {
                0 { $form.Activate(); [E2E.Win]::SetForegroundWindow($form.Handle) | Out-Null; $tb.Focus() | Out-Null }
                1 { Press @(0x11, 0x12) 0x56 }                                  # Ctrl+Alt+V
                2 { $script:visible = Popup-Visible }
                3 { Press @() 0x0D }                                            # Enter: paste the selected (top) entry
                5 { $script:text = $tb.Text; $script:after = Popup-Visible; $timer.Stop(); $form.Close() }
            }
            $script:step++
        })
        $form.Add_Shown({ $timer.Start() })
        [void]$form.ShowDialog()
        if ($script:visible -and -not $script:after -and $script:text -match [regex]::Escape("$marker first entry")) { break }
        Write-Host "  (attempt $attempt did not complete, retrying)"
    }
    if (-not ($script:text -match [regex]::Escape("$marker first entry"))) { Write-Host "--- history order ---"; & $Cli list }
    Check "hotkey opens the popup" ([bool]$script:visible)
    Check "popup closes after paste" (-not $script:after)
    Check "Enter pastes the top entry into the focused window" ($script:text -match [regex]::Escape("$marker first entry")) "got [$($script:text)]"

    Write-Host "administrator helper (started without elevation: checks the plumbing, not UAC)"
    $adm = Start-Process $Exe -ArgumentList "--admin" -PassThru
    Start-Sleep 3
    Check "helper window opens on start" (Popup-Visible "Clipvault (admin)")
    Press @(0x11, 0x12, 0x10) 0x56                                           # Ctrl+Alt+Shift+V toggles it off...
    Start-Sleep -Milliseconds 900
    $off = -not (Popup-Visible "Clipvault (admin)")
    Press @(0x11, 0x12, 0x10) 0x56                                           # ...and on again
    Start-Sleep -Milliseconds 1200
    Check "its own hotkey toggles the helper window" ($off -and (Popup-Visible "Clipvault (admin)"))
    Check "helper does not touch the main window" (-not (Popup-Visible "Clipvault"))
    Stop-Process $adm -Force -ErrorAction SilentlyContinue

    Write-Host "installer (per-user, no administrator rights; LOCALAPPDATA redirected to a temp folder)"
    $realLocal = $env:LOCALAPPDATA
    $env:LOCALAPPDATA = Join-Path $env:TEMP ("cvinst-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
    New-Item -ItemType Directory $env:LOCALAPPDATA | Out-Null
    try {
        $setup = Join-Path $env:TEMP "Clipvault-Setup.exe"
        Copy-Item $Exe $setup -Force
        $p = Start-Process $setup -ArgumentList "--install", "--silent" -Wait -PassThru
        $inst = Join-Path $env:LOCALAPPDATA "Programs\Clipvault\Clipvault.exe"
        $lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "Clipvault.lnk"
        $key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Clipvault"
        Check "Clipvault-Setup.exe installs without UAC" ($p.ExitCode -eq 0 -and (Test-Path $inst))
        Check "Start menu shortcut" (Test-Path $lnk)
        Check "Apps & features entry" ((Get-ItemProperty $key -ErrorAction SilentlyContinue).DisplayName -eq "Clipvault")
        $p = Start-Process $inst -ArgumentList "--uninstall", "--silent" -Wait -PassThru
        Start-Sleep 7
        Check "uninstall removes folder, shortcut and registry entry" (($p.ExitCode -eq 0) -and -not (Test-Path (Split-Path $inst)) -and -not (Test-Path $lnk) -and -not (Test-Path $key))
        Remove-Item $setup -Force -ErrorAction SilentlyContinue
    }
    finally {
        Remove-Item $env:LOCALAPPDATA -Recurse -Force -ErrorAction SilentlyContinue
        $env:LOCALAPPDATA = $realLocal
    }

    Write-Host "limits and pause (settings.json in a fresh data folder)"
    function Run-With-Settings($json, [scriptblock]$body) {
        Get-Process Clipvault -ErrorAction SilentlyContinue | Stop-Process -Force
        $d = Join-Path $env:TEMP ("clipvault-e2e-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
        New-Item -ItemType Directory $d | Out-Null
        Set-Content -Path (Join-Path $d "settings.json") -Value $json -Encoding utf8
        $prev = $env:CLIPVAULT_DATA_DIR
        $env:CLIPVAULT_DATA_DIR = $d
        $a = Start-Process $Exe -ArgumentList "--background" -PassThru
        Start-Sleep 3
        try { & $body } finally {
            Stop-Process $a -Force -ErrorAction SilentlyContinue
            $env:CLIPVAULT_DATA_DIR = $prev
            Start-Sleep -Milliseconds 400
            Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    Run-With-Settings '{"max_item_kb": 1}' {
        Set-Clipboard -Value ("big " * 600)       # about 2.4 KB: over the 1 KB limit
        Start-Sleep -Milliseconds 800
        Set-Clipboard -Value "small entry"
        Start-Sleep -Milliseconds 900
        $l = & $Cli list 2>&1 | Out-String
        Check "an entry over the size limit is not stored, a small one is" (($l -match "small entry") -and -not ($l -match "big big"))
    }
    Run-With-Settings '{"paused": true}' {
        Set-Clipboard -Value "must not be recorded"
        Start-Sleep -Milliseconds 1200
        $l = (& $Cli list 2>&1 | Out-String).Trim()
        Check "paused recording stores nothing" ($l -eq "")
    }

    Write-Host "secret protection"
    $fakeKey = "sk-E2EMARK" + (-join ((48..57) + (97..122) | Get-Random -Count 32 | ForEach-Object { [char]$_ }))
    function Set-Clip-WithFormats([string]$text, [hashtable]$formats) {
        for ($i = 0; $i -lt 6; $i++) {
            try {
                $d = New-Object System.Windows.Forms.DataObject
                $d.SetText($text)
                foreach ($k in $formats.Keys) { $d.SetData($k, [IO.MemoryStream]::new([byte[]]$formats[$k])) }
                [System.Windows.Forms.Clipboard]::SetDataObject($d, $true)
                return
            } catch { Start-Sleep -Milliseconds 200 }
        }
    }
    function Clip-Text { try { (Get-Clipboard -Raw) } catch { "" } }

    Run-With-Settings '{"secret_policy": "memory_only", "secret_clear_secs": 3}' {
        Set-Clipboard -Value $fakeKey
        Start-Sleep -Milliseconds 1200
        $l = & $Cli list 2>&1 | Out-String
        $onDisk = Get-ChildItem $env:CLIPVAULT_DATA_DIR -Recurse -File | Where-Object { [IO.File]::ReadAllText($_.FullName, [Text.Encoding]::Latin1) -match "E2EMARK" }
        Check "memory-only: the key is neither listed nor on disk" (-not ($l -match "E2EMARK") -and -not $onDisk)
        Check "memory-only: still on the clipboard before the timeout" ((Clip-Text) -match "E2EMARK")
        Start-Sleep -Seconds 4
        Check "memory-only: wiped from the clipboard after N seconds" (-not ((Clip-Text) -match "E2EMARK"))
        Set-Clipboard -Value "plain text survives"
        Start-Sleep -Seconds 4
        Check "memory-only: ordinary text is not wiped" ((Clip-Text) -match "plain text survives")
    }
    Run-With-Settings '{"secret_policy": "dont_store"}' {
        Set-Clipboard -Value $fakeKey
        Start-Sleep -Milliseconds 1200
        Check "do-not-store: nothing recorded" (-not ((& $Cli list 2>&1 | Out-String) -match "\S"))
    }
    Run-With-Settings '{"secret_policy": "mask"}' {
        Set-Clipboard -Value $fakeKey
        Start-Sleep -Milliseconds 1200
        $l = & $Cli list 2>&1 | Out-String
        $onDisk = Get-ChildItem $env:CLIPVAULT_DATA_DIR -Recurse -File | Where-Object { [IO.File]::ReadAllText($_.FullName, [Text.Encoding]::Latin1) -match "E2EMARK" }
        Check "masked: a placeholder is stored, the key is not" (($l -match "API") -and -not ($l -match "E2EMARK") -and -not $onDisk) $l
    }
    Run-With-Settings '{"secrets_enabled": true, "custom_patterns": ["ACME-[0-9]{4}"]}' {
        Set-Clipboard -Value "badge ACME-4821 issued"
        Start-Sleep -Milliseconds 1200
        Set-Clipboard -Value "an ordinary note"
        Start-Sleep -Milliseconds 1200
        $l = & $Cli list 2>&1 | Out-String
        Check "custom pattern is treated as a secret, ordinary text is stored" (($l -match "ordinary note") -and -not ($l -match "ACME-4821"))
    }

    Write-Host "apps' own flags and the process blacklist"
    Run-With-Settings '{}' {
        Set-Clip-WithFormats "flag exclude" @{ "ExcludeClipboardContentFromMonitorProcessing" = @(1) }
        Start-Sleep -Milliseconds 1000
        Set-Clip-WithFormats "flag history zero" @{ "CanIncludeInClipboardHistory" = @(0, 0, 0, 0) }
        Start-Sleep -Milliseconds 1000
        Set-Clip-WithFormats "flag cloud zero" @{ "CanUploadToCloudClipboard" = @(0, 0, 0, 0) }
        Start-Sleep -Milliseconds 1000
        Set-Clip-WithFormats "flag viewer ignore" @{ "Clipboard Viewer Ignore" = @(1) }
        Start-Sleep -Milliseconds 1000
        Set-Clip-WithFormats "flag history one" @{ "CanIncludeInClipboardHistory" = @(1, 0, 0, 0) }
        Start-Sleep -Milliseconds 1200
        $l = & $Cli list 2>&1 | Out-String
        Check "the four flag formats prevent recording" (-not ($l -match "flag exclude|flag history zero|flag cloud zero|flag viewer ignore")) $l
        Check "a flag that allows recording does not block it" ($l -match "flag history one") $l
    }
    Run-With-Settings '{"blacklist": ["pwsh.exe"]}' {
        Set-Clip-WithFormats "copied from a blacklisted process" @{}
        Start-Sleep -Milliseconds 1200
        Check "copy from a blacklisted process creates no entry" (-not ((& $Cli list 2>&1 | Out-String) -match "\S"))
    }
    Run-With-Settings '{"blacklist": []}' {
        Set-Clip-WithFormats "copied from an allowed process" @{}
        Start-Sleep -Milliseconds 1200
        Check "the same copy is recorded when the process is not blacklisted" ((& $Cli list 2>&1 | Out-String) -match "allowed process")
    }
}
finally {
    if ($script:fail -gt 0 -and (Test-Path "$data\logs\clipvault.log")) { Write-Host "--- app log tail ---"; Get-Content "$data\logs\clipvault.log" -Tail 12 }
    Get-Process Clipvault -ErrorAction SilentlyContinue | Stop-Process -Force
    if ($saved) { Set-Clipboard -Value $saved }
    Start-Sleep -Milliseconds 500
    Remove-Item $data -Recurse -Force -ErrorAction SilentlyContinue
}
if ($script:fail -gt 0) { Write-Host "e2e: $script:fail check(s) FAILED"; exit 1 }
Write-Host "e2e: all checks passed"
