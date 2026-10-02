import { readFileSync } from "node:fs";
export type Chunk = {
  ringExponent: "R2e9" | "R2e10";
  pageIndex: number;
  encodedChunks: Uint8Array;
};

const chunkDirectory = new URL("../../chunks/", import.meta.url);
const pages = [
  ["R2e9", 3],
  ["R2e10", 5],
] as const;

export const chunks: Chunk[] = pages.flatMap(([ringExponent, count]) =>
  Array.from({ length: count }, (_, pageIndex) => ({
    ringExponent,
    pageIndex,
    encodedChunks: new Uint8Array(
      readFileSync(new URL(`${ringExponent.toLowerCase()}/page-${pageIndex}.bin`, chunkDirectory)),
    ),
  })),
);
