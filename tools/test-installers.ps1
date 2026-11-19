# SPDX-License-Identifier: GPL-3.0-or-later
# Native Windows checks without service registration or administrator privileges.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $root 'install.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) { throw ($errors | Out-String) }
$functions = $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $true)
foreach ($function in $functions) { Invoke-Expression $function.Extent.Text }
function Assert-True([bool] $Value, [string] $Message) { if (-not $Value) { throw $Message } }
function Assert-Throws([scriptblock] $Action) { $threw = $false; try { & $Action } catch { $threw = $true }; Assert-True $threw 'Operation should have been refused' }
Assert-True (Test-ElzevirVersion 'v1.2.3') 'Stable version refused'
Assert-True (Test-ElzevirVersion 'v0.1.0-alpha.1') 'Preview version refused'
Assert-True (-not (Test-ElzevirVersion 'v01.2.3')) 'Noncanonical version accepted'
Assert-True (-not (Test-ElzevirVersion 'v1.2.3-alpha.01')) 'Noncanonical preview version accepted'
$hash = 'a' * 64
$sums = "$hash  package.zip`n"
Assert-True ((Get-ElzevirChecksum $sums 'package.zip') -ceq $hash) 'Checksum was not exact'
Assert-Throws { Get-ElzevirChecksum ($sums + $sums) 'package.zip' }
Assert-Throws { Get-ElzevirChecksum ($sums + 'invalid  package.zip') 'package.zip' }
Assert-Throws { Get-ElzevirChecksum $sums 'another.zip' }
Assert-Throws { Get-ElzevirResponse 'http://example.invalid/package.zip' }
Assert-Throws { Get-ElzevirResponse 'https://user:password@example.invalid/package.zip' }
$official = 'https://github.com/PauloFBaldiFH/Elzevir'
$originalResponse = (Get-Item Function:Get-ElzevirResponse).ScriptBlock
try {
    function Get-ElzevirResponse([uri] $Uri, [switch] $Head) {
        if (-not $Head) {
            Assert-True ($Uri.AbsoluteUri -ceq 'https://api.github.com/repos/PauloFBaldiFH/Elzevir/releases?per_page=1') 'Unexpected release listing'
            $response = New-Object PSObject -Property @{ ContentLength=-1 }
            $response | Add-Member ScriptMethod GetResponseStream { return [IO.MemoryStream]::new([Text.Encoding]::UTF8.GetBytes('[{"tag_name":"v0.1.0-alpha.2","prerelease":true}]'), $false) }
            $response | Add-Member ScriptMethod Dispose {}
            return $response
        }
        Assert-True ($Uri.AbsoluteUri -ceq "$official/releases/latest") 'Unexpected release lookup'
        $response = New-Object PSObject -Property @{ ResponseUri=[uri]$script:releaseUri }
        $response | Add-Member ScriptMethod Dispose {}
        return $response
    }
    $script:releaseUri = "$official/releases/tag/v2.0.0"
    Assert-True ((Get-ElzevirLatest $official) -ceq 'v2.0.0') 'Stable latest release was not preferred'
    $script:releaseUri = "$official/releases"
    Assert-True ((Get-ElzevirLatest $official) -ceq 'v0.1.0-alpha.2') 'Newest preview was not selected without a stable release'
} finally { Set-Item Function:Get-ElzevirResponse $originalResponse }
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('elzevir-bootstrap-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $originalResponse = (Get-Item Function:Get-ElzevirResponse).ScriptBlock
    $script:declaredLength = -1
    try {
        function Get-ElzevirResponse {
            $response = New-Object PSObject -Property @{ ContentLength=$script:declaredLength }
            $response | Add-Member ScriptMethod GetResponseStream { return [IO.MemoryStream]::new([byte[]](1,2,3), $false) }
            $response | Add-Member ScriptMethod Dispose {}
            return $response
        }
        $download = Join-Path $temporary 'download'
        Assert-Throws { Save-ElzevirDownload 'https://example.invalid/package' $download 2 }
        Assert-True (-not (Test-Path -LiteralPath $download)) 'Oversized partial download was kept'
        Save-ElzevirDownload 'https://example.invalid/package' $download 3
        Assert-True ((Get-Item -LiteralPath $download).Length -eq 3) 'Streaming download failed'
        Assert-Throws { Save-ElzevirDownload 'https://example.invalid/package' $download 3 }
        Assert-True ((Get-Item -LiteralPath $download).Length -eq 3) 'Existing download was modified'
        Remove-Item -LiteralPath $download
        $script:declaredLength = 4
        Assert-Throws { Save-ElzevirDownload 'https://example.invalid/package' $download 8 }
        Assert-True (-not (Test-Path -LiteralPath $download)) 'Truncated download was kept'
    } finally { Set-Item Function:Get-ElzevirResponse $originalResponse }
    $package = Join-Path $temporary 'fixture.zip'
    $zip = [IO.Compression.ZipFile]::Open($package, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($name in @('../outside','README.md','LICENSE')) {
            $entry = $zip.CreateEntry($name)
            $writer = New-Object IO.StreamWriter($entry.Open())
            try { $writer.Write('fictional') } finally { $writer.Dispose() }
        }
    } finally { $zip.Dispose() }
    $output = Join-Path $temporary 'output'
    New-Item -ItemType Directory -Path $output | Out-Null
    Assert-Throws { Expand-ElzevirPackage $package $output }
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $temporary 'outside'))) 'ZIP escaped the output directory'
} finally { Remove-Item -LiteralPath $temporary -Recurse -Force }
# The whole script must survive the antivirus scan that blocked `irm | iex`;
# -Plan stops before any download or change.
$plan = & ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $root 'install.ps1')))) -Plan -Version 'v1.2.3' | ConvertFrom-Json
Assert-True ($plan.version -ceq 'v1.2.3') 'Installer plan did not run'
Write-Output 'Windows bootstrap parser, checksum and extraction checks passed.'
