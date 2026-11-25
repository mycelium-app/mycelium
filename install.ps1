param([string]$ControlPlane = $env:MYCELIUM_CONTROL_PLANE)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$releases = if ($env:MYCELIUM_RELEASES) { $env:MYCELIUM_RELEASES } else { 'https://github.com/mycelium-app/mycelium/releases/download' }
$latest = if ($env:MYCELIUM_LATEST) { $env:MYCELIUM_LATEST } else { 'https://github.com/mycelium-app/mycelium/releases/latest/download/latest' }
$asset = 'mycelium-windows-arm64.exe'
$taskName = 'Mycelium node'

function Fail([string]$message) {
    throw "mycelium install: $message"
}

function Quote([string]$value) {
    "'" + $value.Replace("'", "''") + "'"
}

if ($env:OS -ne 'Windows_NT') {
    Fail 'this installer is for Windows. On Linux and macOS use install.sh.'
}

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ("$arch" -ne 'Arm64') {
    Fail "unsupported architecture $arch. Published Windows builds are for ARM64."
}

$binDir = Join-Path $env:LOCALAPPDATA 'Programs\Mycelium'
$binary = Join-Path $binDir 'mycelium.exe'
$runner = Join-Path $binDir 'node.ps1'
$nodeConfig = if ($env:MYCELIUM_NODE_CONFIG) { $env:MYCELIUM_NODE_CONFIG } else { Join-Path $env:APPDATA 'mycelium\node.toml' }

function Stop-Node {
    if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) {
        Stop-ScheduledTask -TaskName $taskName
    }

    Get-Process -Name mycelium -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $binary } | Stop-Process -Force
}

function Install-Binary {
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null

    try {
        if ($env:MYCELIUM_BINARY) {
            if (-not (Test-Path -PathType Leaf $env:MYCELIUM_BINARY)) {
                Fail "MYCELIUM_BINARY points at $env:MYCELIUM_BINARY, which is not a file."
            }

            $source = $env:MYCELIUM_BINARY
        } else {
            try { $tag = (Invoke-RestMethod -UseBasicParsing $latest).Trim() } catch { Fail "could not read $latest. Check your network connection." }
            $source = Join-Path $tmp $asset
            try { Invoke-WebRequest -UseBasicParsing -OutFile $source "$releases/$tag/$asset" } catch { Fail "could not download $releases/$tag/$asset. There may be no Windows build in $tag." }
            try { $checksum = Invoke-RestMethod -UseBasicParsing "$releases/$tag/$asset.sha256" } catch { Fail "could not download the checksum for $asset." }
            $expected = ("$checksum".Trim() -split '\s+')[0].ToLowerInvariant()
            $actual = (Get-FileHash -Algorithm SHA256 $source).Hash.ToLowerInvariant()
            if ($expected -ne $actual) {
                Fail "checksum mismatch for $asset. Nothing was installed."
            }
        }

        Stop-Node
        Copy-Item -Force $source "$binary.new"
        Move-Item -Force "$binary.new" $binary
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

function Write-Runner {
    $lines = @("`$env:MYCELIUM_NODE_CONFIG = $(Quote $nodeConfig)")
    foreach ($name in 'MYCELIUM_CLAUDE_ACP', 'MYCELIUM_CODEX_ACP', 'MYCELIUM_VIBE_ACP') {
        $value = [System.Environment]::GetEnvironmentVariable($name)
        if ($value) {
            $lines += "`$env:$name = $(Quote $value)"
        }
    }

    $lines += "while (`$true) { & $(Quote $binary) node run; Start-Sleep -Seconds 3 }"
    Set-Content -Encoding UTF8 -Path $runner -Value $lines
}

function Join-IfNeeded {
    if (Test-Path -PathType Leaf $nodeConfig) {
        return $true
    }

    if (-not $ControlPlane) {
        Write-Host "Installed. Join a control plane with: `$env:MYCELIUM_NODE_CONFIG = $(Quote $nodeConfig); & $(Quote $binary) node join <control plane url>"
        return $false
    }

    $env:MYCELIUM_NODE_CONFIG = $nodeConfig
    & $binary node join $ControlPlane | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Fail 'the node did not join. Run the installer again to retry.'
    }

    return $true
}

function Start-Node {
    $user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$runner`""
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
    Register-ScheduledTask -Force -TaskName $taskName -Description 'Mycelium node' -Action $action -Trigger $trigger -Principal $principal -Settings $settings | Out-Null
    Start-ScheduledTask -TaskName $taskName
}

Install-Binary
Write-Runner
if (Join-IfNeeded) {
    Start-Node
    Write-Host 'Mycelium node is running.'
}
