// Copyright (C) HybirdVM.
// This file is part of HybirdVM.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.


use super::*;

use frame_support::{
	ConsensusEngineId,
	derive_impl,
	dispatch::DispatchClass,
	parameter_types,
	traits::{ConstU32, ConstU64, FindAuthor, Get},
	weights::Weight,
};
use sp_core::{crypto::{AccountId32, UncheckedFrom}, ConstBool, H256, U256};
use sp_runtime::{
	traits::{Convert, BlakeTwo256, IdentityLookup},
	BuildStorage, Perbill,
};
use hp_system::EvmHybirdVMExtension;
use frame_system::pallet_prelude::*;
use frame_support::pallet_prelude::*;
use fp_evm::Precompile;
use pallet_evm_precompile_simple::{ECRecover, Identity, Ripemd160, Sha256};
use pallet_evm::{
	    AddressMapping, BalanceOf, EnsureAddressTruncated, FeeCalculator, GasWeightMapping,
		IsPrecompileResult, PrecompileHandle, PrecompileResult, PrecompileSet,
	};
use pallet_contracts::chain_extension::SysConfig;

use crate as pallet_hybird_vm;

type Block = frame_system::mocking::MockBlock<Test>;
pub type Balance = u64;

frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		Timestamp: pallet_timestamp,
		Randomness: pallet_insecure_randomness_collective_flip,
		Evm: pallet_evm,
		Contracts: pallet_contracts,
		HybirdVM: pallet_hybird_vm,
	}
);

impl pallet_insecure_randomness_collective_flip::Config for Test {}

parameter_types! {
	pub(crate) static ExtrinsicBaseWeight: Weight = Weight::zero();
	pub(crate) static ExistentialDeposit: u64 = 0;
}

pub struct BlockWeights;
impl Get<frame_system::limits::BlockWeights> for BlockWeights {
	fn get() -> frame_system::limits::BlockWeights {
		frame_system::limits::BlockWeights::builder()
			.base_block(Weight::zero())
			.for_class(DispatchClass::all(), |weights| {
				weights.base_extrinsic = ExtrinsicBaseWeight::get().into();
			})
			.for_class(DispatchClass::non_mandatory(), |weights| {
				weights.max_total = Weight::from_parts(1024, u64::MAX).into();
			})
			.build_or_panic()
	}
}

pub type AccountId = AccountId32;

#[derive_impl(frame_system::config_preludes::ParaChainDefaultConfig as frame_system::DefaultConfig)]
impl frame_system::Config for Test {
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockWeights = BlockWeights;
	type BlockLength = ();
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type Nonce = u64;
	type RuntimeCall = RuntimeCall;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
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
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type ExistentialDeposit = ConstU64<1>;
	type ReserveIdentifier = [u8; 8];
    type AccountStore = System;
}

parameter_types! {
	pub const MinimumPeriod: u64 = 1;
}

impl pallet_timestamp::Config for Test {
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}


impl hp_system::EvmHybirdVMExtension<Test> for Test{
	fn call_hybird_vm(
		origin: OriginFor<Test>,
		data: Vec<u8>,
		target_gas: Option<u64>
		) -> Result<(Vec<u8>, u64), sp_runtime::DispatchError>
	{
		let target_weight = <Test as pallet_evm::Config>::GasWeightMapping::gas_to_weight(target_gas.unwrap_or(0), false);
		let (result_output, result_weight) = HybirdVM::call_wasm4evm(origin, data, target_weight)?;
		
		Ok((result_output, <Test as pallet_evm::Config>::GasWeightMapping::weight_to_gas(result_weight)))
	}
}

fn hash(a: u64) -> H160 {
	H160::from_low_u64_be(a)
}

pub struct MockPrecompileSet<T>(PhantomData<T>);

impl<T> PrecompileSet for MockPrecompileSet<T> 
where
     T: pallet_evm::Config + EvmHybirdVMExtension<T>,
{
	fn execute(&self, handle: &mut impl PrecompileHandle) -> Option<PrecompileResult> {
		match handle.code_address() {
			// Ethereum precompiles :
			a if a == hash(1) => Some(ECRecover::execute(handle)),
			a if a == hash(2) => Some(Sha256::execute(handle)),
			a if a == hash(3) => Some(Ripemd160::execute(handle)),
			a if a == hash(4) => Some(Identity::execute(handle)),
			a if a == hash(5) => Some(pallet_evm_precompile_call_hybird_vm::CallHybirdVM::<T>::execute(handle)),
			_ => None,
		}
	}
	
	fn is_precompile(&self, address: H160, _gas: u64) -> IsPrecompileResult {
		IsPrecompileResult::Answer {
			is_precompile: [hash(1), hash(2), hash(3), hash(4), hash(5)].contains(&address),
			extra_cost: 0,
		}
	}
}

pub struct CompactAddressMapping;

impl AddressMapping<AccountId32> for CompactAddressMapping {
	fn into_account_id(address: H160) ->  AccountId32 {	
		let mut data = [0u8; 32];
		data[0..20].copy_from_slice(&address[..]);
		AccountId32::from(data)
	}
}

pub struct FixedGasPrice;
impl FeeCalculator for FixedGasPrice {
	fn min_gas_price() -> (U256, Weight) {
		// Return some meaningful gas price and weight
		(1_000_000_000u128.into(), Weight::from_parts(7u64, 0))
	}
}

pub struct FindAuthorTruncated;
impl FindAuthor<H160> for FindAuthorTruncated {
	fn find_author<'a, I>(_digests: I) -> Option<H160>
	where
		I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
	{
		let a:[u8; 20] = [12,34,45,0,0, 0,0,0,0,0, 0,0,0,0,0, 0,0,0,0,0];
		
		Some(H160::from(a))
	}
}
const BLOCK_GAS_LIMIT: u64 = 150_000_000;
const MAX_POV_SIZE: u64 = 5 * 1024 * 1024;

parameter_types! {
	pub BlockGasLimit: U256 = U256::from(BLOCK_GAS_LIMIT);
	pub const GasLimitPovSizeRatio: u64 = BLOCK_GAS_LIMIT.saturating_div(MAX_POV_SIZE);
	pub WeightPerGas: Weight = Weight::from_parts(20_000, 0);
	pub MockPrecompiles: MockPrecompileSet<Test> = MockPrecompileSet(Default::default());
	pub SuicideQuickClearLimit: u32 = 0;
	pub const ChainId: u64 = 42;
}

impl pallet_evm::Config for Test {
	type FeeCalculator = FixedGasPrice;
	type GasWeightMapping = pallet_evm::FixedGasWeightMapping<Self>;
	type WeightPerGas = WeightPerGas;

	type BlockHashMapping = pallet_evm::SubstrateBlockHashMapping<Self>;
	type CallOrigin = EnsureAddressTruncated;

	type WithdrawOrigin = EnsureAddressTruncated;
	type AddressMapping = CompactAddressMapping;
	type Currency = Balances;

	type RuntimeEvent = RuntimeEvent;
	type PrecompilesType = MockPrecompileSet<Self>;
	type PrecompilesValue = MockPrecompiles;
	type ChainId = ChainId;
	type BlockGasLimit = BlockGasLimit;
	type Runner = pallet_evm::runner::stack::Runner<Self>;
	type OnChargeTransaction = ();
	type OnCreate = ();
	type FindAuthor = FindAuthorTruncated;
	type GasLimitPovSizeRatio = GasLimitPovSizeRatio;
	type SuicideQuickClearLimit = SuicideQuickClearLimit;
	type Timestamp = Timestamp;
	type WeightInfo = ();
}

impl Convert<Weight, BalanceOf<Self>> for Test {
	fn convert(w: Weight) -> BalanceOf<Self> {
		w.ref_time()
	}
}

#[derive(Default)]
pub struct HybirdVMChainExtension;

impl pallet_contracts::chain_extension::ChainExtension<Test> for HybirdVMChainExtension{
    fn call<E>(&mut self, env: Environment<E, InitState>) -> Result<RetVal, DispatchError>
	where
		E: Ext<T = Test>,
		<E::T as SysConfig>::AccountId: UncheckedFrom<<E::T as SysConfig>::Hash> + AsRef<[u8]>
	{
		let func_id = env.func_id();
		match func_id {
			5 => HybirdVM::call_evm4wasm::<E>(env),
			_ => Err(DispatchError::from("Passed unknown func_id to chain extension")),			
		}
	}
}

pub enum AllowBalancesCall {}

impl frame_support::traits::Contains<RuntimeCall> for AllowBalancesCall {
	fn contains(call: &RuntimeCall) -> bool {
		matches!(call, RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. }))
	}
}

// Unit = the base number of indivisible units for balances
const UNIT: Balance = 1_000_000_000_000_000_000;
const MILLIUNIT: Balance = 1_000_000_000_000_000;

const fn deposit(items: u32, bytes: u32) -> Balance {
	(items as Balance * UNIT + (bytes as Balance) * (5 * MILLIUNIT / 100)) / 10
}

fn schedule<T: pallet_contracts::Config>() -> pallet_contracts::Schedule<T> {
	pallet_contracts::Schedule {
		limits: pallet_contracts::Limits {
			runtime_memory: 1024 * 1024 * 1024,
			..Default::default()
		},
		..Default::default()
	}
}

parameter_types! {
	pub static UploadAccount: Option<<Test as frame_system::Config>::AccountId> = None;
	pub static InstantiateAccount: Option<<Test as frame_system::Config>::AccountId> = None;
}

pub struct EnsureAccount<T, A>(sp_std::marker::PhantomData<(T, A)>);
impl<T: Config, A: sp_core::Get<Option<AccountId32>>>
	EnsureOrigin<<T as frame_system::Config>::RuntimeOrigin> for EnsureAccount<T, A>
where
	<T as frame_system::Config>::AccountId: From<AccountId32>,
{
	type Success = T::AccountId;

	fn try_origin(o: T::RuntimeOrigin) -> Result<Self::Success, T::RuntimeOrigin> {
		let who = <frame_system::EnsureSigned<_> as EnsureOrigin<_>>::try_origin(o.clone())?;
		if matches!(A::get(), Some(a) if who != a.clone().into()) {
			return Err(o)
		}

		Ok(who)
	}

	#[cfg(feature = "runtime-benchmarks")]
	fn try_successful_origin() -> Result<T::RuntimeOrigin, ()> {
		Err(())
	}
}

parameter_types! {
	pub const DepositPerItem: Balance = deposit(1, 0);
	pub const DepositPerByte: Balance = deposit(0, 1);
	pub Schedule: pallet_contracts::Schedule<Test> = schedule::<Test>();
	pub const DefaultDepositLimit: Balance = deposit(1024, 1024 * 1024);
	pub const CodeHashLockupDepositPercent: Perbill = Perbill::from_percent(0);
	pub const MaxDelegateDependencies: u32 = 32;
}

#[derive_impl(pallet_contracts::config_preludes::TestDefaultConfig)]
impl pallet_contracts::Config for Test {
	type Time = Timestamp;
	type Randomness = Randomness;
	type Currency = Balances;
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type CallFilter = AllowBalancesCall;
	type DepositPerItem = DepositPerItem;
	type DepositPerByte = DepositPerByte;
	type CallStack = [pallet_contracts::Frame<Self>; 23];
	type WeightInfo = pallet_contracts::weights::SubstrateWeight<Self>;
	type ChainExtension = HybirdVMChainExtension;
	type Schedule = Schedule;
	type AddressGenerator = pallet_contracts::DefaultAddressGenerator;
	type MaxCodeLen = ConstU32<{ 128 * 1024 }>;
	type DefaultDepositLimit = DefaultDepositLimit;
	type MaxStorageKeyLen = ConstU32<128>;
	type MaxDebugBufferLen = ConstU32<{ 2 * 1024 * 1024 }>;
	type UnsafeUnstableInterface = ConstBool<false>;
	type CodeHashLockupDepositPercent = CodeHashLockupDepositPercent;
	type MaxDelegateDependencies = MaxDelegateDependencies;
	type RuntimeHoldReason = RuntimeHoldReason;
	type UploadOrigin = EnsureAccount<Self, UploadAccount>;
	type InstantiateOrigin = EnsureAccount<Self, InstantiateAccount>;	
	type Environment = ();
	type Debug = ();
	type Migrations = ();
	type Xcm = ();
	
}


parameter_types! {
	pub const EnableCallEVM: bool = true;
	pub const EnableCallWasmVM: bool = true;
}

impl pallet_hybird_vm::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type EnableCallEVM = EnableCallEVM;
	type EnableCallWasmVM = EnableCallWasmVM;
}

const A:[u8; 32] = [1,1,1,1,1, 2,2,2,2,2, 3,3,3,3,3, 4,4,4,4,4, 0,0,0,0,0, 0,0,0,0,0, 0,0];
const B:[u8; 32] = [2,2,2,2,2, 3,3,3,3,3, 4,4,4,4,4, 5,5,5,5,5, 0,0,0,0,0, 0,0,0,0,0, 0,0];
pub const ALICE: AccountId32 = AccountId32::new(A);
pub const BOB: AccountId32 = AccountId32::new(B);

pub struct ExtBuilder {
	existential_deposit: u64,
}

impl Default for ExtBuilder {
	fn default() -> Self {
		Self { existential_deposit: 1 }
	}
}

impl ExtBuilder {
	pub fn existential_deposit(mut self, existential_deposit: u64) -> Self {
		self.existential_deposit = existential_deposit;
		self
	}
	pub fn set_associated_consts(&self) {
		EXISTENTIAL_DEPOSIT.with(|v| *v.borrow_mut() = self.existential_deposit);
	}
	pub fn build(self) -> sp_io::TestExternalities {
		self.set_associated_consts();
		let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
		pallet_balances::GenesisConfig::<Test> { balances: vec![] }
			.assimilate_storage(&mut t)
			.unwrap();
		pallet_evm::GenesisConfig::<Test> {
			accounts: std::collections::BTreeMap::new(),
			_marker: PhantomData,
		}.assimilate_storage(&mut t).unwrap();			
		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}
}

pub(crate) fn last_event() -> RuntimeEvent {
	frame_system::Pallet::<Test>::events().pop().expect("Event expected").event
}

pub(crate) fn expect_event<E: Into<RuntimeEvent>>(e: E) {
	assert_eq!(last_event(), e.into());
}
