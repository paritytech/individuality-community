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

//! Benchmarks for the assets forwarder.

use super::*;

use frame_benchmarking::v2::{instance_benchmarks, *};
use frame_support::traits::fungible::{InspectHold, Mutate};
use frame_system::RawOrigin;
use pallet_assets::BenchmarkHelper as AssetsBenchmarkHelper;
use sp_runtime::traits::{Saturating, StaticLookup, Zero};

/// What the benchmarks cannot set up themselves, because only the runtime knows how its XCM
/// channels are made.
pub trait BenchmarkHelper {
	/// Opens the channel to the destination chain so the router can deliver messages.
	fn open_destination_channel();
}

impl BenchmarkHelper for () {
	fn open_destination_channel() {}
}

/// Creates a live asset and returns its parameter form, with `caller` funded to pay the forward
/// deposit and the delivery fees.
fn setup_asset<T: Config<I>, I: 'static>(
	caller: &T::AccountId,
) -> Result<T::AssetIdParameter, BenchmarkError> {
	<T as Config<I>>::BenchmarkHelper::open_destination_channel();
	let id = <T as pallet_assets::Config<I>>::BenchmarkHelper::create_asset_id_parameter(1);
	let balance = <T as Config<I>>::Currency::minimum_balance()
		.saturating_add(T::ForwardDeposit::get())
		.saturating_mul(1_000_000u32.into());
	<T as Config<I>>::Currency::set_balance(caller, balance);
	pallet_assets::Pallet::<T, I>::force_create(
		RawOrigin::Root.into(),
		id.clone(),
		T::Lookup::unlookup(caller.clone()),
		true,
		1u32.into(),
	)
	.map_err(|_| BenchmarkError::Stop("failed to create asset"))?;
	Ok(id)
}

#[instance_benchmarks]
mod benches {
	use super::*;

	#[benchmark]
	fn forward_asset() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		let id = setup_asset::<T, I>(&caller)?;

		#[extrinsic_call]
		_(RawOrigin::Signed(caller), id.clone());

		let asset_id: T::AssetId = id.into();
		assert!(ForwardedAssets::<T, I>::contains_key(asset_id));
		Ok(())
	}

	#[benchmark]
	fn sync_asset_status() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		let id = setup_asset::<T, I>(&caller)?;
		Pallet::<T, I>::forward_asset(RawOrigin::Signed(caller.clone()).into(), id.clone())
			.map_err(|_| BenchmarkError::Stop("failed to forward asset"))?;
		// Change the status locally, otherwise the sync is rejected as a no-op.
		let team = T::Lookup::unlookup(caller.clone());
		pallet_assets::Pallet::<T, I>::force_asset_status(
			RawOrigin::Root.into(),
			id.clone(),
			team.clone(),
			team.clone(),
			team.clone(),
			team,
			2u32.into(),
			true,
			false,
		)
		.map_err(|_| BenchmarkError::Stop("failed to change asset status"))?;

		#[extrinsic_call]
		_(RawOrigin::Signed(caller), id.clone());

		let asset_id: T::AssetId = id.into();
		let record = ForwardedAssets::<T, I>::get(asset_id).expect("asset is forwarded");
		assert_eq!(record.min_balance, 2u32.into());
		assert!(record.is_sufficient);
		Ok(())
	}

	#[benchmark]
	fn remove_forwarded_asset() -> Result<(), BenchmarkError> {
		let caller: T::AccountId = whitelisted_caller();
		let id = setup_asset::<T, I>(&caller)?;
		Pallet::<T, I>::forward_asset(RawOrigin::Signed(caller.clone()).into(), id.clone())
			.map_err(|_| BenchmarkError::Stop("failed to forward asset"))?;
		let origin =
			T::ManagerOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;

		#[extrinsic_call]
		_(origin as T::RuntimeOrigin, id.clone());

		let asset_id: T::AssetId = id.into();
		assert!(!ForwardedAssets::<T, I>::contains_key(asset_id));
		assert!(<T as Config<I>>::Currency::balance_on_hold(
			&HoldReason::<I>::ForwardDeposit.into(),
			&caller
		)
		.is_zero());
		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
