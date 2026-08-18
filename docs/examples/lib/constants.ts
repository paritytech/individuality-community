/** Shared constants for the People <-> Asset Hub bring-up and churn scripts. */
import { Enum, FixedSizeBinary } from "polkadot-api";

/**
 * The full-people collection identifier: `PEOPLE_IDENTIFIER` in the runtime
 * (`support/src/traits/reality.rs`), a 32-byte value — the text
 * `pop:polkadot.network/people` (27 chars) right-padded with 5 spaces to 32.
 *
 * Churn is driven on this collection (not people-lite): it is populated by
 * `People.force_recognize_personhood`, a sudo call that only needs valid member
 * keys, whereas lite membership requires VRF-proof `attest` calls. The notifier
 * treats every collection uniformly, so this fully exercises the OCW path.
 */
export const PEOPLE_IDENTIFIER = FixedSizeBinary.fromText("pop:polkadot.network/people     ");

/** Ring exponent for the people collection (`MembersFlexibleRingExponent = R2e9`). */
export const PEOPLE_RING_EXPONENT = Enum("R2e9");

/** Asset Hub's para id in this local network (= `ASSET_HUB_ID` / `NEXT_ASSET_HUB_ID`). */
export const ASSET_HUB_PARA_ID = 1500;

/**
 * Index of `MembersSubscriber` in the Asset Hub runtime's construct_runtime
 * (`MembersSubscriber: indiv_pallet_members_subscriber = 97`).
 */
export const SUBSCRIBER_PALLET_INDEX = 97;
