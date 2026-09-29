# Verified PreviewNet run

Verified at `2026-09-29T05:07:54.101Z` against `wss://previewnet.substrate.dev/people`.
Runtime: `next-people-paseo` version `3003000`.

All ten extrinsics are in canonical finalized blocks. The verifier re-fetched each
extrinsic, checked its Blake2-256 hash and required its `System.ExtrinsicSuccess`
event. These are actual submitted transactions, not encoded examples.

Transaction hashes open the exact extrinsic in PAPI Console, with PreviewNet People
selected. Block-index links open the containing block.

| Operation | Block-index | Transaction hash |
|---|---|---|
| load holders 0..4 | [164813-2](https://dev.papi.how/explorer/0x98d08daa6491034f885740e3b3458a1831537098c6ae0dc39070dd8f9fb473e7#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0xfcc69cc496140bcac9308d4371168c3ecf22658828c477c6280a8ee40e06ca0a](https://dev.papi.how/explorer/0x98d08daa6491034f885740e3b3458a1831537098c6ae0dc39070dd8f9fb473e7#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| load holders 5..9 | [164818-2](https://dev.papi.how/explorer/0xf58009519136effaf9a3034150274bc5932a30e483684cabff9730c5bdbfc709#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0x24f84b018be973f7d9d241ef36bd2ce7a1e28e6ca5cf5d56c043878ebb8b87b7](https://dev.papi.how/explorer/0xf58009519136effaf9a3034150274bc5932a30e483684cabff9730c5bdbfc709#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| buy unload token 0 | [164823-2](https://dev.papi.how/explorer/0xc6f600708200a3fc8efbd7dcffd9f153fa0cf183bb3887fd7c68eaeb9079b92b#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0x58f80733ee439321764aebde81f6c4d1e200aef4feae0957b9030c1d5881a6d8](https://dev.papi.how/explorer/0xc6f600708200a3fc8efbd7dcffd9f153fa0cf183bb3887fd7c68eaeb9079b92b#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| unload into Alice 0 | [164870-2](https://dev.papi.how/explorer/0x551de85f1b448403f333f44e74f458d31d1b3c8c27c05f52fadd53480129a829#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0xcc39e614558d3ce749db96340c5be21ad0d2bc232725920c05e506fa8929a7e8](https://dev.papi.how/explorer/0x551de85f1b448403f333f44e74f458d31d1b3c8c27c05f52fadd53480129a829#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| Alice -> Bob (round 0) | [164875-2](https://dev.papi.how/explorer/0xecd03806e663521a2fdf568762bb0b106145b67147c9ff7ef4fee29c89507882#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0x7809c6991b2ce1342d53bac3582d701a64c24e63b061dbc460d2d18881d61f73](https://dev.papi.how/explorer/0xecd03806e663521a2fdf568762bb0b106145b67147c9ff7ef4fee29c89507882#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| Bob -> Charlie (round 0) | [164881-2](https://dev.papi.how/explorer/0xb0ce300d5f92409846f261485a8395b7106fe8ce6f176ea66a69c03d5dfbf0a9#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0x7749787d01d1f9493c1349ef645e27bce433a7b9a186cecf1ef5795a3ebcb9eb](https://dev.papi.how/explorer/0xb0ce300d5f92409846f261485a8395b7106fe8ce6f176ea66a69c03d5dfbf0a9#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| Charlie -> Dave (round 0) | [164887-2](https://dev.papi.how/explorer/0xf570556f399a8556cfaa4db59d776ac25b2b1a8afc10ad3b57038d018a4f30ac#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0xa8b4b3b676f6fc3a20a8a13c56de69d0aa1ac7da7732a10c70e4378837c33c53](https://dev.papi.how/explorer/0xf570556f399a8556cfaa4db59d776ac25b2b1a8afc10ad3b57038d018a4f30ac#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| Dave -> Ferdie (round 0) | [164893-2](https://dev.papi.how/explorer/0x0266b61466b7707521e8d0b247e2ca6de3a90e648611398910ae5313c9fba820#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0xc1f07611b54a5d6b4538a84d43e77ad4c3dba9959b4ee167768845bf00b6df0d](https://dev.papi.how/explorer/0x0266b61466b7707521e8d0b247e2ca6de3a90e648611398910ae5313c9fba820#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| Ferdie -> Alice (round 0) | [164899-2](https://dev.papi.how/explorer/0x85986b6c658c4d57d08e216b080874af92afe2e2d76984d8058a7225b8762578#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0x8993971a7bff9f4b76c1c1ff24fc05428fbe60cd57dd20c8f8f1a2fc60fd4c19](https://dev.papi.how/explorer/0x85986b6c658c4d57d08e216b080874af92afe2e2d76984d8058a7225b8762578#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |
| recycle Alice 0 | [164905-2](https://dev.papi.how/explorer/0xe706ed28a9246243e1f4db1f4eb520f8926a3a5a7043e748f549ec0dfe5f52ce#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople) | [0xb1859d61abb59d5d0fe3ef6c335e3a93e940611e68e59802f2a6f3535cd4d979](https://dev.papi.how/explorer/0xe706ed28a9246243e1f4db1f4eb520f8926a3a5a7043e748f549ec0dfe5f52ce#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople&tx=2) |

The coin moved through Alice → Bob → Charlie → Dave → Ferdie → Alice. Its age
was checked as 1 through 5 at the respective inclusion blocks. Alice then loaded
it into recycler ring 0; revision 37 included 46 members at
[0x154bd623583b2d44fa467a57cdab6a26f5d439cca648f10a7adc12028a39df3c](https://dev.papi.how/explorer/0x154bd623583b2d44fa467a57cdab6a26f5d439cca648f10a7adc12028a39df3c#networkId=custom&endpoint=wss%3A%2F%2Fpreviewnet.substrate.dev%2Fpeople).

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
`check_validtx_priority.sh` passed. `check_todos.sh` failed on existing todo URL
formatting in unrelated Rust files; no Rust source was changed. CI was not run
as part of the local verification, and no PR was opened.
