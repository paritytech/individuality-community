# Initial Setup Scripts

Bash scripts that initialize blockchain state across the Paseo stack (People, AssetHub, Bulletin, Relay). Each script is **self-contained** (works standalone) and **guarded against re-runs** (it skips extrinsics that already took effect, since resubmitting them would fail). They follow a defined execution order, with some steps depending on earlier ones (noted in their headers).

They only configure chains that are **already running**, so make sure the configured endpoints are reachable.
For a reproducible local Relay + People + Asset Hub network, follow the
[Zombienet harness runbook](../../e2e/zombienet/README.md).

## Quick Start

```bash
# Prerequisites (first time only)
ENV=local   ./00-requirements.sh

# Run all scripts for an environment
ENV=local   ./start.sh

# Run a single script
ENV=local   ./08a-setup-people-collection.sh
```

## Environments

| Env | Relay | People | Target |
|-----|-------|--------|--------|
| `local` | localhost:10000 | localhost:10010 | Local development |

Set `ENV` before running any script. Defaults to `local` if unset.

## Scripts

Scripts are numbered by execution order. Related scripts share a number prefix with letter suffixes.

| Script | What it does |
|--------|-------------|
| **00** | Installs `dot` CLI and `jq`, imports signer accounts |
| **01a** | Funds attestation, attestation proxy, faucet, and asset owner accounts on People; bootstraps the People sovereign on the Relay |
| **01b-c** | HRMP channel setup (People<->AssetHub, People<->Bulletin) |
| **01d** | Funds sudo, attestation, attestation proxy, faucet, and asset owner accounts on AssetHub by teleporting native PAS from People |
| **02** | Adds sudo proxy delegation on People |
| **03a-g** | XTRNL: create, AssetHub pool, AssetHub metadata, People metadata, conversion rate, Coinage instance, People pool |
| **04a-b** | USDT/USDC: create on AssetHub with metadata, pool, and liquidity; create as foreign assets on People with metadata |
| **05a-c** | Stablecoin faucet: acquire USDT/USDC for the owner on AssetHub, fund the People faucet with USDT/USDC via XCM, fund the AssetHub faucet with USDT/USDC by swapping PAS |
| **06a-c** | PGAS: create, pool, alias fee |
| **07** | Adds ZK chunks on People |
| **08a-b** | Creates the people and people-lite collections on People |
| **09** | Overrides onboarding size for the people and people-lite collections on People |
| **10** | Subscribes AssetHub to ring root updates from People for the people and people-lite collections |
| **11** | Adds Proof of Ink design families from snapshot |
| **12a-c** | Attestation account: invites, allowances, proxy |
| **13** | Sets the DotNS gateway dispatcher address on AssetHub |

## File Structure

```
config-base.env                  # Shared config (token params, constants)
config-local.env                 # Per-environment overrides (RPCs, parachain IDs, signers, accounts, DotNS gateway dispatcher address)
load-config.sh                   # Sources config for ENV, registers chain aliases
utils.sh                         # Shared helpers (has_funds, has_hrmp_channel, etc.)
start.sh                         # Orchestrator: runs all scripts sequentially
xcm-to-parent.yaml               # XCM template for child->relay calls
poi-design-families.json         # Proof of Ink design family data
```

## How It Works

1. Every script sources `load-config.sh`, which:
   - Validates `ENV` (must be `local`)
   - Loads `config-base.env` + `config-${ENV}.env`
   - Validates all required per-env variables are set
   - Registers chain aliases (`relay`, `people`, `asset-hub`, `bulletin`)
   - Computes derived values (parachain accounts)

2. Each script checks on-chain state before submitting and skips extrinsics whose effects are already visible, because resubmitting them (`force_create` on an existing asset, a duplicate `add_proxy`, ...) would fail. This makes re-runs safe in the common case, but it is not idempotency: in-flight XCM can race the checks, and some checks only detect "already ran", not "desired state".

3. `start.sh` runs scripts in isolated subprocesses (`env -i`) to prevent variable leakage between scripts.

## Required Environment Variables

| Variable | Purpose |
|----------|---------|
| `PEOPLE_SUDO_MNEMONIC` | People sudo signer mnemonic |
| `AH_SUDO_MNEMONIC` | AssetHub sudo signer mnemonic |
| `ATTESTATION_MNEMONIC` | Attestation signer mnemonic |
| `ASSET_OWNER_MNEMONIC` | Asset owner signer mnemonic |
| `ENV` | Target environment (defaults to `local`) |

On `local`, the People sudo, AssetHub sudo, attestation, and asset owner signers are all built-in dev accounts, so no mnemonics are required.

## Adding a New Script

1. Create `NN-your-script.sh` (append at the end, or use a letter suffix to group with related scripts)
2. Start with:
   ```bash
   #!/usr/bin/env bash
   # Brief description of what this script does.
   set -euo pipefail
   source ./load-config.sh
   source ./utils.sh
   ```
3. Add a re-run guard: check on-chain state before acting, `exit 0` if the extrinsic already took effect
4. Add the filename to the `SCRIPT_NAMES` array in `start.sh`
5. If the script should only run for a certain env, gate inline at the top:
   ```bash
   if [[ ! "$ENV" =~ ^local$ ]]; then
     echo "Skipping - only relevant for the local environment (current: $ENV)"
     exit 0
   fi
   ```

## Adding a New Environment

1. Create `config-<name>.env` with the required per-env variables (`load-config.sh` lists the missing ones)
2. Add `<name>` to the `ENV` validation in `load-config.sh`
