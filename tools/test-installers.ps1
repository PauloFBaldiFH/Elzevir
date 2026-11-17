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
Assert-True (-not (Test-ElzevirVersion 'v01.2.3')) 'Noncanonical version accepted'
$hash = 'a' * 64
$sums = "$hash  package.zip`n"
Assert-True ((Get-ElzevirChecksum $sums 'package.zip') -ceq $hash) 'Checksum was not exact'
Assert-Throws { Get-ElzevirChecksum ($sums + $sums) 'package.zip' }
Assert-Throws { Get-ElzevirChecksum $sums 'another.zip' }
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('elzevir-bootstrap-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
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
Write-Output 'Windows bootstrap parser, checksum and extraction checks passed.'
