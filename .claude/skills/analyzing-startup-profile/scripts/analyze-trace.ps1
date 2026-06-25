<#
.SYNOPSIS
  just profile-startup が出す Chrome trace (tracing-chrome) を解析する。
  B/E イベントを tid ごとにスタックでペアリングし、span ごとの実時間・
  起動タイムライン・「どの span も走っていない空白(=待ち/未計装)」を出す。

  重要: span の instance 数や active 合計は async では wall-clock ではない
  (.instrument() は await ごとに re-enter する)。回数は LOG で数え、
  待ち時間は下の "no-span gaps" で見ること。

.EXAMPLE
  pwsh .claude/skills/analyzing-startup-profile/scripts/analyze-trace.ps1
  pwsh ...\analyze-trace.ps1 -Path target/profile/trace.json
#>
param([string]$Path = "target/profile/trace.json")

$j = Get-Content $Path -Raw | ConvertFrom-Json
$ev = $j | Where-Object { $_.ph -eq 'B' -or $_.ph -eq 'E' }
$stacks = @{}; $spans = New-Object System.Collections.Generic.List[object]
foreach ($e in $ev) {
    $tid = $e.tid
    if (-not $stacks.ContainsKey($tid)) { $stacks[$tid] = New-Object System.Collections.Stack }
    if ($e.ph -eq 'B') { $stacks[$tid].Push($e) }
    else { $b = $stacks[$tid].Pop(); $spans.Add([pscustomobject]@{ name = $b.name; start = $b.ts; end = $e.ts }) }
}

$tmin = ($spans | Measure-Object start -Minimum).Minimum
$tmax = ($spans | Measure-Object end   -Maximum).Maximum
"=== timeline ==="
"first span start : {0:N1} ms" -f ($tmin / 1000)
"last  span end   : {0:N1} ms  (= wall-clock の概算)" -f ($tmax / 1000)
""
"=== span ごと (instances は POLL 回数であり操作回数ではない) ==="
$spans | Group-Object name | ForEach-Object {
    $g = $_.Group
    [pscustomobject]@{
        name      = $_.Name
        instances = $_.Count
        window_ms = [math]::Round((($g | Measure-Object end -Maximum).Maximum - ($g | Measure-Object start -Minimum).Minimum) / 1000, 1)
        active_ms = [math]::Round((($g | ForEach-Object { $_.end - $_.start } | Measure-Object -Sum).Sum) / 1000, 2)
    }
} | Sort-Object window_ms -Descending | Format-Table -AutoSize

"=== no-span gaps (>15ms): 待ち時間。サーバRTT / 未計装 / backoff のいずれか ==="
$iv = $spans | Sort-Object start
$merged = New-Object System.Collections.Generic.List[object]
foreach ($s in $iv) {
    if ($merged.Count -eq 0) { $merged.Add([pscustomobject]@{ s = $s.start; e = $s.end }); continue }
    $last = $merged[$merged.Count - 1]
    if ($s.start -le $last.e) { if ($s.end -gt $last.e) { $last.e = $s.end } }
    else { $merged.Add([pscustomobject]@{ s = $s.start; e = $s.end }) }
}
for ($i = 0; $i -lt $merged.Count - 1; $i++) {
    $gap = $merged[$i + 1].s - $merged[$i].e
    if ($gap -gt 15000) { "  gap {0,7:N1}ms  (from {1,7:N1} to {2,7:N1}ms)" -f ($gap / 1000), ($merged[$i].e / 1000), ($merged[$i + 1].s / 1000) }
}
