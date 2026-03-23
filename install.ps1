#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\onport"
)

$ErrorActionPreference = 'Stop'

# --- Constants ---
$GITLAB_BASE   = "https://gitlab.cherkaoui.ch"
$PROJECT_PATH  = "HadiCherkaoui%2Fonport"
$BINARY        = "onport-windows-x86_64.exe"

try {
    # --- Fetch the latest release tag from the GitLab Releases API ---
    Write-Host "Fetching latest onport release..."
    $releases = Invoke-RestMethod -Uri "$GITLAB_BASE/api/v4/projects/$PROJECT_PATH/releases"
    $tag = $releases[0].tag_name
    Write-Host "Latest release: $tag"

    # --- Build the download URL for the generic package registry ---
    # URL format: /api/v4/projects/:id/packages/generic/:package_name/:version/:filename
    $downloadUrl = "$GITLAB_BASE/api/v4/projects/$PROJECT_PATH/packages/generic/onport/$tag/$BINARY"
    Write-Host "Downloading from: $downloadUrl"

    # --- Create the install directory if it does not exist ---
    if (-not (Test-Path -Path $InstallDir)) {
        Write-Host "Creating install directory: $InstallDir"
        New-Item -ItemType Directory -Path $InstallDir | Out-Null
    }

    # --- Download the binary and save it as onport.exe ---
    $destination = Join-Path $InstallDir "onport.exe"
    Invoke-WebRequest -Uri $downloadUrl -OutFile $destination -UseBasicParsing
    Write-Host "Binary saved to: $destination"

    # --- Add InstallDir to the current user's PATH if not already present ---
    $userPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
    if ($userPath -split ";" -notcontains $InstallDir) {
        Write-Host "Adding $InstallDir to your PATH..."
        $newPath = ($userPath.TrimEnd(";") + ";" + $InstallDir).TrimStart(";")
        [System.Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
        Write-Host "PATH updated."
    } else {
        Write-Host "$InstallDir is already in your PATH."
    }

    # --- Success ---
    Write-Host ""
    Write-Host "onport $tag installed successfully to: $destination"
    Write-Host "Please restart your terminal (or open a new shell) for the PATH change to take effect."
} catch {
    Write-Host ""
    Write-Host "Error: installation failed."
    Write-Host $_.Exception.Message
    exit 1
}
