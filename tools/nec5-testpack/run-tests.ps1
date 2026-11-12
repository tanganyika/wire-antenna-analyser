# NEC-5 test pack for Wire Antenna Analyser.
#
# Runs your own NEC-5 program on each deck in the "decks" folder, the way EZNEC Pro does:
#   "<your NEC-5 program>" "WTP01.NEC" "WTP01.OUT"
# from the program's own folder, then collects NEC-5's output and messages in "results" and
# zips them into nec5-results.zip. Your NEC-5 program itself is not copied anywhere.
#
# Start it with run-tests.bat (double-click it, or drag your NEC-5 program onto it).
# Works with Windows PowerShell 5.1 and PowerShell 7. ASCII only, on purpose.

param([string]$Nec = "", [double]$TimeoutMinutes = 10)

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$decks = Join-Path $here "decks"
$results = Join-Path $here "results"
$zip = Join-Path $here "nec5-results.zip"

Write-Host ""
Write-Host "NEC-5 test pack for Wire Antenna Analyser"
Write-Host "-----------------------------------------"

if (-not $Nec) {
    Write-Host "Drag your NEC-5 program onto this window (or type its full path), then press Enter:"
    $Nec = Read-Host ">"
}
$Nec = $Nec.Trim().Trim('"').Trim("'")
if (-not (Test-Path -LiteralPath $Nec -PathType Leaf)) {
    Write-Host "There is no program at: $Nec"
    exit 1
}
$necItem = Get-Item -LiteralPath $Nec
$necDir = $necItem.DirectoryName

if (Test-Path -LiteralPath $results) { Remove-Item -LiteralPath $results -Recurse -Force }
New-Item -ItemType Directory -Path $results | Out-Null
$log = Join-Path $results "run-log.txt"

function Log([string]$s) {
    Add-Content -LiteralPath $log -Value $s
}

$v = $necItem.VersionInfo
Log "NEC-5 test pack run, $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Log "program:        $($necItem.FullName)"
Log "size and date:  $($necItem.Length) bytes, $($necItem.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss'))"
Log "version info:   product '$($v.ProductName)' $($v.ProductVersion), file $($v.FileVersion), '$($v.FileDescription)'"
Log "Windows:        $([Environment]::OSVersion.VersionString), PowerShell $($PSVersionTable.PSVersion)"
Log "run from:       $necDir"
Log "time limit:     $TimeoutMinutes minutes per deck"
Log ""

# what is in the NEC-5 folder before the runs, so files NEC-5 adds can be found
$before = @{}
Get-ChildItem -LiteralPath $necDir -File | ForEach-Object { $before[$_.Name] = $true }
$before.Keys | Sort-Object | Set-Content -LiteralPath (Join-Path $results "nec5-folder-before.txt")

$summary = @()
foreach ($deck in Get-ChildItem -LiteralPath $decks -Filter *.nec | Sort-Object Name) {
    $base = $deck.BaseName
    $id = $base.Substring(0, [Math]::Min(2, $base.Length))
    $inName = "WTP$id.NEC"
    $outName = "WTP$id.OUT"
    $inPath = Join-Path $necDir $inName
    $outPath = Join-Path $necDir $outName
    Copy-Item -LiteralPath $deck.FullName -Destination $inPath -Force
    if (Test-Path -LiteralPath $outPath) { Remove-Item -LiteralPath $outPath -Force }

    # the folder just before this run: files are compared by name, size and time afterwards
    $snap = @{}
    Get-ChildItem -LiteralPath $necDir -File | ForEach-Object { $snap[$_.Name] = "$($_.Length) $($_.LastWriteTimeUtc.Ticks)" }

    Write-Host -NoNewline ("{0,-40}" -f $base)
    $stdout = Join-Path $results "$base.stdout.txt"
    $stderr = Join-Path $results "$base.stderr.txt"
    $start = Get-Date
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $necItem.FullName
    $psi.Arguments = "`"$inName`" `"$outName`""
    $psi.WorkingDirectory = $necDir
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $p = [System.Diagnostics.Process]::Start($psi)
    # nothing is typed in: if it asks for anything it gets end-of-input, not a hang
    try { $p.StandardInput.Close() } catch {}
    $outTask = $p.StandardOutput.ReadToEndAsync()
    $errTask = $p.StandardError.ReadToEndAsync()
    $finished = $p.WaitForExit([int]($TimeoutMinutes * 60 * 1000))
    $secs = [Math]::Round(((Get-Date) - $start).TotalSeconds, 1)
    if ($finished) {
        $p.WaitForExit()
        $how = "exit code $($p.ExitCode)"
    } else {
        try { $p.Kill() } catch {}
        $how = "stopped after $TimeoutMinutes minutes (no finish)"
    }
    $null = $outTask.Wait(5000)
    $null = $errTask.Wait(5000)
    $o = ""; $e = ""
    if ($outTask.IsCompleted) { $o = $outTask.Result }
    if ($errTask.IsCompleted) { $e = $errTask.Result }
    Set-Content -LiteralPath $stdout -Value $o
    Set-Content -LiteralPath $stderr -Value $e

    $got = Test-Path -LiteralPath $outPath
    if ($got) {
        Move-Item -LiteralPath $outPath -Destination (Join-Path $results "$base.out") -Force
    }
    Remove-Item -LiteralPath $inPath -Force -ErrorAction SilentlyContinue

    # anything else NEC-5 wrote or changed in its folder during this run
    $extra = @(Get-ChildItem -LiteralPath $necDir -File | Where-Object {
        $_.Name -notlike "WTP*" -and (-not $snap.ContainsKey($_.Name) -or $snap[$_.Name] -ne "$($_.Length) $($_.LastWriteTimeUtc.Ticks)")
    })
    if ($extra.Count -gt 0) {
        $ed = Join-Path $results "$base.other-files"
        New-Item -ItemType Directory -Path $ed -Force | Out-Null
        foreach ($f in $extra) { Copy-Item -LiteralPath $f.FullName -Destination $ed -Force }
    }

    $line = "{0}: {1}, {2} s, output file {3}{4}" -f $base, $how, $secs, $(if ($got) { "written" } else { "MISSING" }), $(if ($extra.Count -gt 0) { ", other files written: " + (($extra | ForEach-Object { $_.Name }) -join ", ") } else { "" })
    Log $line
    $summary += $line
    Write-Host $(if ($got) { "output written ($how)" } else { "NO OUTPUT ($how)" })
}

Get-ChildItem -LiteralPath $necDir -File | ForEach-Object { $_.Name } | Sort-Object | Set-Content -LiteralPath (Join-Path $results "nec5-folder-after.txt")
Copy-Item -LiteralPath $decks -Destination (Join-Path $results "decks") -Recurse

if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
try {
    Compress-Archive -Path (Join-Path $results "*") -DestinationPath $zip
    Write-Host ""
    Write-Host "Done. Please send back this one file:"
    Write-Host "  $zip"
} catch {
    Write-Host ""
    Write-Host "Done, but the zip couldn't be made ($($_.Exception.Message))."
    Write-Host "Please zip and send back this folder instead:"
    Write-Host "  $results"
}
Write-Host "It holds only NEC-5's output and messages for these test decks, not NEC-5 itself."
