param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$File
)

$ErrorActionPreference = "Stop"
$pfx = $env:RENCORD_PFX
$password = $env:RENCORD_PFX_PASSWORD
$signtool = $env:RENCORD_SIGNTOOL
if (-not $pfx -or -not $password -or -not $signtool) {
    Write-Error "RENCORD_PFX, RENCORD_PFX_PASSWORD, and RENCORD_SIGNTOOL must be set"
    exit 1
}

$servers = @(
    "http://timestamp.digicert.com",
    "http://timestamp.sectigo.com",
    "http://timestamp.acs.microsoft.com"
)

foreach ($server in $servers) {
    Write-Host "signing $File via $server"
    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo.FileName = $signtool
    $proc.StartInfo.Arguments = "sign /fd SHA256 /tr $server /td SHA256 /f `"$pfx`" /p `"$password`" `"$File`""
    $proc.StartInfo.UseShellExecute = $false
    $proc.Start() | Out-Null
    if (-not $proc.WaitForExit(90000)) {
        try { $proc.Kill() } catch {}
        Write-Host "timestamp server timed out: $server"
        continue
    }
    if ($proc.ExitCode -eq 0) {
        Set-Content -Path $env:RENCORD_TIMESTAMP_FILE -Value $server -Encoding ascii
        exit 0
    }
    Write-Host "signtool exit $($proc.ExitCode) via $server"
}

exit 1
