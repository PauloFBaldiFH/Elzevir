# SPDX-License-Identifier: GPL-3.0-or-later
#Requires -Version 5.1
[CmdletBinding()]
param(
    [string] $Version = 'latest',
    [string] $Name,
    [ValidatePattern('^[a-z0-9][a-z0-9-]{0,31}$')] [string] $Id = 'library',
    [ValidatePattern('^[a-zA-Z0-9_.-]{1,64}$')] [string] $Login = 'admin',
    [string] $Listen = '127.0.0.1:8080',
    [switch] $Plan,
    [switch] $OpenFirewall,
    [switch] $DefenderExclusion,
    [string] $DownloadOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# SPDX-License-Identifier: GPL-3.0-or-later
Set-StrictMode -Version Latest
function Test-ElzevirVersion([string] $Version) {
    return $Version -cmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
}
function Get-ElzevirChecksum([string] $Checksums, [string] $Name) {
    $found = @($Checksums -split '\r?\n' | Where-Object { $_ -match ('^([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($Name) + '$') })
    if ($found.Count -ne 1) { throw 'Official package checksum absent or duplicated' }
    return ($found[0] -split '\s+')[0].ToLowerInvariant()
}
function Expand-ElzevirPackage([string] $Package, [string] $Output) {
    if ((Get-Item -LiteralPath $Package).Length -gt 536870912) { throw 'Package exceeds size limit' }
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($Package)
    try {
        if ($zip.Entries.Count -ne 3) { throw 'Unexpected package entry count' }
        $seen = @{}
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -cnotin @('elzevir.exe', 'README.md', 'LICENSE') -or $seen.ContainsKey($entry.FullName)) { throw 'Unexpected or duplicate package entry' }
            if (($entry.ExternalAttributes -band 0xF0000000) -eq 0xA0000000) { throw 'Package symlinks are refused' }
            if ($entry.Length -gt 536870912 -or $entry.Length -eq 0) { throw 'Invalid package entry size' }
            $seen[$entry.FullName] = $true
            $destination = Join-Path $Output $entry.FullName
            $input = $entry.Open()
            $target = [IO.File]::Open($destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            try {
                $buffer = New-Object byte[] 65536
                [long] $total = 0
                while (($read = $input.Read($buffer, 0, $buffer.Length)) -gt 0) {
                    $total += $read
                    if ($total -gt $entry.Length) { throw 'Package entry exceeds declared size' }
                    $target.Write($buffer, 0, $read)
                }
                if ($total -ne $entry.Length) { throw 'Truncated package entry' }
                $target.Flush($true)
            } finally { $target.Dispose(); $input.Dispose() }
        }
    } finally { $zip.Dispose() }
}
function Invoke-ElzevirNative([string] $Program, [string[]] $Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'Native Elzevir operation failed; configuration and data were preserved' }
}
function Set-ElzevirPrivateDirectory([string] $Path) {
    & "$env:SystemRoot\System32\icacls.exe" $Path '/inheritance:r' '/grant:r' '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot protect installation directory' }
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'This installer requires Windows' }
$os = [Environment]::OSVersion.Version
if ($os.Major -lt 10 -or $os.Build -lt 17763) { throw 'Windows 10 (1809+) or Server 2019+ is required' }
$architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
switch ($architecture) {
    'AMD64' { $target = 'x86_64-pc-windows-msvc' }
    'ARM64' { $target = 'aarch64-pc-windows-msvc' }
    default { throw 'A 64-bit x64 or ARM64 operating system is required' }
}
if ($Id.EndsWith('-')) { throw 'Invalid institution identifier' }
if ($Version -cne 'latest' -and -not (Test-ElzevirVersion $Version)) { throw 'Version must be latest or a stable SemVer tag' }
$programFiles = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
$installRoot = Join-Path $programFiles 'Elzevir'
$institutionRoot = Join-Path (Join-Path $env:ProgramData 'Elzevir') $Id
$config = Join-Path $institutionRoot 'server.toml'
$data = Join-Path $institutionRoot 'data'
$binary = Join-Path $installRoot 'elzevir.exe'
$service = 'elzevir-' + $Id
if ($Plan) {
    [ordered]@{ target=$target; version=$Version; binary=$binary; config=$config; data=$data; service=$service; firewall=[bool]$OpenFirewall; defender=[bool]$DefenderExclusion } | ConvertTo-Json -Compress
    return
}
if (-not $DownloadOnly) {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run PowerShell as Administrator' }
    foreach ($path in @($installRoot,$institutionRoot,$config,$data,$binary)) {
        if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Installation paths must not be links or junctions' }
    }
    if (Test-Path -LiteralPath $config) {
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf) -or -not (Test-Path -LiteralPath (Join-Path $data 'elzevir.db') -PathType Leaf)) { throw 'Existing installation is incomplete; inspect it before retrying' }
    } elseif (-not $Name -or (Test-Path -LiteralPath $data)) { throw 'First installation requires -Name and a new data directory' }
}
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$repository = 'https://github.com/PauloFBaldiFH/Elzevir'
if ($Version -ceq 'latest') {
    $response = Invoke-WebRequest -Uri "$repository/releases/latest" -Method Head -UseBasicParsing -MaximumRedirection 5
    $resolved = $response.BaseResponse.ResponseUri.AbsoluteUri
    if (-not $resolved.StartsWith("$repository/releases/tag/",[StringComparison]::Ordinal)) { throw 'Cannot resolve an official stable release' }
    $Version = $resolved.Substring($resolved.LastIndexOf('/') + 1)
    if (-not (Test-ElzevirVersion $Version)) { throw 'Latest release is not a stable SemVer tag' }
}
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('elzevir-install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
Set-ElzevirPrivateDirectory $temporary
try {
    $packageName = "elzevir-$Version-$target.zip"
    $package = Join-Path $temporary $packageName
    $sumsPath = Join-Path $temporary 'SHA256SUMS'
    $base = "$repository/releases/download/$Version"
    Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sumsPath -UseBasicParsing -MaximumRedirection 5 -TimeoutSec 60
    if ((Get-Item -LiteralPath $sumsPath).Length -gt 65536) { throw 'Checksum document exceeds limit' }
    $expected = Get-ElzevirChecksum ([IO.File]::ReadAllText($sumsPath)) $packageName
    Invoke-WebRequest -Uri "$base/$packageName" -OutFile $package -UseBasicParsing -MaximumRedirection 5 -TimeoutSec 900
    if ((Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) { throw 'Package checksum mismatch; nothing was installed' }
    $payload = Join-Path $temporary 'payload'
    New-Item -ItemType Directory -Path $payload | Out-Null
    Expand-ElzevirPackage $package $payload
    if ($DownloadOnly) {
        if (Test-Path -LiteralPath $DownloadOnly) { throw 'Download destination must be new' }
        New-Item -ItemType Directory -Path $DownloadOnly | Out-Null
        Set-ElzevirPrivateDirectory $DownloadOnly
        Copy-Item -LiteralPath (Join-Path $payload 'elzevir.exe'),(Join-Path $payload 'README.md'),(Join-Path $payload 'LICENSE') -Destination $DownloadOnly
        Write-Output "Verified package saved to $DownloadOnly"
        return
    }
    if (Test-Path -LiteralPath $config) {
        Invoke-ElzevirNative $binary @('update','--config',$config,'--service',$service,'--version',$Version,'--package',$package,'--checksums',$sumsPath,'--confirm-update','--wait')
        return
    }
    $drive = New-Object IO.DriveInfo([IO.Path]::GetPathRoot($institutionRoot))
    if ($drive.DriveType -ne [IO.DriveType]::Fixed -or $drive.AvailableFreeSpace -lt 1073741824) { throw 'At least 1 GiB on a fixed local disk is required' }
    New-Item -ItemType Directory -Path $installRoot,$institutionRoot -Force | Out-Null
    Set-ElzevirPrivateDirectory $installRoot
    Set-ElzevirPrivateDirectory $institutionRoot
    Copy-Item -LiteralPath (Join-Path $payload 'elzevir.exe') -Destination $binary
    Invoke-ElzevirNative $binary @('init','--name',$Name,'--config',$config,'--data-dir',$data,'--login',$Login,'--listen',$Listen)
    Invoke-ElzevirNative $binary @('service','install','--config',$config,'--name',$service)
    $serviceAccount = 'NT SERVICE\' + $service
    & "$env:SystemRoot\System32\icacls.exe" $installRoot '/grant' "${serviceAccount}:(OI)(CI)RX" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot grant service executable access' }
    & "$env:SystemRoot\System32\icacls.exe" $institutionRoot '/grant' "${serviceAccount}:(OI)(CI)RX" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot grant service configuration access' }
    & "$env:SystemRoot\System32\icacls.exe" $data '/grant' "${serviceAccount}:(OI)(CI)M" '/T' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot grant service data access' }
    Invoke-ElzevirNative $binary @('service','start','--name',$service)
    $healthy = $false
    for ($attempt=0; $attempt -lt 60; $attempt++) {
        & $binary status --config $config --json *> $null
        if ($LASTEXITCODE -eq 0) { $healthy = $true; break }
        Start-Sleep -Seconds 1
    }
    if (-not $healthy) { throw 'Service did not become healthy; data and configuration were preserved' }
    if ($OpenFirewall) {
        $port = [int]($Listen.Substring($Listen.LastIndexOf(':') + 1))
        New-NetFirewallRule -DisplayName "Elzevir ($Id)" -Direction Inbound -Action Allow -Protocol TCP -LocalPort $port | Out-Null
    }
    if ($DefenderExclusion) { Add-MpPreference -ExclusionPath $data }
    $shortcutDirectory = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut((Join-Path $shortcutDirectory "Painel do Elzevir ($Id).lnk"))
    $shortcut.TargetPath = $binary
    $shortcut.Arguments = 'panel --config "' + $config + '" --lang pt-BR'
    $shortcut.WorkingDirectory = $institutionRoot
    $shortcut.Save()
    Write-Output "Elzevir ready. Open the Start menu shortcut, or: & '$binary' panel --config '$config' --lang pt-BR"
} finally {
    Remove-Item -LiteralPath $temporary -Recurse -Force
}
