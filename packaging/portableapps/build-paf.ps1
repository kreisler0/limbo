<#
.SYNOPSIS
  Turns the assembled LimboPortable folder into LimboPortable_x.y.z.paf.exe
  with the official PortableApps.com Installer.

.DESCRIPTION
  Downloads the PortableApps.com Installer from portableapps.com (following
  its download pages and redirects), unpacks it with 7-Zip without running
  its installer, and runs it on the package folder. The .paf.exe is written
  next to the package folder. Every download attempt is logged, so a changed
  download page is easy to diagnose.

.PARAMETER Package
  The LimboPortable folder (App\, help.html, LimboPortable.exe...).

.EXAMPLE
  packaging\portableapps\build-paf.ps1 -Package dist-portable\LimboPortable
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Package,
  [string]$Tools = (Join-Path ([IO.Path]::GetTempPath()) 'pa-tools'),
  [int]$TimeoutSeconds = 240
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'  # Invoke-WebRequest is very slow with a progress bar
$UserAgent = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36'
$AppPage = 'https://portableapps.com/apps/development/portableapps.com_installer'

function Test-Exe([string]$Path) {
  if (-not (Test-Path $Path)) { return $false }
  if ((Get-Item $Path).Length -lt 500KB) { return $false }
  $head = [System.IO.File]::ReadAllBytes($Path)[0..1]
  return $head[0] -eq 0x4D -and $head[1] -eq 0x5A  # "MZ"
}

function Test-AllowedHost([Uri]$Uri) {
  $h = $Uri.Host.ToLowerInvariant()
  return $h -eq 'portableapps.com' -or $h.EndsWith('.portableapps.com') -or $h -eq 'sourceforge.net' -or $h.EndsWith('.sourceforge.net')
}

# Links in a page that lead toward the file: hrefs, meta refresh targets and
# quoted URLs in scripts that mention the file name or a download/redirect page.
function Get-DownloadLinks([string]$Html, [Uri]$From, [string]$File) {
  $pattern = '(?:href|src)\s*=\s*["'']([^"'']+)["'']|url\s*=\s*["'']?([^"''>\s;]+)|["''](https?://[^"''\s]+)["'']'
  $links = foreach ($m in [regex]::Matches($Html, $pattern, 'IgnoreCase')) {
    $raw = $null
    foreach ($i in 1..3) { if ($m.Groups[$i].Success) { $raw = $m.Groups[$i].Value; break } }
    $raw = [System.Net.WebUtility]::HtmlDecode($raw)
    if ($raw -notmatch [regex]::Escape($File) -and $raw -notmatch '(downloading|redirect|/download\b)') { continue }
    try { $u = [Uri]::new($From, $raw) } catch { continue }
    if (Test-AllowedHost $u) { $u.AbsoluteUri }
  }
  # Links naming the file itself first.
  $links | Select-Object -Unique | Sort-Object { if ($_ -match [regex]::Escape($File)) { 0 } else { 1 } }
}

function Get-Installer([string]$Dest) {
  $page = (Invoke-WebRequest $AppPage -UseBasicParsing -UserAgent $UserAgent).Content
  $file = [regex]::Match($page, 'PortableApps\.comInstaller_[0-9][0-9.]*(_[A-Za-z]+)?\.paf\.exe').Value
  if (-not $file) { throw "The PortableApps.com Installer file name wasn't found on $AppPage" }
  Write-Host "PortableApps.com Installer: $file"
  $target = Join-Path $Dest $file
  if (Test-Exe $target) { return $target }

  $enc = [Uri]::EscapeDataString($file)
  $queue = [System.Collections.Generic.Queue[string]]::new()
  foreach ($u in @(Get-DownloadLinks $page ([Uri]$AppPage) $file) + @(
      "https://portableapps.com/downloading/?a=PortableApps.comInstaller&s=s&d=pa&n=PortableApps.com%20Installer&f=$enc",
      "https://portableapps.com/redirect/?a=PortableApps.comInstaller&s=s&d=pa&f=$enc",
      "https://portableapps.com/redirect/?a=PortableApps.comInstaller&s=s&d=sfpa&f=$enc",
      "https://sourceforge.net/projects/portableapps/files/PortableApps.com%20Installer/$enc/download"
    )) { $queue.Enqueue($u) }

  $seen = @{}
  $tmp = Join-Path $Dest 'download.tmp'
  while ($queue.Count -gt 0 -and $seen.Count -lt 30) {
    $url = $queue.Dequeue()
    if ($seen.ContainsKey($url)) { continue }
    $seen[$url] = $true
    try {
      $r = Invoke-WebRequest $url -OutFile $tmp -PassThru -UseBasicParsing -UserAgent $UserAgent -MaximumRedirection 10
      $type = $r.Headers['Content-Type'] -join ','
      $size = (Get-Item $tmp).Length
      Write-Host ("GET {0} -> {1} {2} {3:N0} bytes" -f $url, $r.StatusCode, $type, $size)
      if (Test-Exe $tmp) {
        Move-Item -Force $tmp $target
        Write-Host "Downloaded $file from $url"
        return $target
      }
      if ($size -lt 2MB) {
        $html = Get-Content $tmp -Raw
        $final = if ($r.BaseResponse.RequestMessage.RequestUri) { $r.BaseResponse.RequestMessage.RequestUri } else { [Uri]$url }
        foreach ($next in Get-DownloadLinks $html $final $file) { if (-not $seen.ContainsKey($next)) { $queue.Enqueue($next) } }
      }
    } catch {
      Write-Host "GET $url -> failed: $($_.Exception.Message)"
    }
  }
  throw "Couldn't download $file (tried $($seen.Count) URLs, listed above)"
}

$Package = (Resolve-Path $Package).Path
New-Item -ItemType Directory -Force $Tools | Out-Null
$installer = Get-Installer $Tools

# A .paf.exe is an NSIS installer: 7-Zip unpacks it without running it.
$unpacked = Join-Path $Tools 'Installer'
& 7z x $installer "-o$unpacked" -y | Out-Null
if ($LASTEXITCODE -ne 0) { throw "7-Zip couldn't unpack $installer" }
$exe = Get-ChildItem -Recurse $unpacked -Filter 'PortableApps.comInstaller.exe' | Select-Object -First 1
if (-not $exe) { throw 'PortableApps.comInstaller.exe not found after unpacking' }

$outDir = Split-Path $Package -Parent
$before = @(Get-ChildItem $outDir -Filter '*.paf.exe' | ForEach-Object FullName)
Write-Host "Running $($exe.FullName) on $Package"
$p = Start-Process $exe.FullName -ArgumentList "`"$Package`"" -PassThru
if (-not $p.WaitForExit($TimeoutSeconds * 1000)) {
  $p.Kill()
  Write-Host "The PortableApps.com Installer didn't exit within $TimeoutSeconds s (a dialog may be waiting)."
}
$paf = Get-ChildItem $outDir -Filter '*.paf.exe' | Where-Object { $before -notcontains $_.FullName } | Select-Object -First 1
if (-not $paf) {
  Get-ChildItem -Recurse $Package, $unpacked -Include '*.log', '*Log*.txt' -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "--- $($_.FullName)"; Get-Content $_.FullName | Write-Host }
  throw 'The PortableApps.com Installer produced no .paf.exe'
}
Write-Host ("Built {0}: {1:N2} MB" -f $paf.Name, ($paf.Length / 1MB))
$paf.FullName
