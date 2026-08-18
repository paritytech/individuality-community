// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg(test)]

use codec::Encode;
use frame_support::{
	assert_ok, derive_impl,
	dispatch::{DispatchErrorWithPostInfo, GetDispatchInfo},
	match_types,
	pallet_prelude::Debug,
	parameter_types,
	storage::with_transaction,
	traits::{fungible::Mutate, Randomness, UnixTime},
	PalletId,
};
use frame_system::{
	offchain::{CreateAuthorizedTransaction, CreateBare, CreateTransaction, CreateTransactionBase},
	EnsureRoot,
};
use indiv_pallet_members::Root;
use indiv_pallet_mob_rule::MOB_CONTEXT;
use indiv_pallet_people::{
	extension::{AsPerson, AsPersonInfo},
	pallet::PEOPLE_MEMBER_IDENTIFIER,
};
use indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipantInfo;
use indiv_support::{
	crypto::{BandersnatchSuite, BandersnatchVrfVerifiable, GenerateVerifiable},
	traits::{AllocateStorage, Context, RevisionIndex, RingIndex, RI_ZERO},
};
use rand::{prelude::ThreadRng, Rng};
use sp_arithmetic::Percent;
use sp_core::{ConstU16, ConstU32, ConstU64, H256};
#[cfg(feature = "runtime-benchmarks")]
use sp_runtime::BoundedVec;
use sp_runtime::{
	generic::ExtensionVersion,
	testing::{TestSignature as TicketSignature, UintAuthorityId},
	traits::{Applyable, BlakeTwo256, Checkable, IdentityLookup},
	transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidityError},
	BuildStorage, DispatchError, DispatchResult, TransactionOutcome,
};
use verifiable::Entropy;
use xcm::v5::Location;

pub type TransactionExtension = (
	frame_system::AuthorizeCall<Test>,
	indiv_pallet_people::extension::AsPerson<Test>,
	indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant<Test>,
	frame_system::CheckNonce<Test>,
);

pub type Header = sp_runtime::generic::Header<u64, sp_runtime::traits::BlakeTwo256>;
pub type Block = sp_runtime::generic::Block<Header, UncheckedExtrinsic>;
pub type UncheckedExtrinsic = sp_runtime::generic::UncheckedExtrinsic<
	u64,
	RuntimeCall,
	sp_runtime::testing::UintAuthorityId,
	TransactionExtension,
>;

pub(crate) const EXTENSION_VERSION: ExtensionVersion = 0;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		Balances: pallet_balances,
		ChunksManager: indiv_pallet_chunks_manager,
		Members: indiv_pallet_members,
		People: indiv_pallet_people,
		MobRule: indiv_pallet_mob_rule,
		ProofOfInk: indiv_pallet_proof_of_ink,
	}
);

pub type ProofOfInkCall = indiv_pallet_proof_of_ink::Call<Test>;
pub type PeopleCall = indiv_pallet_people::Call<Test>;

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block;
	type RuntimeEvent = RuntimeEvent;
	type BlockHashCount = ConstU64<250>;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<u64>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = ConstU16<42>;
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}

parameter_types! {
	pub const ExistentialDeposit: u64 = 5;
	pub const MaxReserves: u32 = 50;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type Balance = u64;
	type RuntimeEvent = RuntimeEvent;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = ();
	type MaxLocks = ();
	type MaxReserves = MaxReserves;
	type ReserveIdentifier = [u8; 8];
	type RuntimeHoldReason = RuntimeHoldReason;
	type RuntimeFreezeReason = RuntimeFreezeReason;
	type FreezeIdentifier = ();
}

// The TestAccountContexts type, which must implement trait Contains and return true only for the
// chosen set of contexts.
pub const MOCK_CONTEXT: Context = *b"pop:polkadot.network/mock       ";
match_types! {
	pub type TestAccountContexts: impl Contains<Context> = {
		&MOB_CONTEXT| &MOCK_CONTEXT
	};
}

impl<LocalCall> CreateBare<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	fn create_bare(call: Self::RuntimeCall) -> Self::Extrinsic {
		UncheckedExtrinsic::new_bare(call)
	}
}

impl<LocalCall> CreateTransactionBase<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	type Extrinsic = UncheckedExtrinsic;
	type RuntimeCall = RuntimeCall;
}

impl<LocalCall> CreateTransaction<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	type Extension = TransactionExtension;
	fn create_transaction(
		call: <Self as CreateTransactionBase<LocalCall>>::RuntimeCall,
		extension: Self::Extension,
	) -> Self::Extrinsic {
		UncheckedExtrinsic::new_transaction(call, extension)
	}
}

impl<LocalCall> CreateAuthorizedTransaction<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	fn create_extension() -> Self::Extension {
		(
			frame_system::AuthorizeCall::new(),
			indiv_pallet_people::extension::AsPerson::new(None),
			indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::new(None),
			frame_system::CheckNonce::from(0),
		)
	}
}

/// Chunk page size for the integration tests - must match what we use in genesis.
pub const CHUNK_PAGE_SIZE: u32 = 1024;

#[cfg(feature = "runtime-benchmarks")]
pub struct ChunksManagerBenchHelper;

#[cfg(feature = "runtime-benchmarks")]
impl
	indiv_pallet_chunks_manager::BenchmarkHelper<
		<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk,
	> for ChunksManagerBenchHelper
{
	fn chunk_page() -> Vec<<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk> {
		use indiv_support::genesis::ring_verifier_builder_params;
		use verifiable::ring::RingDomainSize;
		let chunks = ring_verifier_builder_params(RingDomainSize::Domain16);
		chunks.into_iter().take(CHUNK_PAGE_SIZE as usize).collect()
	}
}

impl indiv_pallet_chunks_manager::Config for Test {
	type WeightInfo = ();
	type Chunk = <BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk;
	type PageSize = ConstU32<CHUNK_PAGE_SIZE>;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ChunksManagerBenchHelper;
}

parameter_types! {
	pub const FlexibleRingExp: indiv_support::traits::RingExponent = indiv_support::traits::RingExponent::R2e9;
	pub const PeopleCollectionOwner: Location = Location::here();
}

impl indiv_pallet_members::Config for Test {
	type WeightInfo = ();
	type Crypto = BandersnatchVrfVerifiable;
	type Location = Location;
	type ChunksManager = ChunksManager;
	type Clock = TestClock;
	type MaxCollections = ConstU32<10>;
	type OnboardingQueuePageSize = ConstU32<512>;
	type MaxFlexibleRingExponent = FlexibleRingExp;
	type RingBuildingMemberLimit = ConstU32<100>;
	type OldRootRetentionDuration = ConstU64<600>;
	type OnRingRootChange = ();
	type OffchainWorkerInterval = ConstU64<1>;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ();
}

#[cfg(feature = "runtime-benchmarks")]
pub struct PeopleBenchHelper;

#[cfg(feature = "runtime-benchmarks")]
impl
	indiv_pallet_people::BenchmarkHelper<
		<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk,
	> for PeopleBenchHelper
{
	fn valid_account_context() -> Context {
		[0u8; 32]
	}

	fn initialize_chunks() -> Vec<<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk> {
		use indiv_support::genesis::ring_verifier_builder_params;
		use verifiable::ring::RingDomainSize;
		let chunks = ring_verifier_builder_params(RingDomainSize::Domain16);
		chunks.into_iter().take(CHUNK_PAGE_SIZE as usize).collect()
	}
}

impl indiv_pallet_people::Config for Test {
	type WeightInfo = ();
	type AccountContexts = TestAccountContexts;
	type MemberService = Members;
	type CollectionOwner = PeopleCollectionOwner;
	type OnboardingQueuePageSize = ConstU32<512>;
	type RingExponent = FlexibleRingExp;
	type StaleAliasCleanupInterval = ConstU64<5>;
	type SelfInclusionDelay = ConstU64<3600>;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = PeopleBenchHelper;
}

parameter_types! {
	pub const MobRulePotId: PalletId = PalletId(*b"MobRwrds");
	pub const MinTurnoutPercentage: Percent = Percent::from_percent(10);
	pub const BalancesLocation: Location = Location::here();
}

impl indiv_pallet_mob_rule::Config for Test {
	type WeightInfo = ();
	type Currency = Balances;
	type CurrencyLocationInfo = BalancesLocation;
	type Clock = TestClock;
	type EnsurePerson = indiv_pallet_people::EnsurePersonalAliasInContext<Test>;
	type MaxVoteClaimDuration = ConstU64<7200>;
	type MinCaseDuration = ConstU32<86400>; // 24 * 60 * 60
	type MaxVotingDuration = ConstU32<1209600>; // 14 * 24 * 60 * 60
	type MinTurnoutNominal = ConstU32<10>;
	type MinTurnoutPercentage = MinTurnoutPercentage;
	type MaxPayoutRoundSchedules = ConstU32<5>;
	type VotingPenaltyDuration = ConstU64<10>;
	type InterventionOrigin = EnsureRoot<u64>;
	type PotId = MobRulePotId;
	type MaxVotesClaimable = ConstU32<10>;
	type OffchainWorkInterval = ConstU64<10>;
	type CleanVotesBatchSize = ConstU32<10>;
	type VotesOpenForClaimsDuration = ConstU32<3600>; // 60 * 60
	type MinimumVoterThreshold = ConstU32<1>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = MobRuleBenchHelper;
}

parameter_types! {
	pub const PoiPotId: PalletId = PalletId(*b"PoiPot__");
}

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_proof_of_ink::BenchmarkHelper<Test> for Test {
	fn sign(
		_seed: u64,
		_msg: &[u8],
	) -> <Test as indiv_pallet_proof_of_ink::Config>::TicketSignature {
		unimplemented!();
	}

	fn create_ticket(
		_seed: u64,
	) -> (
		<Test as indiv_pallet_proof_of_ink::Config>::TicketPublic,
		<Test as indiv_pallet_proof_of_ink::Config>::Ticket,
	) {
		unimplemented!();
	}

	fn create_tickets(
		_seed: u64,
	) -> BoundedVec<
		indiv_pallet_proof_of_ink::ReferralTicket<
			<Test as indiv_pallet_proof_of_ink::Config>::Ticket,
		>,
		<Test as indiv_pallet_proof_of_ink::Config>::MaxActiveReferrals,
	> {
		unimplemented!();
	}

	fn build_person_origin(_personal_id: indiv_support::traits::PersonalId) -> RuntimeOrigin {
		unimplemented!();
	}

	fn setup_currency() {}
}

impl indiv_pallet_proof_of_ink::Config for Test {
	type WeightInfo = ();
	type Deposit = ();
	type People = People;
	type EnsurePerson = indiv_pallet_people::EnsurePersonalIdentity<Test>;
	type TicketSignature = TicketSignature;
	type TicketPublic = UintAuthorityId;
	type Ticket = u64;
	type Oracle = MobRule;
	type Randomness = TestRandommess;
	type DataStore = TestDataStore;
	type MaxActiveReferrals = ConstU32<10>;
	type MaxRetryAttempts = ConstU32<1>;
	type MaxReimbursementValues = ConstU32<10>;
	type Currency = Balances;
	type PotId = PoiPotId;
	type InvitationsOrigin = EnsureRoot<Self::AccountId>;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	type Crypto = BandersnatchVrfVerifiable;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = Test;
}

pub struct TestClock;
impl UnixTime for TestClock {
	fn now() -> core::time::Duration {
		core::time::Duration::from_secs(0)
	}
}

#[cfg(feature = "runtime-benchmarks")]
pub struct MobRuleBenchHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_mob_rule::benchmarking::BenchmarkHelper<Test> for MobRuleBenchHelper {
	fn set_valid_time() {
		unimplemented!();
	}

	fn setup_currency() {
		unimplemented!();
	}
}

pub struct TestRandommess;
impl Randomness<[u8; 32], u64> for TestRandommess {
	fn random(_subject: &[u8]) -> ([u8; 32], u64) {
		(Default::default(), 0)
	}
}

pub struct TestDataStore;
impl AllocateStorage<u64> for TestDataStore {
	fn allocate_storage(_who: &u64, _len: u64, _count: u32) -> DispatchResult {
		Ok(())
	}
	fn refresh_allocation(_who: &u64) -> DispatchResult {
		Ok(())
	}
}

#[allow(dead_code)]
pub fn advance_to(b: u64) {
	while System::block_number() < b {
		System::set_block_number(System::block_number() + 1);
	}
}

#[allow(dead_code)]
pub fn advance_by(b: u64) {
	for _ in 0..b {
		System::set_block_number(System::block_number() + 1);
	}
}

pub struct ConfigRecord;

pub fn new_config() -> ConfigRecord {
	ConfigRecord
}

pub struct TestExt(ConfigRecord);
#[allow(dead_code)]
impl TestExt {
	pub fn new() -> Self {
		Self(new_config())
	}

	pub fn execute_with<R>(self, f: impl Fn() -> R) -> R {
		new_test_ext().execute_with(f)
	}
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	// Get the chunk page hashes for the genesis config
	let chunk_page_hashes = indiv_support::genesis::ring_verifier_all_builder_params_hashes::<
		BandersnatchSuite,
	>(CHUNK_PAGE_SIZE);

	RuntimeGenesisConfig {
		system: Default::default(),
		chunks_manager: indiv_pallet_chunks_manager::GenesisConfig::<Test> {
			encoded_chunk_page_hashes: chunk_page_hashes,
			_phantom: Default::default(),
		},
		balances: pallet_balances::GenesisConfig::<Test> { ..Default::default() },
		..Default::default()
	}
	.build_storage()
	.unwrap()
	.into()
}

/// Initialize chunks in the ChunksManager for testing.
/// This must be called before tests that use ring operations.
pub fn initialize_chunks() {
	use frame_support::BoundedVec;
	use indiv_support::traits::RingExponent;
	use verifiable::ring::RingDomainSize;

	let domain_size: RingDomainSize =
		RingExponent::R2e9.try_into().expect("R2e9 should convert to Domain11");
	let chunks = indiv_support::genesis::ring_verifier_builder_params(domain_size);

	// Write chunks directly to storage, paginated by CHUNK_PAGE_SIZE
	let page_size = CHUNK_PAGE_SIZE as usize;
	for (page_idx, chunk_page) in chunks.chunks(page_size).enumerate() {
		let bounded: BoundedVec<_, _> = chunk_page
			.iter()
			.cloned()
			.map(indiv_pallet_chunks_manager::UncheckedChunk)
			.collect::<Vec<_>>()
			.try_into()
			.expect("page size matches");
		indiv_pallet_chunks_manager::Chunks::<Test>::insert(
			RingExponent::R2e9,
			page_idx as u32,
			bounded,
		);
	}
}

/// Create the people collection in the Members pallet.
/// This must be called before any ring operations.
pub fn create_people_collection() {
	use indiv_support::traits::{AppendOnlyMembers, RingExponent, RingMode};

	<Members as AppendOnlyMembers>::create_collection(
		PeopleCollectionOwner::get(),
		PEOPLE_MEMBER_IDENTIFIER,
		10, // onboarding size
		RingMode::Flexible,
		RingExponent::R2e9,
		None,
	)
	.expect("Failed to create people collection");
}

/// Set the onboarding size for the people collection.
pub fn set_people_onboarding_size(size: u32) {
	assert_ok!(Members::set_onboarding_size(root(), *PEOPLE_MEMBER_IDENTIFIER, size));
}

/// Trigger onboarding and ring building for all pending people.
pub fn build_rings() {
	Members::process_maintenance();
}

#[allow(unused)]
pub fn signed(who: u64) -> RuntimeOrigin {
	RuntimeOrigin::signed(who)
}

pub fn root() -> RuntimeOrigin {
	RuntimeOrigin::root()
}

#[allow(unused)]
pub fn nobody() -> RuntimeOrigin {
	RuntimeOrigin::none()
}

type AccountIdOf<S> = <S as frame_system::Config>::AccountId;

type EncodedPublicKey = <BandersnatchVrfVerifiable as GenerateVerifiable>::Member;
type SecretKey = <BandersnatchVrfVerifiable as GenerateVerifiable>::Secret;

pub fn account_id_to_bandersnatch_key_pair(
	account_id: AccountIdOf<Test>,
) -> (EncodedPublicKey, SecretKey) {
	let entropy: Entropy = account_id.to_le_bytes().repeat(4).try_into().unwrap();
	let secret_key = BandersnatchVrfVerifiable::new_secret(entropy);
	let pub_key = BandersnatchVrfVerifiable::member_from_secret(&secret_key);
	(pub_key, secret_key)
}

pub fn register_alias(
	(pub_key, secret_key): (&EncodedPublicKey, &SecretKey),
	(_sender_account_id, alias_account_id): (AccountIdOf<Test>, AccountIdOf<Test>),
	context: Context,
) -> Result<(), TransactionExecutionError> {
	let set_alias_account_call: RuntimeCall =
		indiv_pallet_people::Call::<Test>::set_alias_account {
			account: alias_account_id,
			call_valid_at: frame_system::Pallet::<Test>::block_number(),
		}
		.into();

	let tx_ext_part = (
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
		frame_system::CheckNonce::<Test>::from(0),
	);

	use indiv_pallet_members::{RingKeys, RingKeysStatus};
	use verifiable::ring::RingDomainSize;
	let all_keys = RingKeys::<Test>::get((PEOPLE_MEMBER_IDENTIFIER, RI_ZERO, 0u32));
	let all_keys_status = RingKeysStatus::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, RI_ZERO);
	let keys = all_keys.into_iter().take(all_keys_status.included as usize);

	let commitment =
		BandersnatchVrfVerifiable::open(RingDomainSize::Domain11, pub_key, keys).unwrap();

	let (proof, _alias) = BandersnatchVrfVerifiable::create(
		commitment,
		secret_key,
		&context,
		&(&(EXTENSION_VERSION, &set_alias_account_call), tx_ext_part)
			.using_encoded(sp_io::hashing::blake2_256),
	)
	.unwrap();

	exec_proof_as_alias_tx(proof, RI_ZERO, context, set_alias_account_call)
}

/// We gather both error into a single type in order to do `assert_ok` and `assert_err` safely.
/// Otherwise, we can easily miss the inner error in a `Resut<Resut<_, _>, _>`.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum TransactionExecutionError {
	Validity(TransactionValidityError),
	// This ignores the post info.
	Dispatch(DispatchErrorWithPostInfo),
}

impl TransactionExecutionError {
	pub fn unwrap_dispatch(self) -> DispatchErrorWithPostInfo {
		let Self::Dispatch(error) = self else {
			panic!("validity error unwrapped as dispatch");
		};
		error
	}
}

impl From<DispatchErrorWithPostInfo> for TransactionExecutionError {
	fn from(e: DispatchErrorWithPostInfo) -> Self {
		TransactionExecutionError::Dispatch(e)
	}
}

impl From<TransactionValidityError> for TransactionExecutionError {
	fn from(e: TransactionValidityError) -> Self {
		TransactionExecutionError::Validity(e)
	}
}

impl From<DispatchError> for TransactionExecutionError {
	fn from(e: DispatchError) -> Self {
		TransactionExecutionError::Dispatch(e.into())
	}
}

impl From<InvalidTransaction> for TransactionExecutionError {
	fn from(e: InvalidTransaction) -> Self {
		TransactionExecutionError::Validity(e.into())
	}
}

/// Execute a transaction with the given origin, call and transaction extension.
pub fn exec_tx(
	who: Option<u64>,
	tx_ext: TransactionExtension,
	call: impl Into<RuntimeCall>,
) -> Result<(), TransactionExecutionError> {
	let tx = match who {
		Some(who) => UncheckedExtrinsic::new_signed(call.into(), who, UintAuthorityId(who), tx_ext),
		None => UncheckedExtrinsic::new_transaction(call.into(), tx_ext),
	};

	let info = tx.get_dispatch_info();
	let len = tx.encoded_size();

	let checked = Checkable::check(tx, &frame_system::ChainContext::<Test>::default())?;

	// validation is always rollbacked in production.
	with_transaction(|| {
		let valid = checked.validate::<Test>(TransactionSource::External, &info, len);

		TransactionOutcome::Rollback(Result::<_, DispatchError>::Ok(valid))
	})
	.unwrap()?;

	checked.apply::<Test>(&info, len)??;

	Ok(())
}

pub fn exec_signed_tx(
	who: u64,
	call: impl Into<RuntimeCall>,
) -> Result<(), TransactionExecutionError> {
	let nonce = frame_system::Account::<Test>::get(who).nonce;
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		indiv_pallet_people::extension::AsPerson::new(None),
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
		frame_system::CheckNonce::from(nonce),
	);

	exec_tx(Some(who), tx_ext, call)
}

pub fn exec_as_alias_tx(
	who: u64,
	call: impl Into<RuntimeCall>,
) -> Result<(), TransactionExecutionError> {
	let nonce = frame_system::Account::<Test>::get(who).nonce;
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		AsPerson::new(Some(AsPersonInfo::AsPersonalAliasWithAccount(nonce))),
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
		frame_system::CheckNonce::from(nonce),
	);

	exec_tx(Some(who), tx_ext, call)
}

pub fn exec_proof_as_alias_tx(
	proof: indiv_pallet_people::ProofOf<Test>,
	ring: RingIndex,
	context: [u8; 32],
	call: impl Into<RuntimeCall>,
) -> Result<(), TransactionExecutionError> {
	let revision = people_revision(ring);
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		AsPerson::new(Some(AsPersonInfo::AsPersonalAliasWithProof(proof, ring, revision, context))),
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
		frame_system::CheckNonce::from(0),
	);

	exec_tx(None, tx_ext, call)
}

pub fn people_revision(ring: RingIndex) -> RevisionIndex {
	Root::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, ring)
		.map(|root| root.revision)
		.expect("people ring root should exist in integration tests")
}

pub fn exec_as_personal_id(
	who: u64,
	call: impl Into<RuntimeCall>,
) -> Result<(), TransactionExecutionError> {
	let nonce = frame_system::Account::<Test>::get(who).nonce;
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		AsPerson::new(Some(AsPersonInfo::AsPersonalIdentityWithAccount(nonce))),
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
		frame_system::CheckNonce::from(nonce),
	);

	exec_tx(Some(who), tx_ext, call)
}

pub fn exec_as_apply_with_sig(
	who: u64,
	call: impl Into<RuntimeCall>,
) -> Result<(), TransactionExecutionError> {
	let nonce = frame_system::Account::<Test>::get(who).nonce;
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		indiv_pallet_people::extension::AsPerson::new(None),
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(Some(
			AsProofOfInkParticipantInfo::AsApplyWithSig(nonce),
		)),
		frame_system::CheckNonce::from(nonce),
	);

	exec_tx(Some(who), tx_ext, call)
}

pub fn exec_as_referred_candidate(
	who: u64,
	call: impl Into<RuntimeCall>,
) -> Result<(), TransactionExecutionError> {
	let nonce = frame_system::Account::<Test>::get(who).nonce;
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		indiv_pallet_people::extension::AsPerson::new(None),
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(Some(
			AsProofOfInkParticipantInfo::AsReferred(nonce),
		)),
		frame_system::CheckNonce::from(nonce),
	);

	exec_tx(Some(who), tx_ext, call)
}

pub fn set_up_reimbursements_and_pot(reward_value: u64) {
	ProofOfInk::set_reimbursement_values(
		root(),
		vec![(reward_value, 100)].try_into().unwrap(),
		vec![(reward_value, 100)].try_into().unwrap(),
	)
	.unwrap();

	Balances::mint_into(&ProofOfInk::proof_of_ink_pot_id(), 1000000).unwrap();
}

/// Call `set_personal_id_account` for the given personal id and account.
pub fn setup_personal_id_account(
	personal_id: u64,
	secret: &<BandersnatchVrfVerifiable as GenerateVerifiable>::Secret,
	account: u64,
) {
	let call = RuntimeCall::People(PeopleCall::set_personal_id_account {
		account,
		call_valid_at: frame_system::Pallet::<Test>::block_number(),
	});
	let other_tx_ext = (
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
		frame_system::CheckNonce::<Test>::from(0),
	);
	// Here we simply ignore implicit as they are null.
	let msg = (&EXTENSION_VERSION, &call, &other_tx_ext).using_encoded(sp_io::hashing::blake2_256);
	let signature = BandersnatchVrfVerifiable::sign(secret, &msg).unwrap();
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		indiv_pallet_people::extension::AsPerson::<Test>::new(Some(
			indiv_pallet_people::extension::AsPersonInfo::AsPersonalIdentityWithProof(
				signature,
				personal_id,
			),
		)),
		other_tx_ext.0,
		other_tx_ext.1,
	);
	assert_ok!(exec_tx(None, tx_ext.clone(), call.clone()));
}

/// Call `set_alias_account` for the given personal id and account.
pub fn setup_alias_account(
	key: &<BandersnatchVrfVerifiable as GenerateVerifiable>::Member,
	secret: &<BandersnatchVrfVerifiable as GenerateVerifiable>::Secret,
	context: Context,
	account: u64,
) {
	use indiv_support::traits::RingPosition;
	use verifiable::ring::RingDomainSize;

	// Look up the member's position to get the ring index.
	let position = indiv_pallet_members::Members::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, key)
		.expect("member not found");
	let RingPosition::Included { ring_index, .. } = position else {
		panic!("member isn't included in a ring")
	};
	let commitment = {
		use indiv_pallet_members::{RingKeys, RingKeysStatus};
		let all_keys = RingKeys::<Test>::get((PEOPLE_MEMBER_IDENTIFIER, ring_index, 0u32));
		let all_keys_status = RingKeysStatus::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, ring_index);
		let keys = all_keys.into_iter().take(all_keys_status.included as usize);
		BandersnatchVrfVerifiable::open(RingDomainSize::Domain11, key, keys).unwrap()
	};
	let call = RuntimeCall::People(PeopleCall::set_alias_account {
		account,
		call_valid_at: frame_system::Pallet::<Test>::block_number(),
	});
	let other_tx_ext = (
		indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
		frame_system::CheckNonce::<Test>::from(0),
	);
	// Here we simply ignore implicit as they are null.
	let msg = (&EXTENSION_VERSION, &call, &other_tx_ext).using_encoded(sp_io::hashing::blake2_256);
	let (proof, _alias) = BandersnatchVrfVerifiable::create(commitment, secret, &context, &msg)
		.expect("proof creation failed");
	let tx_ext = (
		frame_system::AuthorizeCall::new(),
		indiv_pallet_people::extension::AsPerson::<Test>::new(Some(
			indiv_pallet_people::extension::AsPersonInfo::AsPersonalAliasWithProof(
				proof,
				ring_index,
				people_revision(ring_index),
				context,
			),
		)),
		other_tx_ext.0,
		other_tx_ext.1,
	);
	assert_ok!(exec_tx(None, tx_ext.clone(), call.clone()));
}
// Used to generate key pairs with random entropy
pub struct BandersnatchKeyPairGenerator {
	rng: ThreadRng,
}

impl BandersnatchKeyPairGenerator {
	pub fn new() -> Self {
		BandersnatchKeyPairGenerator { rng: rand::thread_rng() }
	}
	pub fn generate_key_pair(&mut self) -> (SecretKey, EncodedPublicKey) {
		let mut entropy = [0u8; 32];
		self.rng.fill(&mut entropy);

		let secret_key = BandersnatchVrfVerifiable::new_secret(entropy);
		let pub_key = BandersnatchVrfVerifiable::member_from_secret(&secret_key);

		(secret_key, pub_key)
	}
}
