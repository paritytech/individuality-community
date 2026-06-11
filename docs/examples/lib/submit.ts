/**
 * Shared helpers for submitting transactions and reporting their outcome.
 *
 * Both build a small `(tx-or-call, label)` submitter bound to a signer, throw a
 * labelled error on failure.
 *
 */
import type { PolkadotSigner } from "polkadot-api";

// JSON.stringify throws on bigints, which chain values can carry.
const toJson = (v: unknown) => JSON.stringify(v, (_, x) => (typeof x === "bigint" ? x.toString() : x));

interface SubmitResult {
  ok: boolean;
  dispatchError?: unknown;
  block: { hash: string };
}

// Anything with a `signAndSubmit` — i.e. a built transaction from any pallet.
interface SignedTx {
  signAndSubmit: (signer: PolkadotSigner, options?: any) => Promise<SubmitResult>;
}

// The minimal slice of a generated TypedApi we need to dispatch a Sudo.sudo
// call and read its inner result back off Sudo.Sudid. Both the People and Asset
// Hub APIs structurally satisfy this, so the helper stays chain-agnostic while
// the generic below keeps the `call` argument precisely typed per chain.
interface SudoApi {
  tx: {
    Sudo: {
      sudo: (arg: { call: any }) => {
        signAndSubmit: (signer: PolkadotSigner, options?: any) => Promise<SudoTxResult>;
      };
    };
  };
  event: {
    Sudo: {
      Sudid: {
        filter: (events: any) => Array<{ sudo_result: { success: boolean; value: unknown } }>;
      };
    };
  };
}
interface SudoTxResult extends SubmitResult {
  events: any;
}

/**
 * Build a `submit(tx, label)` for plain signed transactions, bound to `signer`.
 * Throws on dispatch failure; logs `  <label>: ok` and returns the tx result.
 *
 * `signOptions` carries chain-specific signing extras (e.g. the People chain's
 * `customSignedExtensions`); omit it for chains that need none.
 */
export function signedSubmitter(signer: PolkadotSigner, signOptions?: unknown) {
  return async <T extends SignedTx>(tx: T, label: string) => {
    const r = await tx.signAndSubmit(signer, signOptions);
    if (!r.ok) throw new Error(`${label} failed: ${toJson(r.dispatchError)}`);
    console.log(`  ${label}: ok`);
    return r;
  };
}

/**
 * Build a `sudo(call, label)` submitter bound to `api`/`signer`. It submits the
 * call via `Sudo.sudo`, throws on either an outer-tx or inner-call failure (see
 * the file header), logs `  <label>: ok` on success, and returns the tx result.
 *
 * `signOptions` carries chain-specific signing extras (e.g. the People chain's
 * `customSignedExtensions`); omit it for chains that need none.
 */
export function sudoSubmitter<A extends SudoApi>(api: A, signer: PolkadotSigner, signOptions?: unknown) {
  type Call = Parameters<A["tx"]["Sudo"]["sudo"]>[0]["call"];
  return async (call: Call, label: string) => {
    const r = await api.tx.Sudo.sudo({ call }).signAndSubmit(signer, signOptions);
    if (!r.ok) throw new Error(`${label}: outer sudo tx failed: ${toJson(r.dispatchError)}`);
    const [sudid] = api.event.Sudo.Sudid.filter(r.events);
    if (sudid && !sudid.sudo_result.success) {
      throw new Error(`${label}: inner call failed: ${toJson(sudid.sudo_result.value)}`);
    }
    console.log(`  ${label}: ok`);
    return r;
  };
}
