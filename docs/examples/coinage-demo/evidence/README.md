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

## Devnet payment reproduction — 30 September 2026

Eight successful extrinsics were submitted to public Paseo People (the documented
Devnet), instance 0, between 16:57 and 17:03 GMT+7. A separate RPC provider
(`wss://rpc.interweb-it.com/people-paseo`) re-fetched the canonical finalized blocks,
verified transaction hashes and checked `System.ExtrinsicSuccess`.

The five-minute payment delay was **not reproduced** in this bounded run. Timing
below is measured from the start of signing to the client's finalized notification;
it includes RPC and finality latency and is not a phone UI measurement.

| Stage | Time to finality | Transaction proof |
|---|---:|---|
| Setup: load backing asset | 65.3 s | [0xc45bb873cf6e6a0988ee0878d6820c4f6f807c2df6b7e4617f140f80230a91ed](https://dev.papi.how/explorer/0xbf4f3807e2fa6b8230fdcd5cbacdea245104ecc167cab51fa3853e2df878b65b#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |
| Setup: buy unload token | 65.4 s | [0xe445bff80da0eb20b7d1eb7916321da3d79b43da9654c0bed05b0f31d9a2889f](https://dev.papi.how/explorer/0xcb6ef6d5e142350eb5d416ccca747d0c059f9629c1ad4b08fe8ac6b521bf42b5#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |
| Setup: unload into Alice | 28.3 s | [0x0525079f3e6b2db8aab8e0becadf33ddf23337298e26a3fd0c87ccfd7320447e](https://dev.papi.how/explorer/0x0bcc68a7c2dbf601be090e6470393b330efda75b9ee65f2895963614325519f7#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |
| Direct: Alice → Bob | 32.5 s | [0x78f2705e1237b01ec212b703e9b63289133674c49f1843897f44b3772babd393](https://dev.papi.how/explorer/0x6de94d0839f558791f567e41dfc1e1d18fcb9473c781b4600a191ee2f6a5f425#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |
| Exact handoff: recipient claim | 28.8 s | [0x64d4661bbbb126ae2c8dbd964489cd41cc7aca58d940f01d23320bbd976fdd2f](https://dev.papi.how/explorer/0x20125eb2baf7cc36196623a2648eadef4e183b69180ed83250bebb773be6091e#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |
| Prepared payment: split | 27.9 s | [0xbb1810efba0e5d740bf59f6aca94a3b574761f36c2927e7b656036306c0c4094](https://dev.papi.how/explorer/0x132a7c9a1bc3fdb4ac73d4e28e74abdc30e722c4a8f427f9c0b72781be3b1df0#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |
| Prepared payment: recipient claim | 32.4 s | [0x1bf02bca07b644050416366a056d6f5964112348bcf5c0f04c01f0282cb0f5dc](https://dev.papi.how/explorer/0xb7a492f5c168910c7c00d3928adf3803baf1717c7a86be6b4592ee4aa01940ad#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |
| Explicit recycler load | 28.3 s | [0x0194e9daa8908ef659b0f1d6637d3d8b5d14f9191be771149accd8ecd04afa87](https://dev.papi.how/explorer/0x33c6900a4e46e2e0bf7c948c7f1c2bb765ee9943fe29460d4650f2be114e8961#networkId=custom&endpoint=wss%3A%2F%2Fpeople-paseo.rotko.net&tx=2) |

The prepared payment took 60.3 seconds across split and claim. After the explicit
recycler load finalized, confirmed membership became available another 10.9 seconds
later (about 39.2 seconds from load signing). Fastest adds no optional privacy wait
once membership is confirmed. No background bot traffic was generated.

The setup uses one denomination-5 holding (320000 raw backing-asset units). The
finished run leaves a denomination-4 change coin at Eve and a denomination-4
voucher in the recycler. Each represents 160000 raw backing-asset units. No voucher
secrets are published. All six account keys are public development keys.

This models direct transfer, local exact-key handoff followed by recipient claim,
split followed by recipient claim, and explicit recycling. It does not exercise
phone messaging, mobile claim queues, input-wait recovery, UI classification or
personhood allowance acquisition. It cannot identify Alex's payment or exclude a
phone-side or earlier network delay. The entire diagnostic sequence includes
setup and several separate payments; its total runtime is not one payment's delay.

- [Verified transactions, encoded extrinsics and on-chain events](devnet-reproduction-transactions.json)
- [Timing receipts, coin-state checks and confirmed recycler membership](devnet-reproduction-state.json)
- [Reproduction script](../reproduce.ts)

Validation: TypeScript checking and all nine script tests passed. Repository
`scripts/check.sh` passed formatting, clippy and all 2831 Rust tests.
`check_validtx_priority.sh` passed; `check_todos.sh` still reports existing TODO
URL formatting in unrelated Rust files. No Rust source was changed.
