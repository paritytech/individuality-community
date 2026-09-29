# Verified PreviewNet run

Verified at `2026-09-29T05:07:54.101Z` against `wss://previewnet.substrate.dev/people`.
Runtime: `next-people-paseo` version `3003000`.

All ten extrinsics are in canonical finalized blocks. The verifier re-fetched each
extrinsic, checked its Blake2-256 hash and required its `System.ExtrinsicSuccess`
event. These are actual submitted transactions, not encoded examples.

| Operation | Block-index | Transaction hash |
|---|---|---|
| load holders 0..4 | 164813-2 | `0xfcc69cc496140bcac9308d4371168c3ecf22658828c477c6280a8ee40e06ca0a` |
| load holders 5..9 | 164818-2 | `0x24f84b018be973f7d9d241ef36bd2ce7a1e28e6ca5cf5d56c043878ebb8b87b7` |
| buy unload token 0 | 164823-2 | `0x58f80733ee439321764aebde81f6c4d1e200aef4feae0957b9030c1d5881a6d8` |
| unload into Alice 0 | 164870-2 | `0xcc39e614558d3ce749db96340c5be21ad0d2bc232725920c05e506fa8929a7e8` |
| Alice -> Bob (round 0) | 164875-2 | `0x7809c6991b2ce1342d53bac3582d701a64c24e63b061dbc460d2d18881d61f73` |
| Bob -> Charlie (round 0) | 164881-2 | `0x7749787d01d1f9493c1349ef645e27bce433a7b9a186cecf1ef5795a3ebcb9eb` |
| Charlie -> Dave (round 0) | 164887-2 | `0xa8b4b3b676f6fc3a20a8a13c56de69d0aa1ac7da7732a10c70e4378837c33c53` |
| Dave -> Ferdie (round 0) | 164893-2 | `0xc1f07611b54a5d6b4538a84d43e77ad4c3dba9959b4ee167768845bf00b6df0d` |
| Ferdie -> Alice (round 0) | 164899-2 | `0x8993971a7bff9f4b76c1c1ff24fc05428fbe60cd57dd20c8f8f1a2fc60fd4c19` |
| recycle Alice 0 | 164905-2 | `0xb1859d61abb59d5d0fe3ef6c335e3a93e940611e68e59802f2a6f3535cd4d979` |

The coin moved through Alice → Bob → Charlie → Dave → Ferdie → Alice. Its age
was checked as 1 through 5 at the respective inclusion blocks. Alice then loaded
it into recycler ring 0; revision 37 included 46 members at
`0x154bd623583b2d44fa467a57cdab6a26f5d439cca648f10a7adc12028a39df3c`.

Ten denomination-8 holdings remain in instance 0. Each represents 2560000 raw
units of the configured backing asset. Local voucher secrets are retained under
the ignored run directory; this evidence contains no voucher entropy.

- [Raw verified transactions and on-chain events](previewnet-transactions.json)
- [Coin state checks and final recycler membership](previewnet-state.json)

During development, rejected submissions exposed a too-short mortality window
and differences from the older test harness: compressed commitments and bounded
proof byte vectors. The final script uses an eight-block era, `verifiablejs@1.6.0`
and metadata-driven proof encoding. Rejected attempts are not counted above.

This confirms the on-chain Coinage lifecycle. It does not claim that Alex’s mobile
full-privacy UI has been tested or that its waiting policy is bypassed.

Validation: `pnpm check` and all seven `pnpm test` cases passed. The repository's
`scripts/check.sh` passed formatting, clippy and all 2734 Rust tests.
`check_validtx_priority.sh` passed. `check_todos.sh` failed on existing TODO URL
formatting in unrelated Rust files; no Rust source was changed. CI was not run
as part of the local verification, and no PR was opened.
