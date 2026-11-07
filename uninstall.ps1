#Requires -RunAsAdministrator
$ErrorActionPreference = 'SilentlyContinue'
$installDir = Join-Path $env:ProgramFiles 'NitroControl'
$dataDir = Join-Path $env:ProgramData 'NitroControl'

sc.exe stop NitroControlBroker | Out-Null
sc.exe delete NitroControlBroker | Out-Null
schtasks /Delete /TN NitroControlWidget /F | Out-Null
schtasks /Delete /TN NitroControlBroker /F | Out-Null
Get-Process nitro-widget,nitro-broker | Stop-Process -Force
Start-Sleep -Seconds 1

Remove-Item $dataDir -Recurse -Force
# Leave $installDir in place if this script is running from it; otherwise remove it.
if ($PSCommandPath -notlike ($installDir + '*')) {
    Remove-Item $installDir -Recurse -Force
}
Write-Host "已停止并删除 NitroControlBroker 服务、NitroControlWidget 任务。"
