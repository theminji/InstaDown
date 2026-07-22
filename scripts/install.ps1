$ErrorActionPreference = "Stop"

$Repository = if ($env:INSTADOWN_REPO) { $env:INSTADOWN_REPO } else { "__REPOSITORY__" }
$InstallDir = if ($env:INSTADOWN_INSTALL_DIR) {
    $env:INSTADOWN_INSTALL_DIR
} else {
    Join-Path $env:LOCALAPPDATA "Programs\instadown"
}
$Archive = "instadown-windows-x86_64.zip"
$Url = "https://github.com/$Repository/releases/latest/download/$Archive"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("instadown-" + [guid]::NewGuid())

try {
    New-Item -ItemType Directory -Path $TempDir | Out-Null
    Write-Host "Installing instadown from $Repository..."
    Invoke-WebRequest -Uri $Url -OutFile (Join-Path $TempDir $Archive)
    Expand-Archive -Path (Join-Path $TempDir $Archive) -DestinationPath $TempDir
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item (Join-Path $TempDir "instadown.exe") (Join-Path $InstallDir "instadown.exe") -Force

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $PathEntries = $UserPath -split ";" | Where-Object { $_ }
    if ($InstallDir -notin $PathEntries) {
        $NewPath = (@($PathEntries) + $InstallDir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    }

    Write-Host "Installed instadown to $InstallDir\instadown.exe"
    Write-Host "Open a new terminal, then make sure yt-dlp and ffmpeg are installed."
} finally {
    if (Test-Path $TempDir) {
        Remove-Item -Recurse -Force $TempDir
    }
}

