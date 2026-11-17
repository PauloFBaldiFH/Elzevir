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
    $found = @()
    foreach ($line in ($Checksums -split '\r?\n')) {
        $parts = $line.Trim() -split '\s+', 3
        if ($parts.Count -ge 2 -and $parts[1].TrimStart('*') -ceq $Name) {
            if ($parts.Count -ne 2 -or $parts[0] -cnotmatch '^[0-9a-fA-F]{64}$') { throw 'Invalid official package checksum' }
            $found += $parts[0].ToLowerInvariant()
        }
    }
    if ($found.Count -ne 1) { throw 'Official package checksum absent or duplicated' }
    return $found[0]
}
function Get-ElzevirResponse([uri] $Uri, [switch] $Head) {
    $current = $Uri
    for ($redirect=0; $redirect -le 5; $redirect++) {
        if (-not $current.IsAbsoluteUri -or $current.Scheme -cne 'https' -or $current.UserInfo) { throw 'Downloads require HTTPS without embedded credentials' }
        $request = [Net.WebRequest]::Create($current)
        $request.AllowAutoRedirect = $false
        $request.Timeout = 60000
        $request.ReadWriteTimeout = 60000
        $request.UserAgent = 'Elzevir-Installer'
        $request.Method = if ($Head) { 'HEAD' } else { 'GET' }
        try { $response = $request.GetResponse() } catch { throw 'Official release request failed' }
        $status = [int]$response.StatusCode
        if ($status -in @(301,302,303,307,308)) {
            $location = $response.Headers['Location']
            $response.Dispose()
            if (-not $location) { throw 'Release redirect has no destination' }
            $current = New-Object Uri($current, $location)
            continue
        }
        if ($status -ne 200) { $response.Dispose(); throw 'Unexpected release response' }
        return $response
    }
    throw 'Too many release redirects'
}
function Save-ElzevirDownload([uri] $Uri, [string] $Output, [long] $Limit) {
    if ($Limit -le 0 -or $Limit -gt 536870912) { throw 'Invalid download size limit' }
    $response = Get-ElzevirResponse $Uri
    $input = $null
    $target = $null
    $created = $false
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    try {
        if ($response.ContentLength -gt $Limit) { throw 'Release download exceeds size limit' }
        $input = $response.GetResponseStream()
        $target = [IO.File]::Open($Output, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $created = $true
        $buffer = New-Object byte[] 65536
        [long] $total = 0
        while (($read = $input.Read($buffer, 0, $buffer.Length)) -gt 0) {
            if ($deadline.Elapsed.TotalSeconds -gt 900) { throw 'Release download timed out' }
            $total += $read
            if ($total -gt $Limit) { throw 'Release download exceeds size limit' }
            $target.Write($buffer, 0, $read)
        }
        if ($response.ContentLength -ge 0 -and $total -ne $response.ContentLength) { throw 'Truncated release download' }
        $target.Flush($true)
    } catch {
        if ($target) { $target.Dispose(); $target = $null }
        if ($created) { [IO.File]::Delete($Output) }
        throw
    } finally {
        if ($target) { $target.Dispose() }
        if ($input) { $input.Dispose() }
        $response.Dispose()
    }
}
function Expand-ElzevirPackage([string] $Package, [string] $Output) {
    if ((Get-Item -LiteralPath $Package).Length -gt 536870912) { throw 'Package exceeds size limit' }
    $directory = [IO.File]::OpenRead($Package)
    try {
        if ($directory.Length -lt 22) { throw 'Truncated ZIP directory' }
        $null = $directory.Seek(-22, [IO.SeekOrigin]::End)
        $end = New-Object byte[] 22
        if ($directory.Read($end,0,22) -ne 22 -or [BitConverter]::ToUInt32($end,0) -ne 0x06054B50) { throw 'Invalid ZIP directory' }
        if ([BitConverter]::ToUInt32($end,4) -ne 0 -or [BitConverter]::ToUInt16($end,8) -ne 3 -or [BitConverter]::ToUInt16($end,10) -ne 3 -or [BitConverter]::ToUInt16($end,20) -ne 0) { throw 'Unsupported ZIP directory or entry count' }
        [long] $centralEnd = [long][BitConverter]::ToUInt32($end,12) + [long][BitConverter]::ToUInt32($end,16)
        if ($centralEnd -gt $directory.Length - 22) { throw 'Invalid ZIP directory bounds' }
    } finally { $directory.Dispose() }
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
function Grant-ElzevirServiceAccess([string] $InstallRoot, [string] $InstitutionRoot, [string] $Data, [string] $Service) {
    $account = 'NT SERVICE\' + $Service
    foreach ($path in @($InstallRoot,$InstitutionRoot)) {
        & "$env:SystemRoot\System32\icacls.exe" $path '/grant' ($account + ':(OI)(CI)RX') | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Cannot grant service executable/configuration access' }
    }
    & "$env:SystemRoot\System32\icacls.exe" $Data '/grant' ($account + ':(OI)(CI)M') '/T' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot grant service data access' }
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
$installRoot = Join-Path $programFiles $(if ($Id -ceq 'library') { 'Elzevir' } else { 'Elzevir-' + $Id })
$institutionRoot = Join-Path (Join-Path $env:ProgramData 'Elzevir') $Id
$config = Join-Path $institutionRoot 'server.toml'
$data = Join-Path $institutionRoot 'data'
$binary = Join-Path $installRoot 'elzevir.exe'
$service = if ($Id -ceq 'library') { 'elzevir' } else { 'elzevir-' + $Id }
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
    if (-not (Test-Path -LiteralPath $config) -and (Test-Path -LiteralPath $binary)) { throw 'Existing executable without matching configuration was preserved' }
}
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$repository = 'https://github.com/PauloFBaldiFH/Elzevir'
if ($Version -ceq 'latest') {
    $response = Get-ElzevirResponse -Uri "$repository/releases/latest" -Head
    try { $resolved = $response.ResponseUri.AbsoluteUri } finally { $response.Dispose() }
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
    Save-ElzevirDownload "$base/SHA256SUMS" $sumsPath 65536
    if ((Get-Item -LiteralPath $sumsPath).Length -gt 65536) { throw 'Checksum document exceeds limit' }
    $expected = Get-ElzevirChecksum ([IO.File]::ReadAllText($sumsPath)) $packageName
    Save-ElzevirDownload "$base/$packageName" $package 536870912
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
        $managerJson = & $binary service status --name $service
        if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect the existing service' }
        $manager = $managerJson | ConvertFrom-Json
        if ($manager.state -ceq 'not_installed') {
            Invoke-ElzevirNative $binary @('service','install','--config',$config,'--name',$service)
            Grant-ElzevirServiceAccess $installRoot $institutionRoot $data $service
            Invoke-ElzevirNative $binary @('service','start','--name',$service)
        }
        Invoke-ElzevirNative $binary @('update','--config',$config,'--service',$service,'--version',$Version,'--package',$package,'--checksums',$sumsPath,'--confirm-update','--wait')
        return
    }
    $drive = New-Object IO.DriveInfo([IO.Path]::GetPathRoot($institutionRoot))
    if ($drive.DriveType -ne [IO.DriveType]::Fixed -or $drive.AvailableFreeSpace -lt 1073741824) { throw 'At least 1 GiB on a fixed local disk is required' }
    New-Item -ItemType Directory -Path $installRoot,$institutionRoot -Force | Out-Null
    Set-ElzevirPrivateDirectory $installRoot
    Set-ElzevirPrivateDirectory $institutionRoot
    Copy-Item -LiteralPath (Join-Path $payload 'elzevir.exe') -Destination $binary
    Invoke-ElzevirNative $binary @('init','--name',$Name,'--config',$config,'--data-dir',$data,'--login',$Login,'--listen',$Listen,'--check-port')
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
    $shortcutPath = Join-Path $shortcutDirectory "Painel do Elzevir ($Id).lnk"
    $shortcutBytes = [IO.File]::ReadAllBytes($shortcutPath)
    if ($shortcutBytes.Length -lt 76) { throw 'Invalid panel shortcut' }
    $shortcutBytes[21] = $shortcutBytes[21] -bor 0x20
    [IO.File]::WriteAllBytes($shortcutPath, $shortcutBytes)
    Write-Output "Elzevir ready. Open the Start menu shortcut, or: & '$binary' panel --config '$config' --lang pt-BR"
} finally {
    Remove-Item -LiteralPath $temporary -Recurse -Force
}
