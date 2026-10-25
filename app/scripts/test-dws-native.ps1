$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../src-tauri/src/runtime/dws_native.ps1')
if ($env:OS -eq 'Windows_NT') {
    $dwsCliPath = $env:ComSpec
    $successArgs = @('/d', '/c', 'echo. 1>&2 & echo native-notice 1>&2 & echo native-ok & exit /b 0')
    $failureArgs = @('/d', '/c', 'echo native-failure 1>&2 & exit /b 7')
} else {
    $dwsCliPath = '/bin/sh'
    $successArgs = @('-c', 'printf "\nnative-notice\n" >&2; printf "native-ok\n"; exit 0')
    $failureArgs = @('-c', 'printf "native-failure\n" >&2; exit 7')
}
& { Invoke-IMBoardDws @successArgs } 2>&1 | Tee-Object -Variable captured | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'stderr changed a successful native exit' }
$text = (@($captured | ForEach-Object { [string]$_ }) -join "`n")
if ($text -notmatch 'native-notice' -or $text -notmatch 'native-ok') { throw 'stdout/stderr was lost' }
if ($ErrorActionPreference -ne 'Stop') { throw 'caller error policy was weakened' }
& { Invoke-IMBoardDws @failureArgs } 2>&1 | Out-Null
if ($LASTEXITCODE -ne 7) { throw 'real nonzero exit was lost' }
$nextRan = $false
if ($LASTEXITCODE -eq 0) { $nextRan = $true }
if ($nextRan) { throw 'authorization continued after native failure' }
$dwsCliPath = Join-Path ([System.IO.Path]::GetTempPath()) ('imboard-absent-' + [Guid]::NewGuid().ToString() + '.exe')
$launchRejected = $false
try { & { Invoke-IMBoardDws } 2>&1 | Out-Null } catch { $launchRejected = $true }
if (-not $launchRejected) { throw 'launch failure reused a stale exit code' }
$cmdletStopped = $false
try { Get-Item -LiteralPath $dwsCliPath | Out-Null } catch { $cmdletStopped = $true }
if (-not $cmdletStopped) { throw 'security/file cmdlet errors no longer stop' }
@{version=$PSVersionTable.PSVersion.ToString();stderrPreserved=$true;zeroExitPreserved=$true;nonzeroExitPreserved=$true;launchFailureRejected=$launchRejected;cmdletStopPreserved=$cmdletStopped} | ConvertTo-Json -Compress
exit 0
