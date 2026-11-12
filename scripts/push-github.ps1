param(
    [Parameter(Mandatory = $true)][string]$Owner,
    [Parameter(Mandatory = $true)][string]$Repository,
    [switch]$Private,
    [string]$Branch = "main"
)

$ErrorActionPreference = "Stop"
$token = $env:GITHUB_TOKEN
if (-not $token) { $token = $env:GH_TOKEN }
if (-not $token) { throw "set GITHUB_TOKEN or GH_TOKEN with repo scope first" }

$git = "D:\dsh\.tools\mingit\cmd\git.exe"
if (-not (Test-Path $git)) { $git = "git" }
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$headers = @{
    Authorization          = "Bearer $token"
    Accept                 = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"           = "rustget-push"
}
$body = @{ name = $Repository; private = [bool]$Private } | ConvertTo-Json
try {
    $repo = Invoke-RestMethod -Method Post -Uri "https://api.github.com/user/repos" -Headers $headers -Body $body
    Write-Output "created: $($repo.html_url)"
}
catch {
    $status = $_.Exception.Response.StatusCode.value__
    if ($status -ne 422) { throw }
    Write-Output "repository already exists; continuing"
}

& $git remote remove origin 2>$null
& $git remote add origin "https://github.com/$Owner/$Repository.git"
$pushUrl = "https://x-access-token:$token@github.com/$Owner/$Repository.git"
& $git push --set-upstream $pushUrl $Branch
if ($LASTEXITCODE -ne 0) { throw "git push failed" }
& $git remote set-url origin "https://github.com/$Owner/$Repository.git"
Write-Output "pushed: https://github.com/$Owner/$Repository/tree/$Branch"
