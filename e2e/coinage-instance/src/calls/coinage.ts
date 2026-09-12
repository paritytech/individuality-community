import { Enum } from "polkadot-api";

import { peopleApi } from "../helper/api.ts";
import {
  COINAGE_ASSET_UNIT,
  COINAGE_PALLET_ACCOUNT,
  EXAMPLE_ASSET_LOCATION,
  EXAMPLE_ASSET_MIN_BALANCE,
} from "../helper/config.ts";
import { ORIGINS } from "../helper/origins.ts";
import { submitPeople } from "../helper/submit.ts";
import { waitFor } from "../helper/wait.ts";

/** Provision the pallet account for Coinage.
 * - A non-sufficient asset must be touched first.
 * - The example asset is sufficient, so minting its minimum balance also creates its account. */
export async function coinageFundPalletAccount(): Promise<void> {
  const call = peopleApi.tx.Assets.mint({
    id: EXAMPLE_ASSET_LOCATION,
    beneficiary: Enum("Id", COINAGE_PALLET_ACCOUNT),
    amount: EXAMPLE_ASSET_MIN_BALANCE,
  });
  await submitPeople("Coinage fund account", ORIGINS.assetIssuer, call);
}

export async function coinageCreateSponsoredInstance(): Promise<number> {
  const instanceId = await peopleApi.query.Coinage.NextInstanceId.getValue();
  const call = peopleApi.tx.Coinage.create_sponsored_instance({
    asset_id: EXAMPLE_ASSET_LOCATION,
    asset_unit: COINAGE_ASSET_UNIT,
    initial_funding: undefined,
  });
  await submitPeople("Coinage create sponsored", ORIGINS.coinageSponsor, call);
  return instanceId;
}

export function coinageNextInstanceId(): Promise<number> {
  return peopleApi.query.Coinage.NextInstanceId.getValue();
}

/** Build the admin call that creates a sufficient instance. */
export function coinageCreateSufficientInstanceCall() {
  return peopleApi.tx.Coinage.create_sufficient_instance({
    asset_id: EXAMPLE_ASSET_LOCATION,
    asset_unit: COINAGE_ASSET_UNIT,
  });
}

/** Build the admin call that converts a sponsored instance to sufficient. */
export function coinageMakeInstanceSufficientCall(instanceId: number) {
  return peopleApi.tx.Coinage.make_instance_sufficient({ instance_id: instanceId });
}

export async function waitForSufficientInstance(instanceId: number): Promise<void> {
  await waitFor(`Coinage instance ${instanceId}`, async () => {
    const instance = await peopleApi.query.Coinage.Instances.getValue(instanceId);
    return instance?.mode.type === "Sufficient";
  });
}
