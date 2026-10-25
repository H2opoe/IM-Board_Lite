# Only non-interactive PAT granting uses this bounded runner. Keep the owned
# Process object until exit is confirmed; never kill by a reusable PID or name.
function ConvertTo-IMBoardNativeArgument([string]$Value) {
    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
    return '"' + [regex]::Replace([regex]::Replace($Value, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
}
function Invoke-IMBoardDwsBounded {
    param([int]$TimeoutSeconds = 180, [string[]]$NativeArguments)
    if ($TimeoutSeconds -le 0) { throw 'Invalid DWS permission timeout.' }
    $dwsStartInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $dwsStartInfo.FileName = $dwsCliPath
    $dwsStartInfo.Arguments = (($NativeArguments | ForEach-Object { ConvertTo-IMBoardNativeArgument $_ }) -join ' ')
    $dwsStartInfo.UseShellExecute = $false
    $dwsStartInfo.CreateNoWindow = $true
    $dwsStartInfo.RedirectStandardInput = $true
    $dwsStartInfo.RedirectStandardOutput = $true
    $dwsStartInfo.RedirectStandardError = $true
    $dwsStartInfo.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
    $dwsStartInfo.StandardErrorEncoding = $dwsStartInfo.StandardOutputEncoding
    $dwsProcess = [System.Diagnostics.Process]::new()
    $dwsProcess.StartInfo = $dwsStartInfo
    $dwsStarted = $false
    $dwsConfirmedExit = $false
    $global:LASTEXITCODE = $null
    try {
        $dwsStarted = $dwsProcess.Start()
        if (-not $dwsStarted) { throw 'DWS permission process could not be started.' }
        $dwsProcess.StandardInput.Close()
        $dwsOutput = $dwsProcess.StandardOutput.ReadLineAsync()
        $dwsError = $dwsProcess.StandardError.ReadLineAsync()
        $dwsTimer = [System.Diagnostics.Stopwatch]::StartNew()
        while ($null -ne $dwsOutput -or $null -ne $dwsError -or -not $dwsProcess.HasExited) {
            if ($dwsTimer.Elapsed.TotalSeconds -ge $TimeoutSeconds) {
                if (-not $dwsProcess.HasExited) { $dwsProcess.Kill() }
                if (-not $dwsProcess.WaitForExit(5000)) { throw 'IMBOARD_DWS_PERMISSION_EXIT_UNCONFIRMED' }
                $dwsConfirmedExit = $true
                $global:LASTEXITCODE = 124
                throw 'IMBOARD_DWS_PERMISSION_TIMEOUT: 登录凭据已保留，消息读取权限授权未完成。请重试权限授权。'
            }
            if ($null -ne $dwsOutput -and $dwsOutput.IsCompleted) {
                $dwsLine = $dwsOutput.GetAwaiter().GetResult()
                if ($null -eq $dwsLine) { $dwsOutput = $null }
                else { Write-Output $dwsLine; $dwsOutput = $dwsProcess.StandardOutput.ReadLineAsync() }
            }
            if ($null -ne $dwsError -and $dwsError.IsCompleted) {
                $dwsLine = $dwsError.GetAwaiter().GetResult()
                if ($null -eq $dwsLine) { $dwsError = $null }
                else { Write-Output $dwsLine; $dwsError = $dwsProcess.StandardError.ReadLineAsync() }
            }
            Start-Sleep -Milliseconds 20
        }
        $dwsConfirmedExit = $dwsProcess.WaitForExit(5000)
        if (-not $dwsConfirmedExit) { throw 'IMBOARD_DWS_PERMISSION_EXIT_UNCONFIRMED' }
        $global:LASTEXITCODE = $dwsProcess.ExitCode
    } finally {
        if ($dwsStarted -and -not $dwsConfirmedExit) {
            if (-not $dwsProcess.HasExited) { $dwsProcess.Kill() }
            if (-not $dwsProcess.WaitForExit(5000)) { throw 'IMBOARD_DWS_PERMISSION_EXIT_UNCONFIRMED' }
        }
        $dwsProcess.Dispose()
    }
}
