# Embedded after the escaped paths, version and localized messages supplied by Rust.
function Write-UpdateLog([string]$message) {
    try {
        Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) $message" -Encoding UTF8
    } catch {}
}

try {
    Write-UpdateLog "waiting for application process $applicationProcessId before setup update"
    Write-UpdateLog "installer: $installer; target: $current; expected version: $expectedVersion; setup log: $setupLog"
    Wait-Process -Id $applicationProcessId -ErrorAction SilentlyContinue
    $installDir = [System.IO.Path]::GetDirectoryName($current)
    # Start-Process joins arguments into a command line, so quote each path explicitly.
    $arguments = @('/SP-', '/NORESTART', '/CURRENTUSER', '/RESTARTEXITCODE=3010',
        ('/DIR="{0}"' -f $installDir), ('/LOG="{0}"' -f $setupLog))
    $setup = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Normal -PassThru
    # -Wait waits for the whole process tree, including the app launched by the finish page.
    # Retain the setup handle so its exit code remains available after a quick exit.
    $null = $setup.Handle
    $setup.WaitForExit()
    $exitCode = $setup.ExitCode
    $setup.Dispose()
    Write-UpdateLog "setup exited with code $exitCode"
    if ($exitCode -in @(2, 5)) {
        Write-UpdateLog "setup cancelled or aborted; installer retained for retry: $installer"
        exit $exitCode
    }
    if ($exitCode -eq 3010) {
        Write-UpdateLog 'system restart required; installation not yet verified; installer retained'
        exit $exitCode
    }
    if ($null -eq $exitCode -or $exitCode -ne 0) {
        throw "setup exited with code $exitCode"
    }
    $installedVersion = (Get-Item -LiteralPath $current).VersionInfo.ProductVersion
    if ($installedVersion -ne $expectedVersion) {
        throw "installed setup version mismatch: expected $expectedVersion, got $installedVersion"
    }
    # Only the wizard owns the launch choice. Never override an unchecked finish-page option.
    Write-UpdateLog 'setup completed and installed version verified; application launch is controlled by the wizard'
    Remove-Item -LiteralPath $installer -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $script -Force -ErrorAction SilentlyContinue
} catch {
    Write-UpdateLog "setup update failed: $($_.Exception.Message); installer retained: $installer; setup log: $setupLog"
    # The application has exited, so its in-app feedback service is no longer available.
    try {
        $shell = New-Object -ComObject WScript.Shell
        $null = $shell.Popup(($failureDescription + "`r`n`r`n" + $_.Exception.Message), 0, $failureTitle, 16)
    } catch {
        Write-UpdateLog "could not display update failure: $($_.Exception.Message)"
    }
    exit 1
}
