<#
Deploy Stylus Contract Script (PowerShell)
Prerequisites:
  - Rust nightly + wasm32 target
  - stylus CLI (NOTE: Windows users may need WSL due to unix-specific code in current stylus release)
  - Environment variables: ARBITRUM_RPC, PRIVATE_KEY
Usage:
  pwsh ./scripts/deploy_stylus.ps1 -ContractCrate contracts/zero_day_futures [-UseWSL]
Outputs:
  Writes a JSON summary with contract address to stdout.
#>
param(
  [string]$ContractCrate = "contracts/zero_day_futures"
  [switch]$UseWSL
)

function Test-EnvVar {
  param([string]$Name)
  if (-not $env:$Name) { Write-Error "Environment variable $Name required"; exit 1 }
}

Test-EnvVar -Name "ARBITRUM_RPC"
Test-EnvVar -Name "PRIVATE_KEY"

Write-Host "[1/4] Adding wasm32 target if missing" -ForegroundColor Cyan
rustup target add wasm32-unknown-unknown | Out-Null

Write-Host "[2/4] Building Stylus contract" -ForegroundColor Cyan
if ($UseWSL) {
  wsl cargo stylus build -p (Split-Path $ContractCrate -Leaf) --release
} else {
  cargo stylus build -p (Split-Path $ContractCrate -Leaf) --release
}
if ($LASTEXITCODE -ne 0) { Write-Error "Stylus build failed"; exit 1 }

$wasmPath = Join-Path $ContractCrate "target/stylus/release/zero_day_futures.wasm"
if (-not (Test-Path $wasmPath)) { Write-Error "WASM not found at $wasmPath"; exit 1 }

Write-Host "[3/4] Deploying..." -ForegroundColor Cyan
if ($UseWSL) {
  wsl stylus deploy --wasm "$wasmPath" --private-key "$env:PRIVATE_KEY" --rpc "$env:ARBITRUM_RPC" | Tee-Object -Variable deployOut
} else {
  stylus deploy --wasm $wasmPath --private-key $env:PRIVATE_KEY --rpc $env:ARBITRUM_RPC | Tee-Object -Variable deployOut
}

if ($LASTEXITCODE -ne 0) { Write-Error "Deploy failed"; exit 1 }

$address = ($deployOut | Select-String -Pattern "Contract address:" | ForEach-Object { ($_ -split ":")[1].Trim() })
if (-not $address) { Write-Error "Could not parse contract address from deploy output"; exit 1 }
Write-Host "[4/4] Deployment complete. Address: $address" -ForegroundColor Green
$summary = @{ contract_address = $address; crate = $ContractCrate; rpc = $env:ARBITRUM_RPC }
Write-Output ($summary | ConvertTo-Json -Compress)
Write-Host "Set environment variable: `n  $env:CONTRACT_ADDRESS = $address" -ForegroundColor Yellow
