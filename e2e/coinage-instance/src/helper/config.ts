import { AccountId, Enum } from "polkadot-api";

export const RPC_RELAY = process.env.RPC_RELAY ?? "ws://localhost:10000";
export const RPC_PEOPLE = process.env.RPC_PEOPLE ?? "ws://localhost:10010";
export const RPC_ASSET_HUB = process.env.RPC_ASSET_HUB ?? "ws://localhost:10020";

export const PEOPLE_PARA_ID = 1_502;
export const ASSET_HUB_PARA_ID = 1_500;

export const EXAMPLE_ASSET_ID = 50_000_414;
// Eve owns the example asset locally and stands in for its production issuer.
export const EXAMPLE_ASSET_OWNER_LOCAL = "5HGjWAeFDfFCWPsjFQdVV2Msvz2XtMktvgocEZcCj68kUMaw";
export const EXAMPLE_ASSET_MIN_BALANCE = 1n;
export const EXAMPLE_ASSET_NAME = "Coinage Example";
export const EXAMPLE_ASSET_SYMBOL = "EXAMPLE";
export const EXAMPLE_ASSET_DECIMALS = 6;
export const COINAGE_ASSET_UNIT = 10_000n;

/** The location People uses to identify the Asset Hub reserve asset. */
export const EXAMPLE_ASSET_LOCATION = {
  parents: 1,
  interior: Enum("X3", [
    Enum("Parachain", ASSET_HUB_PARA_ID),
    Enum("PalletInstance", 50),
    Enum("GeneralIndex", BigInt(EXAMPLE_ASSET_ID)),
  ] as const),
};

const coinageAccount = new Uint8Array(32);
coinageAccount.set(new TextEncoder().encode("modlcoinage "));
export const COINAGE_PALLET_ACCOUNT = AccountId(42).dec(coinageAccount);

/** Relay-chain native asset location on People. */
export const NATIVE_ASSET_LOCATION = { parents: 1, interior: Enum("Here") };

const NATIVE_ASSET_UNIT = 10_000_000_000n;
const EXAMPLE_ASSET_UNIT = 10n ** BigInt(EXAMPLE_ASSET_DECIMALS);
const POOL_TOKEN_AMOUNT = 10_000n;

/** Equal token reserves give the pool a one-to-one initial price. */
export const POOL_NATIVE_ASSET_LIQUIDITY = POOL_TOKEN_AMOUNT * NATIVE_ASSET_UNIT;
export const POOL_EXAMPLE_ASSET_LIQUIDITY = POOL_TOKEN_AMOUNT * EXAMPLE_ASSET_UNIT;
