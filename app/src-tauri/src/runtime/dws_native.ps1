# Windows PowerShell 5.1 turns redirected native stderr into ErrorRecords.
# Scope Continue to the native invocation only; security/file operations stay Stop.
function Invoke-IMBoardDws {
    $dwsSavedErrorPreference = $ErrorActionPreference
    $dwsNativeExitCode = $null
    try {
        $ErrorActionPreference = 'Continue'
        $global:LASTEXITCODE = $null
        & $dwsCliPath @args
        $dwsNativeExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $dwsSavedErrorPreference
    }
    if ($null -eq $dwsNativeExitCode) {
        throw 'DWS executable could not be started. Check the error above.'
    }
    $global:LASTEXITCODE = [int]$dwsNativeExitCode
}
