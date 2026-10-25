$ErrorActionPreference = 'Stop'
$dwsNamespace = __NAMESPACE__
$dwsRegistryRoot = 'HKCU:\Software\IMBoard\DwsCli\Profiles\' + $dwsNamespace
$dwsMutexName = 'Global\IMBoard-DwsProfile-v2-' + [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value + '-' + $dwsNamespace
$dwsMutex = [System.Threading.Mutex]::new($false, $dwsMutexName)
$dwsAcquired = $false
try {
    try { $dwsAcquired = $dwsMutex.WaitOne(0) }
    catch [System.Threading.AbandonedMutexException] { $dwsAcquired = $true }
    if (-not $dwsAcquired) { throw 'This DWS account is busy. Cleanup will be retried.' }
    # Write the tombstone first. A delayed session must never recreate deleted credentials.
    New-Item -Path $dwsRegistryRoot -Force | Out-Null
    New-ItemProperty -LiteralPath $dwsRegistryRoot -Name 'IMBoardDeleted' -PropertyType DWord -Value 1 -Force | Out-Null
    if ((Get-Item -LiteralPath $dwsRegistryRoot).GetValue('IMBoardDeleted', 0) -ne 1) { throw 'DWS deletion marker could not be verified.' }
    foreach ($child in @(Get-ChildItem -LiteralPath $dwsRegistryRoot)) {
        Remove-Item -LiteralPath $child.PSPath -Recurse -Force
    }
    if (@(Get-ChildItem -LiteralPath $dwsRegistryRoot).Count -ne 0) { throw 'DWS credentials could not be removed.' }
} finally {
    if ($dwsAcquired) { $dwsMutex.ReleaseMutex() }
    $dwsMutex.Dispose()
}
