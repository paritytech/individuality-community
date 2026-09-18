# Coinage Instance Example

This guide shows how to create a Coinage instance backed by an asset.

There are two creation paths:

- **Sponsored:** anyone can create the instance. A shared pot must back each load.
- **Sufficient:** governance creates or approves the instance. Loading does not depend on a sponsor pot.

The examples follow the flow in `src/main.ts`.

> Calls named `submitReferendum` simulate approved governance on the local network. They do not submit a real OpenGov referendum.

## Step 1: Choose an asset

A Coinage instance is backed by an existing asset.

This example creates an asset on Asset Hub and registers it on People:

```ts
await submitReferendum("Referendum register example asset", [
  ...buildExampleReserveAssetRegistration(),
  await buildPeopleTransactCall(
    buildExampleForeignAssetRegistration(),
  ),
]);
```

The example asset is marked as a **sufficient asset**. This makes it easier to create asset accounts.

A sufficient asset is not the same as a sufficient Coinage instance:

- **Sufficient asset:** an Assets pallet setting.
- **Sufficient Coinage instance:** a Coinage mode approved by governance.

The asset unit sets the value of the smallest Coinage denomination:

```ts
export const COINAGE_ASSET_UNIT = 10_000n;
```

The example asset has six decimals, so `10_000` units equal `0.01` tokens.

## Step 2: Create a DEX pool

Create a pool between the underlying asset and the chain’s native token:

```ts
await createDexPool(
  NATIVE_ASSET_LOCATION,
  EXAMPLE_ASSET_LOCATION,
);
```

The instance can be created without this pool, but setting it up is strongly recommended.

Coinage uses the pool to convert unload fees from the underlying asset into the native token. This lets users pay the fee with the asset they are unloading.

Without the pool:

- Asset-based unload fees are not available.
- Users must use the native token for those fees.
- Users may need a separately funded account holding the native token.

That works technically, but it is not the intended user experience for private Coinage flows.

The local helper mints test liquidity directly on People. A production pool should use real, reserve-backed liquidity.

## Option A: Create a sponsored instance

Sponsored instance creation is permissionless. Any funded account can call:

```ts
peopleApi.tx.Coinage.create_sponsored_instance({
  asset_id: EXAMPLE_ASSET_LOCATION,
  asset_unit: COINAGE_ASSET_UNIT,
  initial_funding: undefined,
});
```

The creator provides:

- An instance creation deposit.
- Any asset amount missing from the Coinage pallet account’s minimum balance.
- Optional initial funding for the sponsor pot.

The instance creation deposit is held while the instance remains sponsored.

### The sponsor pot

Loading a coin stores a key on-chain. For a sponsored instance, Coinage holds one load deposit from the shared pot for each loaded key.

The user does not pay this deposit directly. The pot pays it.

If the pot does not have enough funds, new loads are rejected.

The deposit returns to the pot after the coin is unloaded or its old data is cleaned up.

### Initial pot funding

`initial_funding` can fund the pot during instance creation:

```ts
peopleApi.tx.Coinage.create_sponsored_instance({
  asset_id: EXAMPLE_ASSET_LOCATION,
  asset_unit: COINAGE_ASSET_UNIT,
  initial_funding: [
    NATIVE_ASSET_LOCATION,
    initialAmount,
  ],
});
```

The example uses `undefined`, so the instance starts without initial pot funding:

```ts
initial_funding: undefined
```

The instance is still created, but loading remains unavailable until the pot can cover a load deposit.

Anyone can fund the pot later:

```ts
peopleApi.tx.Coinage.fund_pot({
  instance_id: instanceId,
  currency: NATIVE_ASSET_LOCATION,
  amount,
});
```

Contributions are recorded for each funder. Funds that are not locked may be withdrawn again.

## Convert a sponsored instance to sufficient

Governance can convert a sponsored instance:

```ts
peopleApi.tx.Coinage.make_instance_sufficient({
  instance_id: instanceId,
});
```

After conversion:

- New loads no longer need collateral from the pot.
- Existing load deposits return to the pot.
- The creator’s instance creation deposit is released.
- Pot contributors can withdraw funds that are no longer locked.

The instance remains the same. Only its funding mode changes.

In the example, the governance call is sent through People:

```ts
await submitReferendum("Referendum bless instance", [
  await buildPeopleTransactCall([
    coinageMakeInstanceSufficientCall(instanceId).decodedCall,
  ]),
]);
```

## Option B: Create a sufficient instance directly

Governance can also create a sufficient instance directly.

A sufficient instance does not use a sponsor pot for loads. Coinage does not hold a per-load deposit.

Users may still need to pay normal transaction and unload fees.

### Prepare the Coinage pallet account

Before creating the instance, the Coinage pallet account must hold the asset’s minimum balance:

```ts
peopleApi.tx.Assets.mint({
  id: EXAMPLE_ASSET_LOCATION,
  beneficiary: Enum("Id", COINAGE_PALLET_ACCOUNT),
  amount: EXAMPLE_ASSET_MIN_BALANCE,
});
```

The example asset is sufficient, so minting its minimum balance also creates the asset account.

For a non-sufficient asset, the account must first be prepared with `Assets.touch`.

### Create the instance

Governance can then create the sufficient instance:

```ts
peopleApi.tx.Coinage.create_sufficient_instance({
  asset_id: EXAMPLE_ASSET_LOCATION,
  asset_unit: COINAGE_ASSET_UNIT,
});
```

This path has:

- No sponsored instance creation deposit.
- No sponsor pot.
- No per-load collateral requirement.

## Selecting the example path

The example chooses its path with:

```ts
const COINAGE_PERMISSIONLESS = true;
```

When set to `true`, it:

1. Creates a sponsored instance.
2. Converts it to a sufficient instance through governance.

When set to `false`, governance creates a sufficient instance directly.
