; SPDX-License-Identifier: GPL-3.0-or-later
; Per-user installer for Buginotch (no admin rights needed).
; Build: makensis -DVERSION=0.1.0 installer/buginotch.nsi

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

Unicode true
Name "Buginotch"
OutFile "..\dist\Buginotch-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\Buginotch"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Buginotch"
!define OLD_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\IsleWin"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
; The finish page's two checkboxes: start now, and start with Windows.
!define MUI_FINISHPAGE_RUN "$INSTDIR\buginotch.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Jalankan Buginotch sekarang"
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Mulai dengan Windows"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION SetAutostart
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Indonesian"

Function SetAutostart
  WriteRegStr HKCU "${RUN_KEY}" "Buginotch" '"$INSTDIR\buginotch.exe"'
FunctionEnd

; Buginotch was called Isle for Windows (IsleWin): remove that install, keep its autostart
; choice, and leave the settings and data in %APPDATA%\IsleWin alone.
Function MigrateOldInstall
  ReadRegStr $0 HKCU "${RUN_KEY}" "IsleWin"
  StrCmp $0 "" noautostart
  WriteRegStr HKCU "${RUN_KEY}" "Buginotch" '"$INSTDIR\buginotch.exe"'
  DeleteRegValue HKCU "${RUN_KEY}" "IsleWin"
  noautostart:
  Delete "$LOCALAPPDATA\Programs\IsleWin\isle-win.exe"
  Delete "$LOCALAPPDATA\Programs\IsleWin\LICENSE"
  Delete "$LOCALAPPDATA\Programs\IsleWin\NOTICE.md"
  Delete "$LOCALAPPDATA\Programs\IsleWin\Uninstall.exe"
  RMDir "$LOCALAPPDATA\Programs\IsleWin"
  Delete "$SMPROGRAMS\Isle.lnk"
  DeleteRegKey HKCU "${OLD_UNINST_KEY}"
FunctionEnd

Section "Install"
  ; A running copy would lock the exe (upgrade case).
  nsExec::Exec 'taskkill /F /IM buginotch.exe'
  nsExec::Exec 'taskkill /F /IM isle-win.exe'
  Sleep 300
  Call MigrateOldInstall
  SetOutPath "$INSTDIR"
  File "..\target\x86_64-pc-windows-gnu\release\buginotch.exe"
  File "..\LICENSE"
  File "..\NOTICE.md"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Buginotch.lnk" "$INSTDIR\buginotch.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "Buginotch"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "Buginotch"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

; Silent installs come from the in-app updater: start the new version when done.
Function .onInstSuccess
  IfSilent 0 notsilent
  Exec '"$INSTDIR\buginotch.exe"'
  notsilent:
FunctionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /F /IM buginotch.exe'
  Sleep 300
  Delete "$INSTDIR\buginotch.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\NOTICE.md"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Buginotch.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "Buginotch"
  DeleteRegKey HKCU "${UNINST_KEY}"
  ; Settings and data in %APPDATA%\IsleWin are the user's; they stay.
SectionEnd
