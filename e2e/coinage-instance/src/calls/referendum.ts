import { Enum } from "polkadot-api";

import { assetHubApi, peopleApi } from "../helper/api.ts";
import { PEOPLE_PARA_ID } from "../helper/config.ts";
import { ORIGINS } from "../helper/origins.ts";
import { submitAssetHub } from "../helper/submit.ts";

type AssetHubCall = Parameters<typeof assetHubApi.tx.Utility.batch_all>[0]["calls"][number];
type PeopleCall = Parameters<typeof peopleApi.tx.Utility.batch_all>[0]["calls"][number];

/** Build an Asset Hub XCM call that dispatches a batch of People calls as Root. People maps the
 * Asset Hub location to its superuser origin and permits unpaid execution from it. */
export async function buildPeopleTransactCall(calls: PeopleCall[]): Promise<AssetHubCall> {
  const peopleBatch = peopleApi.tx.Utility.batch_all({ calls });
  const xcm = assetHubApi.tx.PolkadotXcm.send({
    dest: Enum("V5", {
      parents: 1,
      interior: Enum("X1", Enum("Parachain", PEOPLE_PARA_ID)),
    }),
    message: Enum("V5", [
      Enum("UnpaidExecution", { weight_limit: Enum("Unlimited"), check_origin: undefined }),
      Enum("Transact", {
        origin_kind: Enum("Superuser"),
        fallback_max_weight: undefined,
        call: await peopleBatch.getEncodedData(),
      }),
    ]),
  });
  return xcm.decodedCall;
}

/** Submit a local stand-in for an Asset Hub governance proposal. */
export async function submitReferendum(label: string, calls: AssetHubCall[]): Promise<void> {
  const batch = assetHubApi.tx.Utility.batch_all({ calls });
  await submitAssetHub(label, ORIGINS.referendum, batch);
}
