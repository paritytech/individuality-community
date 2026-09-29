import type { Transaction, PolkadotClient } from "polkadot-api";

// This standalone example uses live metadata instead of generated descriptors.
// Keep the small read surface explicit; PAPI encodes every call against metadata.
type ReadOptions = { at: string };
type Storage<K extends unknown[], V> = {
  getValue(...args: [...K, ReadOptions?]): Promise<V>;
};
export type Coin = { instance_id: number; value: number; age: number };
type Location = {
  parents: number;
  interior: { type: string; value?: unknown };
};
type Instance = {
  asset_id: Location;
  asset_unit: bigint;
  mode: { type: string };
};
type SystemAccount = { nonce: number; data: { free: bigint } };
type Member = { type: string; value: { ring_index: number } };
type Root = { revision: number; root: string };
type DynamicTransaction = Transaction<any, Record<string, { value: unknown }>>;
export interface DemoApi {
  query: {
    Coinage: {
      Instances: Storage<[number], Instance | undefined>;
      CoinsByOwner: Storage<[string], Coin | undefined>;
      PaidTokenCollectionsCreated: {
        getEntries(): Promise<Array<{ keyArgs: [string] }>>;
      };
    };
    System: { Account: Storage<[string], SystemAccount> };
    Assets: {
      Account: Storage<[Location, string], { balance: bigint } | undefined>;
    };
    Members: {
      Members: Storage<[string, string], Member | undefined>;
      Root: Storage<[string, number], Root | undefined>;
      RingKeysStatus: Storage<[string, number], { included: number }>;
      RingKeys: Storage<[string, number, number], string[]>;
    };
  };
  constants: {
    Coinage: {
      MinimumExponent(): Promise<number>;
      MaximumExponent(): Promise<number>;
      RecyclerRingExponent(): Promise<{ type: string }>;
      PaidUnloadTokenRingExponent(): Promise<{ type: string }>;
    };
  };
  tx: Record<
    string,
    Record<string, (args: Record<string, unknown>) => DynamicTransaction>
  >;
}
export function demoApi(client: PolkadotClient): DemoApi {
  return client.getUnsafeApi() as unknown as DemoApi;
}
