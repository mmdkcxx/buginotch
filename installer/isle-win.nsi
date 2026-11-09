; SPDX-License-Identifier: GPL-3.0-or-later
; Per-user installer for Isle for Windows (no admin rights needed).
; Build: makensis -DVERSION=0.1.0 installer/isle-win.nsi

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

Unicode true
Name "Isle for Windows"
OutFile "..\dist\IsleWin-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\IsleWin"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\IsleWin"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
; The finish page's two checkboxes: start now, and start with Windows.
!define MUI_FINISHPAGE_RUN "$INSTDIR\isle-win.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Jalankan Isle sekarang"
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Mulai dengan Windows"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION SetAutostart
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Indonesian"

Function SetAutostart
  WriteRegStr HKCU "${RUN_KEY}" "IsleWin" '"$INSTDIR\isle-win.exe"'
FunctionEnd

Section "Install"
  ; A running copy would lock the exe (upgrade case).
  nsExec::Exec 'taskkill /F /IM isle-win.exe'
  Sleep 300
  SetOutPath "$INSTDIR"
  File "..\target\x86_64-pc-windows-gnu\release\isle-win.exe"
  File "..\LICENSE"
  File "..\NOTICE.md"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Isle.lnk" "$INSTDIR\isle-win.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "Isle for Windows"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "Isle for Windows"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /F /IM isle-win.exe'
  Sleep 300
  Delete "$INSTDIR\isle-win.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\NOTICE.md"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Isle.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "IsleWin"
  DeleteRegKey HKCU "${UNINST_KEY}"
  ; Settings and data in %APPDATA%\IsleWin are the user's; they stay.
SectionEnd
