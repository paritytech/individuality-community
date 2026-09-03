import { peopleApi } from "../helper/api.ts";
import { ASSET_HUB_PARA_ID } from "../helper/config.ts";
import { ORIGINS } from "../helper/origins.ts";
import { submitPeople } from "../helper/submit.ts";

export async function subscribeAssetHub(): Promise<void> {
  const call = peopleApi.tx.MembersNotifier.subscribe_whitelisted({
    subscriber_parachain_id: ASSET_HUB_PARA_ID,
  });
  await submitPeople("Subscribe Asset Hub", ORIGINS.authorized, call);
}
