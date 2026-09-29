<#
.SYNOPSIS
  Measures how much memory Limbo uses: the private working set of limbo.exe
  plus every msedgewebview2.exe that belongs to Limbo's WebView2 profile.

.DESCRIPTION
  "Private working set" is the memory only this app holds in RAM (what Task
  Manager's "Memory" column shows). Shared pages (DLLs mapped by many
  processes) are not counted, so the numbers add up across processes.

  Limbo's engine processes are found by their command line, which contains
  the user data folder (%LOCALAPPDATA%\Limbo\Profile). Other apps that use
  WebView2 (Teams, Widgets, Outlook...) are ignored.

  The in-app readout (memory pill) uses the same metric; the two should agree
  within 5% (plan section 12, phase 2 gate).

.PARAMETER Samples
  Number of samples to take (default 1).

.PARAMETER Interval
  Seconds between samples (default 2).

.PARAMETER Csv
  Append each sample to this CSV file (for docs/MEMORY_REPORT.md).

.PARAMETER Label
  Scenario name written to the CSV, e.g. "10 tabs, balanced".

.PARAMETER Detail
  Also list every process with its kind and memory.

.EXAMPLE
  tools\measure-memory.ps1 -Detail
.EXAMPLE
  tools\measure-memory.ps1 -Samples 30 -Interval 2 -Csv memory.csv -Label "cold start"
#>
[CmdletBinding()]
param(
  [int]$Samples = 1,
  [double]$Interval = 2,
  [string]$Csv,
  [string]$Label = '',
  [switch]$Detail
)

$ErrorActionPreference = 'Stop'
$profileDir = Join-Path $env:LOCALAPPDATA 'Limbo\Profile'

function Get-Kind([string]$commandLine) {
  if ($commandLine -notmatch '--type=') { return 'browser' }
  if ($commandLine -match '--type=renderer' -and $commandLine -match '--extension-process') { return 'extension' }
  if ($commandLine -match '--type=([a-z-]+)') {
    $t = $Matches[1]
    if ($t -eq 'utility' -and $commandLine -match '--utility-sub-type=([\w.]+)') { return "utility ($($Matches[1] -replace '^.*\.', ''))" }
    return $t
  }
  return 'other'
}

function Get-LimboProcesses {
  $procs = Get-CimInstance Win32_Process -Filter "Name = 'limbo.exe' OR Name = 'msedgewebview2.exe'" |
    Where-Object { $_.Name -eq 'limbo.exe' -or ($_.CommandLine -and $_.CommandLine.Contains($profileDir)) }
  if (-not $procs) { return @() }
  $ids = @($procs | ForEach-Object { $_.ProcessId })
  # Private working set per PID (bytes).
  $perf = @{}
  Get-CimInstance Win32_PerfRawData_PerfProc_Process |
    Where-Object { $ids -contains [int]$_.IDProcess } |
    ForEach-Object { $perf[[int]$_.IDProcess] = [int64]$_.WorkingSetPrivate }
  foreach ($p in $procs) {
    [pscustomobject]@{
      Pid   = $p.ProcessId
      Name  = $p.Name
      Kind  = if ($p.Name -eq 'limbo.exe') { 'host' } else { Get-Kind $p.CommandLine }
      Bytes = if ($perf.ContainsKey([int]$p.ProcessId)) { $perf[[int]$p.ProcessId] } else { 0 }
    }
  }
}

function Format-MB([int64]$bytes) { '{0,7:N1} MB' -f ($bytes / 1MB) }

for ($i = 1; $i -le $Samples; $i++) {
  $rows = @(Get-LimboProcesses)
  if ($rows.Count -eq 0) {
    Write-Warning 'Limbo is not running.'
    exit 1
  }
  $total = ($rows | Measure-Object Bytes -Sum).Sum
  $os = Get-CimInstance Win32_OperatingSystem
  $availMB = [math]::Round($os.FreePhysicalMemory / 1KB)
  $stamp = Get-Date -Format 's'

  if ($Detail) {
    $rows | Sort-Object Bytes -Descending |
      Format-Table @{ n = 'PID'; e = { $_.Pid } }, @{ n = 'Kind'; e = { $_.Kind } }, @{ n = 'Memory'; e = { Format-MB $_.Bytes }; a = 'right' } -AutoSize |
      Out-Host
  }
  Write-Host ("{0}  Limbo: {1} in {2} processes   (system free: {3} MB)" -f $stamp, (Format-MB $total).Trim(), $rows.Count, $availMB)

  if ($Csv) {
    $byKind = $rows | Group-Object Kind | ForEach-Object { '{0}={1:N0}' -f $_.Name, (($_.Group | Measure-Object Bytes -Sum).Sum / 1MB) }
    [pscustomobject]@{
      Time        = $stamp
      Label       = $Label
      TotalMB     = [math]::Round($total / 1MB, 1)
      Processes   = $rows.Count
      SystemFreeMB = $availMB
      ByKindMB    = ($byKind -join '; ')
    } | Export-Csv -Path $Csv -Append -NoTypeInformation
  }

  if ($i -lt $Samples) { Start-Sleep -Seconds $Interval }
}
