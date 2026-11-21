param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$File
)

$ErrorActionPreference = "Stop"

$pfx = $env:RENCORD_PFX
$password = $env:RENCORD_PFX_PASSWORD
$signtool = $env:RENCORD_SIGNTOOL
if (-not $pfx -or -not $password -or -not $signtool) {
    throw "RENCORD_PFX, RENCORD_PFX_PASSWORD, and RENCORD_SIGNTOOL must be set"
}

$servers = @(
    "http://timestamp.digicert.com",
    "http://timestamp.sectigo.com"
)

foreach ($server in $servers) {
    Write-Host "signing $File via $server"
    $proc = Start-Process -FilePath $signtool -ArgumentList @(
        "sign", "/fd", "SHA256", "/tr", $server, "/td", "SHA256",
        "/f", $pfx, "/p", $password, $File
    ) -PassThru -NoNewWindow
    $null = Wait-Process -Id $proc.Id -Timeout 90 -ErrorAction SilentlyContinue
    $proc.Refresh()
    if (-not $proc.HasExited) {
        & taskkill.exe /F /T /PID $proc.Id | Out-Null
        Write-Host "timestamp server timed out: $server"
        continue
    }
    if ($proc.ExitCode -eq 0) {
        if ($env:RENCORD_TIMESTAMP_FILE) {
            Set-Content -Path $env:RENCORD_TIMESTAMP_FILE -Value $server -Encoding ascii
        }
        exit 0
    }
    Write-Host "signtool exit $($proc.ExitCode) via $server"
}

throw "could not sign $File"
