import { peopleApi, relayApi } from "../helper/api.ts";
import { ASSET_HUB_PARA_ID, PEOPLE_PARA_ID } from "../helper/config.ts";
import { submitRelayRoot } from "../helper/submit.ts";
import { waitFor } from "../helper/wait.ts";

type HrmpConfig = { hrmp_channel_max_capacity: number; hrmp_channel_max_message_size: number };

export async function openHrmpChannels(): Promise<void> {
  const config = (await relayApi.query.Configuration!.ActiveConfig!.getValue()) as HrmpConfig;
  for (const [sender, recipient] of [
    [PEOPLE_PARA_ID, ASSET_HUB_PARA_ID],
    [ASSET_HUB_PARA_ID, PEOPLE_PARA_ID],
  ]) {
    const call = relayApi.tx.Hrmp!.force_open_hrmp_channel!({
      sender,
      recipient,
      max_capacity: config.hrmp_channel_max_capacity,
      max_message_size: config.hrmp_channel_max_message_size,
    });
    await submitRelayRoot(`HRMP ${sender}->${recipient}`, call);
  }
}

export async function waitForHrmpChannel(): Promise<void> {
  await waitFor(`HRMP ${PEOPLE_PARA_ID}->${ASSET_HUB_PARA_ID}`, async () => {
    const state = await peopleApi.query.ParachainSystem.RelevantMessagingState.getValue();
    return state?.egress_channels.some(([id]) => id === ASSET_HUB_PARA_ID) === true;
  });
}
