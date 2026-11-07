#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$base = Split-Path -Parent $MyInvocation.MyCommand.Path
$installDir = Join-Path $env:ProgramFiles 'NitroControl'
$dataDir = Join-Path $env:ProgramData 'NitroControl'

function Find-Exe([string]$name) {
    $candidates = @(
        (Join-Path $base $name),
        (Join-Path $base ('target\release\' + $name))
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    throw "找不到 $name，请先 cargo build --release"
}

$srcBroker = Find-Exe 'nitro-broker.exe'
$srcWidget = Find-Exe 'nitro-widget.exe'

# stop old service / task / processes
sc.exe stop NitroControlBroker 2>$null | Out-Null
sc.exe delete NitroControlBroker 2>$null | Out-Null
schtasks /Delete /TN NitroControlBroker /F 2>$null | Out-Null
Get-Process nitro-broker,nitro-widget -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

New-Item -ItemType Directory -Force -Path $installDir, $dataDir | Out-Null
$broker = Join-Path $installDir 'nitro-broker.exe'
$widget = Join-Path $installDir 'nitro-widget.exe'

if ($srcBroker -ne $broker) { Copy-Item $srcBroker $broker -Force }
if ($srcWidget -ne $widget) { Copy-Item $srcWidget $widget -Force }
$self = $MyInvocation.MyCommand.Path
if ($self -ne (Join-Path $installDir 'install.ps1')) {
    Copy-Item $self (Join-Path $installDir 'install.ps1') -Force
}
$uninstall = Join-Path $base 'uninstall.ps1'
if ((Test-Path $uninstall) -and ($uninstall -ne (Join-Path $installDir 'uninstall.ps1'))) {
    Copy-Item $uninstall (Join-Path $installDir 'uninstall.ps1') -Force
}

# Program Files: only SYSTEM/Administrators can modify, Users can run
icacls $installDir /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX" | Out-Null

# user SID for pipe ACL, config readable but not writable by the user
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
Set-Content -Path (Join-Path $dataDir 'user.sid') -Value $sid -Encoding ASCII
icacls $dataDir /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)R" | Out-Null

# LocalSystem service, auto start; no scheduled task for the privileged part
$bin = '"' + $broker + '"'
& sc.exe create NitroControlBroker binPath= $bin start= demand obj= LocalSystem DisplayName= NitroControlBroker | Out-Null
Set-ItemProperty "HKLM:SYSTEMCurrentControlSetServicesNitroControlBroker" -Name DisplayName -Value "Nitro Control Broker"
& sc.exe description NitroControlBroker "Nitro Control hardware broker (WMI, LocalSystem)" | Out-Null
& sc.exe failure NitroControlBroker reset= 86400 actions= restart/5000/restart/10000/restart/30000 | Out-Null
& sc.exe sdset NitroControlBroker "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;RPLC;;;$sid)" | Out-Null

Write-Host "已安装 NitroControlBroker 服务（LocalSystem，按需启动）。"
Write-Host "从开始菜单或资源管理器运行 $widget 即可；UI 以普通权限运行。"
Write-Host "服务空闲 5 分钟后会自动停止。"
Write-Host "安装目录: $installDir"
