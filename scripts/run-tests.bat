@echo off
REM Test runner: runs non-photon tests, counts OK/FAIL, collects unresolved calls
setlocal enabledelayedexpansion
cd /d D:\Code\AuraLang
set exe=rust\target\release\aura.exe
set passed=0
set failed=0
set totalUnresolved=0

for /r tests\basics %%f in (*.aura) do (
    if not "%%~nf"=="" goto :next
)
for /r tests %%f in (*.aura) do (
    REM Skip photon, complier, Debug/Test_ prefixed
    echo %%f | findstr /i "\\photon\\" >nul && goto :next
    echo %%f | findstr /i "\\complier\\" >nul && goto :next
    set fname=%%~nf
    echo !fname! | findstr /i "Debug Test_" >nul && goto :next

    set out=
    for /f "usebackq delims=" %%a in (`"`"%%exe%%"`" run "%%f" 2>&1`) do (
        set out=!out!%%a
    )
    if !errorlevel! equ 0 (
        set /a passed+=1
    ) else (
        set /a failed+=1
        echo FAIL: %%f
    )
    :next
)

echo === Results: %passed% OK / %failed% FAIL ===
endlocal
