@echo off
title PingDart AI Tester - Enterprise Autonomous Intelligence
cd /d "%~dp0"

if not "%~1"=="" (
    python ai_tester.py %*
    goto :done
)

:menu
cls
echo =====================================================================
echo           PINGDART AI TESTER - POWERED BY LOCAL OLLAMA
echo       Phases 14C-14F & 15: Enterprise Autonomous Intelligence
echo =====================================================================
echo.
echo   [1] 24/7 Continuous Autonomous Validation & Learning Daemon (Phase 14B)
echo   [2] Coverage Intelligence Engine & Blindspot Hunter (Phase 14C)
echo   [3] Long-Term Endurance Telemetry & Trend Audit (Phase 14D)
echo   [4] Query Plan Regression Detector (EXPLAIN CBO) (Phase 14E)
echo   [5] AI Root Cause Analyzer & Autonomous Repair Patches (Phase 14F)
echo   [6] Distributed Cluster Chaos & Raft Consensus Engine (Phase 15)
echo   [7] 12-Layer Enterprise Chaos & Autonomous Validation Battery
echo   [8] 1000-Op High-Scale AI Benchmark (DBs, Tables, CRUD, Search)
echo   [9] Continuous Infinite Stress Benchmark (Loops until Ctrl+C)
echo   [10] Interactive AI REPL (Chat with Tester in Terminal)
echo   [11] View Latest Daily Health Scorecard (daily_report.md)
echo   [12] View Discovered Attack & Failure Patterns (new_patterns.json)
echo   [13] View Self-Learning Bug Ledger & Regressions
echo   [14] Exit
echo.
echo =====================================================================
set /p choice="Select an option (1-14) [default: 1]: "

if "%choice%"=="" set choice=1
if "%choice%"=="1" goto :daemon
if "%choice%"=="2" goto :coverage
if "%choice%"=="3" goto :endurance
if "%choice%"=="4" goto :plan
if "%choice%"=="5" goto :rootcause
if "%choice%"=="6" goto :cluster
if "%choice%"=="7" goto :chaos
if "%choice%"=="8" goto :benchmark
if "%choice%"=="9" goto :continuous
if "%choice%"=="10" goto :repl
if "%choice%"=="11" goto :scorecard
if "%choice%"=="12" goto :patterns
if "%choice%"=="13" goto :bugs
if "%choice%"=="14" goto :exit
goto :menu

:daemon
cls
echo Starting 24/7 Continuous Autonomous Validation & Learning Daemon...
echo Press Ctrl+C at any time to pause and save session checkpoints.
echo.
python ai_tester.py --mode continuous-learning
goto :done

:coverage
cls
echo Starting Coverage Intelligence Engine & Blindspot Hunter...
python ai_tester.py --mode coverage
goto :done

:endurance
cls
echo Starting Long-Term Endurance Telemetry & Resource Trend Audit...
python ai_tester.py --mode endurance
goto :done

:plan
cls
echo Starting Query Plan Regression Detector (EXPLAIN CBO Analysis)...
python ai_tester.py --mode plan-regression
goto :done

:rootcause
cls
echo Starting AI Root Cause Analyzer & Autonomous Repair Patch Engine...
python ai_tester.py --mode root-cause
goto :done

:cluster
cls
echo Starting Distributed Cluster Chaos & Raft Consensus Engine...
python ai_tester.py --mode cluster-chaos
goto :done

:chaos
cls
echo Starting 12-Layer Autonomous Validation Battery...
python ai_tester.py --mode chaos
goto :done

:benchmark
cls
echo Starting 1000-operation AI Benchmark...
python ai_tester.py --mode benchmark --count 1000
goto :done

:continuous
cls
echo Starting Continuous Infinite Benchmark (Press Ctrl+C to Stop)...
python ai_tester.py --mode benchmark --continuous
goto :done

:repl
cls
echo Starting Interactive AI REPL...
python ai_tester.py --mode interactive
goto :done

:scorecard
cls
echo Opening Latest Daily Health Scorecard...
python ai_tester.py --mode scorecard
goto :done

:patterns
cls
echo Viewing Discovered Attack & Failure Patterns...
python ai_tester.py --mode patterns
goto :done

:bugs
cls
echo Viewing Self-Learning Bug Ledger...
python ai_tester.py --mode bugs
goto :done

:done
echo.
echo =====================================================================
echo Run completed.
echo =====================================================================
pause
goto :menu

:exit
exit /b 0
