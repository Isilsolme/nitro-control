Unicode true
!include "MUI2.nsh"

!define APPNAME "Nitro Control"
!define VERSION "0.1.1"
!define PUBLISHER "Nitro Control contributors"
!define SERVICE "NitroControlBroker"

Var SID
Var CHECK
Var LEN
Var DATADIR

Name "Nitro Control"
Caption "Nitro Control 0.1.1 安装程序"
OutFile "..\dist\NitroControl-Setup-0.1.1.exe"
InstallDir "$PROGRAMFILES64\NitroControl"
InstallDirRegKey HKLM "Software\NitroControl" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
SetCompressorDictSize 32
BrandingText "Nitro Control 0.1.1"
Icon "..\widget\assets\icon.ico"
UninstallIcon "..\widget\assets\icon.ico"
VIProductVersion "0.1.1.0"
VIAddVersionKey "ProductName" "Nitro Control"
VIAddVersionKey "CompanyName" "Nitro Control contributors"
VIAddVersionKey "FileDescription" "Nitro Control installer"
VIAddVersionKey "FileVersion" "0.1.1.0"
VIAddVersionKey "ProductVersion" "0.1.1"
VIAddVersionKey "LegalCopyright" "GPL-3.0-or-later"

!define MUI_ABORTWARNING
!define MUI_ICON "..\widget\assets\icon.ico"
!define MUI_UNICON "..\widget\assets\icon.ico"
!define MUI_FINISHPAGE_TEXT "安装完成。请从开始菜单启动 Nitro Control。服务按需启动，空闲 5 分钟后自动停止。"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Section "Nitro Control (required)" SEC_CORE
  SectionIn RO
  SetShellVarContext all
  SetOutPath "$INSTDIR"

  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop NitroControlBroker'
  nsExec::ExecToLog '"$SYSDIR\sc.exe" delete NitroControlBroker'
  nsExec::ExecToLog 'schtasks /Delete /TN NitroControlWidget /F'
  nsExec::ExecToLog 'taskkill /IM nitro-widget.exe /F'
  nsExec::ExecToLog 'taskkill /IM nitro-broker.exe /F'
  Sleep 1200

  File "..\target\release\nitro-broker.exe"
  File "..\target\release\nitro-widget.exe"
  File "..\install.ps1"
  File "..\uninstall.ps1"
  File "..\README.md"
  File "..\README.en.md"
  File "..\LICENSE"


  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -Command "[Security.Principal.WindowsIdentity]::GetCurrent().User.Value"'
  Pop $0
  Pop $1
  StrCmp $1 "" sid_fail
  StrLen $LEN $1
  IntOp $LEN $LEN - 2
  StrCpy $SID $1 $LEN
  StrCmp $SID "" sid_fail
  StrCpy $CHECK $SID 4
  StrCmp $CHECK "S-1-" sid_ok
sid_fail:
  MessageBox MB_ICONSTOP "无法获取当前用户 SID，安装中止。"
  Abort
sid_ok:

  StrCpy $DATADIR "$WINDIR\..\ProgramData\NitroControl"
  CreateDirectory "$DATADIR"
  FileOpen $0 "$DATADIR\user.sid" w
  FileWrite $0 "$SID"
  FileClose $0

  nsExec::ExecToLog 'icacls "$INSTDIR" /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX"'
  nsExec::ExecToLog 'icacls "$DATADIR" /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)R"'

  nsExec::ExecToLog '"$SYSDIR\sc.exe" create NitroControlBroker binPath= $\"$INSTDIR\nitro-broker.exe$\" start= demand obj= LocalSystem DisplayName= NitroControlBroker'
  nsExec::ExecToStack '"$SYSDIR\sc.exe" query NitroControlBroker'
  Pop $0
  Pop $1
  StrCmp $0 "0" service_ok
    MessageBox MB_ICONSTOP "NitroControlBroker 服务创建失败。$\r$\n$1"
    Abort
service_ok:
  WriteRegStr HKLM "SYSTEM\CurrentControlSet\Services\NitroControlBroker" "DisplayName" "Nitro Control Broker"
  nsExec::ExecToLog '"$SYSDIR\sc.exe" description NitroControlBroker "Nitro Control hardware broker (WMI, LocalSystem)"'
  nsExec::ExecToLog '"$SYSDIR\sc.exe" failure NitroControlBroker reset= 86400 actions= restart/5000/restart/10000/restart/30000'

  nsExec::ExecToLog '"$SYSDIR\sc.exe" sdset NitroControlBroker D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;RPLC;;;$SID)'

  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Nitro Control"
  CreateShortcut "$SMPROGRAMS\Nitro Control\Nitro Control.lnk" "$INSTDIR\nitro-widget.exe" "" "$INSTDIR\nitro-widget.exe" 0
  CreateShortcut "$SMPROGRAMS\Nitro Control\Uninstall Nitro Control.lnk" "$INSTDIR\uninstall.exe"
  CreateShortcut "$DESKTOP\Nitro Control.lnk" "$INSTDIR\nitro-widget.exe" "" "$INSTDIR\nitro-widget.exe" 0

  WriteRegStr HKLM "Software\NitroControl" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "DisplayName" "Nitro Control"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "DisplayVersion" "0.1.1"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "Publisher" "Nitro Control contributors"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "DisplayIcon" "$INSTDIR\nitro-widget.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl" "NoRepair" 1
SectionEnd

Section /o "Start widget at login (optional)" SEC_AUTOSTART
  SetShellVarContext all
  CreateShortcut "$SMSTARTUP\Nitro Control.lnk" "$INSTDIR\nitro-widget.exe"
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop NitroControlBroker'
  nsExec::ExecToLog '"$SYSDIR\sc.exe" delete NitroControlBroker'
  nsExec::ExecToLog 'schtasks /Delete /TN NitroControlWidget /F'
  nsExec::ExecToLog 'taskkill /IM nitro-widget.exe /F'
  Sleep 800
  StrCpy $DATADIR "$WINDIR\..\ProgramData\NitroControl"

  Delete "$INSTDIR\nitro-broker.exe"
  Delete "$INSTDIR\nitro-widget.exe"
  Delete "$INSTDIR\install.ps1"
  Delete "$INSTDIR\uninstall.ps1"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\README.en.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  RMDir /r "$DATADIR"

  Delete "$SMPROGRAMS\Nitro Control\Nitro Control.lnk"
  Delete "$SMPROGRAMS\Nitro Control\Uninstall Nitro Control.lnk"
  RMDir "$SMPROGRAMS\Nitro Control"
  Delete "$DESKTOP\Nitro Control.lnk"
  Delete "$SMSTARTUP\Nitro Control.lnk"

  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\NitroControl"
  DeleteRegKey HKLM "Software\NitroControl"
SectionEnd
