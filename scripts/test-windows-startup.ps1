param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$TimeoutSeconds = 20
)

$ErrorActionPreference = 'Stop'
$executablePath = (Resolve-Path -LiteralPath $Executable).Path
# This is an interactive UI smoke test: the real window must paint to exercise its render loop.
$testProcess = Start-Process -FilePath $executablePath -WindowStyle Normal -PassThru
try {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $responsiveSamples = 0
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $testProcess.Refresh()
        if ($testProcess.HasExited) {
            throw "Application exited during startup (exit code $($testProcess.ExitCode))."
        }
        if ($testProcess.MainWindowHandle -eq [IntPtr]::Zero) {
            continue
        }
        # A first painted frame is insufficient: notification loops can starve the message pump.
        if (-not $testProcess.Responding) {
            throw 'Application window is not responding after startup.'
        }
        $responsiveSamples++
        if ($responsiveSamples -ge 10) {
            Write-Output 'PASS: application window remained responsive for 10 consecutive samples.'
            exit 0
        }
    }
    throw 'Timed out waiting for a responsive application window.'
}
finally {
    $testProcess.Refresh()
    if (-not $testProcess.HasExited) {
        # Only the process created by this test is stopped; existing user windows are untouched.
        Stop-Process -Id $testProcess.Id
    }
}
