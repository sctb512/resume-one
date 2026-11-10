param(
    [ValidateSet('debug', 'release')]
    [string]$Profile = 'release',
    [ValidateSet('x64', 'arm64')]
    [string]$Architecture,
    [switch]$Archive
)

$ErrorActionPreference = 'Stop'

if (-not $IsWindows -and $PSVersionTable.PSEdition -eq 'Core') {
    throw 'Run this script on Windows with the MSVC build tools and Windows SDK installed.'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Architecture) {
    $Architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
}
$target = switch ($Architecture) {
    'x64' { 'x86_64-pc-windows-msvc' }
    'arm64' { 'aarch64-pc-windows-msvc' }
    default { throw "Unsupported Windows architecture: $Architecture" }
}
$previousFxc = $env:GPUI_FXC_PATH
$previousPackagedFonts = $env:RESUME_ONE_PACKAGED_FONTS
Push-Location $repoRoot
try {
    if (-not $env:GPUI_FXC_PATH) {
        $sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
        if (Test-Path -LiteralPath $sdkBin -PathType Container) {
            $fxc = Get-ChildItem -LiteralPath $sdkBin -Directory |
                Sort-Object Name -Descending |
                ForEach-Object {
                    $sdkVersion = $_.FullName
                    foreach ($hostArch in @($Architecture, 'x64', 'x86') | Select-Object -Unique) {
                        Join-Path $sdkVersion "$hostArch\fxc.exe"
                    }
                } |
                Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
                Select-Object -First 1
            if ($fxc) { $env:GPUI_FXC_PATH = $fxc }
        }
    }
    if (-not $env:GPUI_FXC_PATH -or -not (Test-Path -LiteralPath $env:GPUI_FXC_PATH -PathType Leaf)) {
        throw 'The Windows SDK HLSL compiler fxc.exe is required; install the Windows SDK or set GPUI_FXC_PATH.'
    }
    $buildArgs = @('build', '--locked', '--target', $target, '-p', 'resume-one-app', '-p', 'resume-one-cli', '-p', 'resume-one-theme-migrate')
    if ($Profile -eq 'release') {
        $buildArgs += '--release'
    }
    $env:RESUME_ONE_PACKAGED_FONTS = '1'
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed with exit code $LASTEXITCODE."
    }

    $buildDir = Join-Path $repoRoot "target\$target\$Profile"
    $bundle = Join-Path $buildDir 'Resume One'
    New-Item -ItemType Directory -Path $bundle -Force | Out-Null
    foreach ($name in @('resume-one-app', 'resume-one', 'resume-one-theme-migrate')) {
        $source = Join-Path $buildDir "$name.exe"
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Missing built binary: $source"
        }
        Copy-Item -LiteralPath $source -Destination (Join-Path $bundle "$name.exe") -Force
    }

    foreach ($name in @('resume-one', 'resume-one-theme-migrate')) {
        & (Join-Path $bundle "$name.exe") --version | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "$name --version failed with exit code $LASTEXITCODE."
        }
    }
    foreach ($folder in @('docs', 'spec', 'assets\themes', 'assets\fonts', 'vendor\gpui-component')) {
        New-Item -ItemType Directory -Path (Join-Path $bundle $folder) -Force | Out-Null
    }
    Copy-Item -LiteralPath (Join-Path $repoRoot 'README.md'), (Join-Path $repoRoot 'assets\example.resume.json') -Destination $bundle -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'resume-one-theme-spec-v2.md') -Destination $bundle -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\example.resume.json') -Destination (Join-Path $bundle 'assets') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'vendor\gpui-component\LICENSE-APACHE') -Destination (Join-Path $bundle 'vendor\gpui-component') -Force
    Copy-Item -Path (Join-Path $repoRoot 'assets\fonts\OFL-*.txt') -Destination (Join-Path $bundle 'assets\fonts') -Force
    $fontFiles = @(
        (Join-Path $repoRoot 'assets\fonts\NotoSansSC-Regular.ttf'),
        (Join-Path $repoRoot 'assets\fonts\NotoSerifSC-SemiBold.ttf')
    )
    Copy-Item -LiteralPath $fontFiles -Destination (Join-Path $bundle 'assets\fonts') -Force
    Copy-Item -Path (Join-Path $repoRoot 'docs\*.md') -Destination (Join-Path $bundle 'docs') -Force
    Copy-Item -Path (Join-Path $repoRoot 'spec\*') -Destination (Join-Path $bundle 'spec') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'resume-one-theme-spec-v2.md') -Destination (Join-Path $bundle 'spec') -Force
    Copy-Item -Path (Join-Path $repoRoot 'assets\themes\*.theme.json') -Destination (Join-Path $bundle 'assets\themes') -Force
    if ($env:RESUME_ONE_RELEASE_TAG) {
        Set-Content -LiteralPath (Join-Path $bundle 'release-tag.txt') -Value $env:RESUME_ONE_RELEASE_TAG -Encoding ascii
    }
    $smokeDir = Join-Path ([IO.Path]::GetTempPath()) ("resume-one-qa-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $smokeDir | Out-Null
    try {
        & (Join-Path $bundle 'resume-one.exe') export --file (Join-Path $bundle 'example.resume.json') `
            --format pdf --out (Join-Path $smokeDir 'example.pdf') | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "Bundled font export failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Remove-Item -LiteralPath $smokeDir -Recurse -Force
    }
    if ($Archive) {
        $archivePath = Join-Path $buildDir "Resume-One-Windows-$Architecture-unsigned.zip"
        if (Test-Path -LiteralPath $archivePath) {
            throw "Archive already exists; move it before packaging: $archivePath"
        }
        Compress-Archive -LiteralPath $bundle -DestinationPath $archivePath
        Write-Output $archivePath
    }
    Write-Output $bundle
}
finally {
    $env:GPUI_FXC_PATH = $previousFxc
    $env:RESUME_ONE_PACKAGED_FONTS = $previousPackagedFonts
    Pop-Location
}
