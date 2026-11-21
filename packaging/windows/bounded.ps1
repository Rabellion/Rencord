function Invoke-Bounded {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [string[]]$ArgumentList = @(),
        [Parameter(Mandatory = $true)][int]$TimeoutSec,
        [Parameter(Mandatory = $true)][string]$Label,
        [string]$RedirectStandardOutput
    )
    $params = @{
        FilePath     = $File
        ArgumentList = $ArgumentList
        PassThru     = $true
        NoNewWindow  = $true
    }
    if ($RedirectStandardOutput) {
        $params.RedirectStandardOutput = $RedirectStandardOutput
    }
    $proc = Start-Process @params
    $null = Wait-Process -Id $proc.Id -Timeout $TimeoutSec -ErrorAction SilentlyContinue
    $proc.Refresh()
    if (-not $proc.HasExited) {
        & taskkill.exe /F /T /PID $proc.Id | Out-Null
        throw "$Label timed out after $TimeoutSec seconds and was killed"
    }
    if ($proc.ExitCode -ne 0) {
        throw "$Label failed with exit code $($proc.ExitCode)"
    }
    return $proc
}
