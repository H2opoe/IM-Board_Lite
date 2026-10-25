$ErrorActionPreference = 'Stop'
. ([ScriptBlock]::Create((Get-Content -LiteralPath (Join-Path $PSScriptRoot '../src-tauri/src/runtime/dws_bounded.ps1') -Raw -Encoding UTF8)))
$work = Join-Path ([IO.Path]::GetTempPath()) ('imboard-dws-bounded-' + [Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    if ($env:OS -eq 'Windows_NT') {
        $dwsCliPath = $env:ComSpec
        $ok = @('/d','/c','echo bounded-ok & echo bounded-notice 1>&2 & exit /b 0')
        $bad = @('/d','/c','echo bounded-failure 1>&2 & exit /b 7')
        $slow = @('-NoLogo','-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 20')
    } else {
        $dwsCliPath = '/bin/sh'
        $ok = @('-c','printf "bounded-ok\n"; printf "bounded-notice\n" >&2; exit 0')
        $bad = @('-c','printf "bounded-failure\n" >&2; exit 7')
        $slow = @('-c','exec sleep 20')
    }
    $out = @(Invoke-IMBoardDwsBounded -TimeoutSeconds 8 -NativeArguments $ok) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $out -notmatch 'bounded-ok' -or $out -notmatch 'bounded-notice') { throw 'success output or stderr lost' }
    $out = @(Invoke-IMBoardDwsBounded -TimeoutSeconds 8 -NativeArguments $bad) -join "`n"
    if ($LASTEXITCODE -ne 7 -or $out -notmatch 'bounded-failure') { throw 'nonzero native exit lost' }
    if ($env:OS -eq 'Windows_NT') { $dwsCliPath = (Get-Process -Id $PID).Path }
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $rejected = $false
    try { Invoke-IMBoardDwsBounded -TimeoutSeconds 1 -NativeArguments $slow | Out-Null } catch {
        if ($_.Exception.Message -notmatch 'IMBOARD_DWS_PERMISSION_TIMEOUT') { throw }
        $rejected = $true
    }
    if (-not $rejected -or $clock.Elapsed.TotalSeconds -gt 7 -or $LASTEXITCODE -ne 124) { throw 'deadline did not stop the owned process' }
    # Windows argv quote rules: empty, whitespace, embedded quotes, trailing slash.
    if ((ConvertTo-IMBoardNativeArgument '') -cne '""') { throw 'empty argument quote failed' }
    if ((ConvertTo-IMBoardNativeArgument 'a b') -cne '"a b"') { throw 'whitespace argument quote failed' }
    if ((ConvertTo-IMBoardNativeArgument 'a"b') -cne '"a\"b"') { throw 'embedded quote failed' }
    if ($ErrorActionPreference -cne 'Stop') { throw 'caller error policy changed' }
    @{success=$true;stdoutStderrPreserved=$true;exitCodePreserved=$true;deadlineStopsOwnedProcess=$true;elapsedSeconds=[Math]::Round($clock.Elapsed.TotalSeconds,2)} | ConvertTo-Json -Compress
} finally { Remove-Item -LiteralPath $work -Recurse -Force }
