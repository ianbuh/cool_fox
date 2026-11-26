@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "profile=%~1"
if "%profile%"=="" set "profile=pareto-residency"
if /i not "%profile%"=="pareto-residency" if /i not "%profile%"=="rtx-3060ti" if /i not "%profile%"=="8gb" (
  echo Usage: Play-VIctory.cmd [pareto-residency ^| rtx-3060ti ^| 8gb] 1>&2
  exit /b 2
)
if not defined VICTORY_GAME_DIR set "VICTORY_GAME_DIR=%~dp0..\GTA6.Xbox.v1.0.2.1"
if not exist "%VICTORY_GAME_DIR%\game_scarlett_master.exe" (
  echo Set VICTORY_GAME_DIR to your own dump folder containing game_scarlett_master.exe. 1>&2
  exit /b 2
)
set "runner=%~dp0victory-3d-rtx-pareto-residency.exe"
if not exist "%runner%" set "runner=%~dp0target\release\victory.exe"
if not exist "%runner%" (
  echo Extract the Windows release or run cargo build --release --locked -p victory. 1>&2
  exit /b 2
)
if /i "%profile%"=="rtx-3060ti" goto :profile_8gb
if /i "%profile%"=="8gb" goto :profile_8gb
goto :profile_common

:profile_8gb
rem Tuned for 8 GB VRAM GPUs (e.g. RTX 3060 Ti) + 8C/16T CPUs (e.g. Ryzen 7 5700X).
if not defined VICTORY_THIN_THREADS set "VICTORY_THIN_THREADS=5"
if not defined VICTORY_JOIN_PARTS set "VICTORY_JOIN_PARTS=4"
if not defined VICTORY_PIPELINE_WORKERS set "VICTORY_PIPELINE_WORKERS=4"
if not defined VICTORY_RIPE_BLOCKS set "VICTORY_RIPE_BLOCKS=2"
if not defined VICTORY_LEVELS_MIN_KB set "VICTORY_LEVELS_MIN_KB=256"
if not defined VICTORY_CARD_ROOM_FOR_TARGETS_MB set "VICTORY_CARD_ROOM_FOR_TARGETS_MB=1536"
if not defined VICTORY_CARD_BUFFERS_MB set "VICTORY_CARD_BUFFERS_MB=384"
if not defined VICTORY_TEXTURE_STAGE_MB set "VICTORY_TEXTURE_STAGE_MB=96"
if not defined VICTORY_TEXTURE_STAGE_MS set "VICTORY_TEXTURE_STAGE_MS=120"
if not defined VICTORY_D3D12_BACKGROUND set "VICTORY_D3D12_BACKGROUND=0"

:profile_common
if not defined VICTORY_AMBIENT_AIR set "VICTORY_AMBIENT_AIR=1.5"
if not defined VICTORY_CACHE_DIR set "VICTORY_CACHE_DIR=%LOCALAPPDATA%\VIctory\cache-pareto"
if not defined VICTORY_TEXTURE_NO_WAIT set "VICTORY_TEXTURE_NO_WAIT=1"
if not defined VICTORY_RELAY set "VICTORY_RELAY=2"
if not defined VICTORY_WIDE_BACK set "VICTORY_WIDE_BACK=1"
if not defined VICTORY_CARD_REHASH_RUNS set "VICTORY_CARD_REHASH_RUNS=1"
if not defined VICTORY_JOIN_COPIES set "VICTORY_JOIN_COPIES=1"
if not defined VICTORY_INDIRECT_WINDOW set "VICTORY_INDIRECT_WINDOW=1"
if not defined VICTORY_RELAY_COPY set "VICTORY_RELAY_COPY=1"
if not defined VICTORY_CARD_STEPS set "VICTORY_CARD_STEPS=2"
if not defined VICTORY_INDIRECT_BATCH set "VICTORY_INDIRECT_BATCH=2"
if not defined VICTORY_MAP_THREADS set "VICTORY_MAP_THREADS=1"
if not defined VICTORY_CARD_AWAKE set "VICTORY_CARD_AWAKE=1"
if not defined VICTORY_SHOW_AFTER set "VICTORY_SHOW_AFTER=1"
if not defined VICTORY_AMBIENT set "VICTORY_AMBIENT=0.8"
if not defined VICTORY_THIN set "VICTORY_THIN=1"
if not defined VICTORY_JOIN_PARTS set "VICTORY_JOIN_PARTS=8"
if not defined VICTORY_UPLOAD_AHEAD set "VICTORY_UPLOAD_AHEAD=1"
if not defined VICTORY_LEVELS_ON_DEMAND set "VICTORY_LEVELS_ON_DEMAND=1"
if not defined VICTORY_CARD_BUFFERS_MIN_KB set "VICTORY_CARD_BUFFERS_MIN_KB=64"
if not defined VICTORY_LOG if not defined RUST_LOG set "VICTORY_LOG=warn"
"%runner%" run "%VICTORY_GAME_DIR%\game_scarlett_master.exe" --console series-s --execute
exit /b %errorlevel%
