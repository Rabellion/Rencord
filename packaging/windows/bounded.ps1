function Invoke-Bounded {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [string[]]$ArgumentList = @(),
        [string]$ArgumentString,
        [Parameter(Mandatory = $true)][int]$TimeoutSec,
        [Parameter(Mandatory = $true)][string]$Label,
        [string]$RedirectStandardOutput
    )
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $File
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    if ($ArgumentString) {
        $start.Arguments = $ArgumentString
    } elseif ($ArgumentList.Count -gt 0) {
        $start.Arguments = ($ArgumentList | ForEach-Object {
            if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
        }) -join ' '
    }
    if ($RedirectStandardOutput) {
        $start.RedirectStandardOutput = $true
    }
    $proc = [Diagnostics.Process]::Start($start)
    $null = Wait-Process -Id $proc.Id -Timeout $TimeoutSec -ErrorAction SilentlyContinue
    $proc.Refresh()
    if ($proc.HasExited -and $RedirectStandardOutput) {
        Set-Content -Path $RedirectStandardOutput -Value $proc.StandardOutput.ReadToEnd() -Encoding utf8
    }
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
