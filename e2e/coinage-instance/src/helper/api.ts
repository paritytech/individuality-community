import { assethub, people } from "@polkadot-api/descriptors";
import { createClient } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";

import { RPC_ASSET_HUB, RPC_PEOPLE, RPC_RELAY } from "./config.ts";
import { log } from "./log.ts";

function connect(chain: string, url: string) {
  return createClient(
    getWsProvider(url, {
      onStatusChanged: status => log(`WebSocket ${chain}`, status.type.toLowerCase(), url),
    }),
  );
}

const relayClient = connect("relay", RPC_RELAY);
const peopleClient = connect("people", RPC_PEOPLE);
const assetHubClient = connect("asset-hub", RPC_ASSET_HUB);

export const relayApi = relayClient.getUnsafeApi();
export const peopleApi = peopleClient.getTypedApi(people);
export const assetHubApi = assetHubClient.getTypedApi(assethub);

export function disconnect(): void {
  relayClient.destroy();
  peopleClient.destroy();
  assetHubClient.destroy();
}
