# REQ-125 · 本机调试用的**同源反向代理**（不是产品的一部分，也不进构建）
#
# 为什么需要它：工作台页面由 `console serve`（:8899）提供，而查询台 / 目录浏览要调的
# `/api/sparql` `/api/catalog` `/api/describe` 属于 REQ-123 的 `query serve`（:8898）。
# 浏览器里这是**两个源**：页面只发同源请求（CSP `connect-src 'self'`），直连会被同源策略拦住。
# 生产上这件事由反向代理按路径分流解决；本脚本是同一口径的**本机最小版**。
#
# 用法（两个服务先各自起好）：
#   pwsh -File tests/fixtures/req125/dev_proxy.ps1
#   → 浏览器打开 http://127.0.0.1:8897/  （页面、/api/projects 等 → :8899）
#                                        （/api/sparql 等      → :8898）
#
# 参数：
#   -Listen  http://127.0.0.1:8897/
#   -Console http://127.0.0.1:8899
#   -Query   http://127.0.0.1:8898
#
# ⚠️ 只绑回环、只用于本机调试；不做 TLS、不做鉴权改写（Authorization 原样转发）。

param(
  [string]$Listen = "http://127.0.0.1:8897/",
  [string]$Console = "http://127.0.0.1:8899",
  [string]$Query = "http://127.0.0.1:8898"
)

$ErrorActionPreference = "Stop"

# 这三个前缀属于查询服务（REQ-123）；其余一律给控制面。
$queryPrefixes = @("/api/sparql", "/api/catalog", "/api/describe", "/mcp")

$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add($Listen)
$listener.Start()
Write-Host "· REQ-125 调试代理：$Listen"
Write-Host "  → 查询服务 $Query（$($queryPrefixes -join ' ')）"
Write-Host "  → 控制面   $Console（其余，含 / 页面）"
Write-Host "  浏览器打开 $Listen ；Ctrl+C 结束。"

function TargetBase([string]$path) {
  foreach ($p in $queryPrefixes) {
    if ($path -eq $p -or $path.StartsWith($p + "?") -or $path.StartsWith($p + "/")) { return $Query }
  }
  return $Console
}

try {
  while ($listener.IsListening) {
    $ctx = $listener.GetContext()
    $req = $ctx.Request
    $res = $ctx.Response
    $path = $req.Url.PathAndQuery
    $base = TargetBase $req.Url.AbsolutePath
    $target = $base + $path

    $headers = @{}
    foreach ($name in $req.Headers.AllKeys) {
      # Host / Connection / Content-Length / Transfer-Encoding 由 HttpClient 自己算。
      if ($name -in @("Host", "Connection", "Content-Length", "Transfer-Encoding", "Expect")) { continue }
      $headers[$name] = $req.Headers[$name]
    }

    $bodyBytes = $null
    if ($req.HasEntityBody) {
      $ms = [System.IO.MemoryStream]::new()
      $req.InputStream.CopyTo($ms)
      $bodyBytes = $ms.ToArray()
    }

    try {
      $resp = Invoke-WebRequest -Uri $target -Method $req.HttpMethod -Headers $headers `
        -Body $bodyBytes -SkipHttpErrorCheck -MaximumRedirection 0 -TimeoutSec 30
      $res.StatusCode = [int]$resp.StatusCode
      if ($resp.Headers["Content-Type"]) {
        $res.ContentType = [string]($resp.Headers["Content-Type"] -join "; ")
      }
      foreach ($h in @("Content-Security-Policy", "X-Content-Type-Options", "Referrer-Policy")) {
        if ($resp.Headers[$h]) { $res.Headers[$h] = [string]($resp.Headers[$h] -join ", ") }
      }
      $bytes = if ($resp.RawContentStream) { $resp.RawContentStream.ToArray() } else { [byte[]]::new(0) }
      $res.ContentLength64 = $bytes.Length
      if ($bytes.Length -gt 0) { $res.OutputStream.Write($bytes, 0, $bytes.Length) }
    }
    catch {
      $msg = "代理失败：$($_.Exception.Message)"
      $bytes = [System.Text.Encoding]::UTF8.GetBytes($msg)
      $res.StatusCode = 502
      $res.ContentType = "text/plain; charset=utf-8"
      $res.ContentLength64 = $bytes.Length
      $res.OutputStream.Write($bytes, 0, $bytes.Length)
    }
    finally {
      $res.OutputStream.Close()
    }
  }
}
finally {
  $listener.Stop()
  $listener.Close()
}
