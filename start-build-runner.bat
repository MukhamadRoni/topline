@echo off
start "Topline build runner" /min powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0.tools\watch.ps1"
