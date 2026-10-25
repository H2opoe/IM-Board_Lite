$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = $OutputEncoding
[Console]::InputEncoding = $OutputEncoding
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$env:IM_BOARD_DWS_NAMESPACE = __NAMESPACE__
$dwsCliPath = __CLI_PATH__
__NATIVE_INVOKER__
__BOUNDED_INVOKER__
if ((Get-FileHash -LiteralPath $dwsCliPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne __SHA256__) { throw 'DWS isolated runtime integrity check failed. Reinstall IM-Board.' }
$dwsCapability = Invoke-IMBoardDws --im-board-isolation-version
if ($LASTEXITCODE -ne 0 -or ([string]$dwsCapability).Trim() -cne 'im-board-dws-isolation-v1') { throw 'DWS isolated runtime capability check failed.' }
$dwsRegistryKey = 'HKCU:\Software\IMBoard\DwsCli\Profiles\' + $env:IM_BOARD_DWS_NAMESPACE + '\dws-cli'
$dwsProfileTokenPath = __TOKEN_PATH__
$dwsProfileConfigDir = __CONFIG_DIR__
$dwsMutexName = 'Global\IMBoard-DwsProfile-v2-' + [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value + '-' + $env:IM_BOARD_DWS_NAMESPACE
$dwsMutex = [System.Threading.Mutex]::new($false, $dwsMutexName)
$dwsAcquired = $false
try {
    try { $dwsAcquired = $dwsMutex.WaitOne(0) }
    catch [System.Threading.AbandonedMutexException] { $dwsAcquired = $true }
    if (-not $dwsAcquired) { throw 'IMBOARD_DWS_ACCOUNT_BUSY: 终端中的钉钉授权或读取尚未结束。请等待终端完成后再保存配置。' }
    $dwsRegistryRoot = 'HKCU:\Software\IMBoard\DwsCli\Profiles\' + $env:IM_BOARD_DWS_NAMESPACE
    if ((Test-Path -LiteralPath $dwsRegistryRoot) -and (Get-Item -LiteralPath $dwsRegistryRoot).GetValue('IMBoardDeleted', 0) -eq 1) { throw 'This IM-Board account was deleted. Create a new account to authorize again.' }
    $dwsNamespaceEmpty = $true
    if (Test-Path -LiteralPath $dwsRegistryKey) {
        $dwsNamespaceEmpty = (Get-Item -LiteralPath $dwsRegistryKey).GetValueNames().Count -eq 0
    }
    if (__MIGRATE_LEGACY__ -and $dwsNamespaceEmpty -and (Test-Path -LiteralPath $dwsProfileTokenPath)) {
        if ((Get-Item -LiteralPath $dwsProfileTokenPath).Length -gt 65536) { throw 'Legacy DWS account token is invalid. Authorize this account again.' }
        $dwsCiphertext = [System.IO.File]::ReadAllText($dwsProfileTokenPath).TrimStart([char]0xfeff).Trim()
        Add-Type -AssemblyName System.Security
        $dwsPlain = $null
        try {
            $dwsEntropy = [System.Text.Encoding]::UTF8.GetBytes('dws-cli' + [char]0 + 'auth-token')
            $dwsPlain = [System.Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String($dwsCiphertext), $dwsEntropy, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
            $dwsToken = [System.Text.Encoding]::UTF8.GetString($dwsPlain) | ConvertFrom-Json
            if (-not $dwsToken.access_token -and -not $dwsToken.refresh_token -and -not $dwsToken.persistent_code) { throw 'Invalid token payload' }
            $dwsProfilesPath = Join-Path $dwsProfileConfigDir 'profiles.json'
            if (Test-Path -LiteralPath $dwsProfilesPath) {
                $dwsProfiles = [System.IO.File]::ReadAllText($dwsProfilesPath) | ConvertFrom-Json
                $dwsSelectedName = $dwsProfiles.currentProfile
                if (-not $dwsSelectedName) { $dwsSelectedName = $dwsProfiles.primaryProfile }
                $dwsSelected = @($dwsProfiles.profiles | Where-Object { $_.name -eq $dwsSelectedName -or $_.corpId -eq $dwsSelectedName })
                if ($dwsSelected.Count -gt 0 -and $dwsSelected[0].corpId -cne $dwsToken.corp_id) { throw 'Account identity does not match saved token' }
                if ($dwsSelected.Count -gt 0 -and $dwsSelected[0].userId -and $dwsSelected[0].userId -cne $dwsToken.user_id) { throw 'User identity does not match saved token' }
            }
        } catch { throw 'Legacy DWS account token could not be verified. Authorize this account again.' }
        finally { if ($null -ne $dwsPlain) { [Array]::Clear($dwsPlain, 0, $dwsPlain.Length) }; $dwsToken = $null }
        New-Item -Path $dwsRegistryKey -Force | Out-Null
        Set-ItemProperty -LiteralPath $dwsRegistryKey -Name 'YXV0aC10b2tlbg' -Value $dwsCiphertext
        if ((Get-Item -LiteralPath $dwsRegistryKey).GetValue('YXV0aC10b2tlbg', $null) -cne $dwsCiphertext) { throw 'Legacy DWS account token migration failed.' }
        Remove-Item -LiteralPath $dwsProfileTokenPath -Force
        $dwsCiphertext = $null
    }
    $global:LASTEXITCODE = 0
    __BODY__
    $dwsExitCode = $LASTEXITCODE
} finally {
    if ($dwsAcquired) { $dwsMutex.ReleaseMutex() }
    $dwsMutex.Dispose()
}
$global:LASTEXITCODE = $dwsExitCode
