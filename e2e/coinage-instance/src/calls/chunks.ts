import { Enum } from "polkadot-api";

import { peopleApi } from "../helper/api.ts";
import type { Chunk } from "../helper/chunks.ts";

export function buildAuthorizedChunkCall(chunk: Chunk) {
  return peopleApi.tx.ChunksManager.add_chunks({
    ring_exponent: Enum(chunk.ringExponent),
    page_index: chunk.pageIndex,
    encoded_chunks: chunk.encodedChunks,
  });
}
