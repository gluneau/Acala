// This file is part of Acala.

// Copyright (C) 2020-2021 Acala Foundation.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use acala_service::chain_spec::evm_genesis;
pub use codec::Encode;
use cumulus_test_relay_sproof_builder::RelayStateSproofBuilder;
use frame_support::{
	assert_noop, assert_ok,
	traits::{schedule::DispatchTime, Currency, GenesisBuild, OnFinalize, OnInitialize, OriginTrait, ValidatorSet},
	weights::constants::*,
};
use frame_system::RawOrigin;

use module_cdp_engine::LiquidationStrategy;
pub use module_support::{
	mocks::MockAddressMapping, AddressMapping, CDPTreasury, DEXManager, Price, Rate, Ratio, RiskManager,
};
use orml_authority::DelayedOrigin;
pub use orml_traits::{Change, GetByKey, MultiCurrency};
use orml_vesting::VestingSchedule;
pub use primitives::currency::*;
pub use sp_core::H160;
use sp_io::hashing::keccak_256;
pub use sp_runtime::{
	traits::{AccountIdConversion, BadOrigin, Convert, Zero},
	DispatchError, DispatchResult, FixedPointNumber, MultiAddress,
};

use xcm::{
	opaque::v0::prelude::{BuyExecution, DepositAsset},
	v0::{
		ExecuteXcm,
		Junction::{self, *},
		MultiAsset,
		MultiLocation::*,
		Outcome, Xcm,
	},
};

#[cfg(feature = "with-mandala-runtime")]
pub use mandala_imports::*;
#[cfg(feature = "with-mandala-runtime")]
mod mandala_imports {
	pub use mandala_runtime::{
		create_x2_parachain_multilocation, get_all_module_accounts, AcalaOracle, AccountId, AuctionManager, Authority,
		AuthoritysOriginId, Balance, Balances, BlockNumber, Call, CdpEngine, CdpTreasury, CreateClassDeposit,
		CreateTokenDeposit, Currencies, CurrencyId, CurrencyIdConvert, DataDepositPerByte, Dex, EmergencyShutdown,
		EnabledTradingPairs, Event, EvmAccounts, ExistentialDeposits, Get, GetNativeCurrencyId, HomaLite, Loans,
		MultiLocation, NativeTokenExistentialDeposit, NetworkId, NftPalletId, OneDay, Origin, OriginCaller,
		ParachainInfo, ParachainSystem, Perbill, Proxy, RelaychainSovereignSubAccount, Runtime, Scheduler, Session,
		SessionManager, SevenDays, System, TokenSymbol, Tokens, TreasuryAccount, TreasuryPalletId, Utility, Vesting,
		XcmConfig, XcmExecutor, NFT,
	};

	pub use runtime_common::{dollar, ACA, AUSD, DOT, LDOT};
	pub const NATIVE_CURRENCY: CurrencyId = ACA;
	pub const LIQUID_CURRENCY: CurrencyId = LDOT;
	pub const RELAY_CHAIN_CURRENCY: CurrencyId = DOT;
	pub const USD_CURRENCY: CurrencyId = AUSD;
	pub const LPTOKEN: CurrencyId = CurrencyId::DexShare(
		primitives::DexShare::Token(TokenSymbol::AUSD),
		primitives::DexShare::Token(TokenSymbol::DOT),
	);
}

#[cfg(feature = "with-karura-runtime")]
pub use karura_imports::*;
#[cfg(feature = "with-karura-runtime")]
mod karura_imports {
	pub use frame_support::parameter_types;
	pub use karura_runtime::{
		create_x2_parachain_multilocation, get_all_module_accounts, AcalaOracle, AccountId, AuctionManager, Authority,
		AuthoritysOriginId, Balance, Balances, BlockNumber, Call, CdpEngine, CdpTreasury, CreateClassDeposit,
		CreateTokenDeposit, Currencies, CurrencyId, CurrencyIdConvert, DataDepositPerByte, Dex, EmergencyShutdown,
		Event, EvmAccounts, ExistentialDeposits, Get, GetNativeCurrencyId, HomaLite, KaruraFoundationAccounts, Loans,
		MultiLocation, NativeTokenExistentialDeposit, NetworkId, NftPalletId, OneDay, Origin, OriginCaller,
		ParachainInfo, ParachainSystem, Perbill, Proxy, RelaychainSovereignSubAccount, Runtime, Scheduler, Session,
		SessionManager, SevenDays, System, TokenSymbol, Tokens, TreasuryPalletId, Utility, Vesting, XTokens, XcmConfig,
		XcmExecutor, NFT,
	};
	pub use primitives::TradingPair;
	pub use runtime_common::{dollar, KAR, KSM, KUSD, LKSM};
	pub use sp_runtime::traits::AccountIdConversion;

	parameter_types! {
		pub EnabledTradingPairs: Vec<TradingPair> = vec![
			TradingPair::from_currency_ids(USD_CURRENCY, NATIVE_CURRENCY).unwrap(),
			TradingPair::from_currency_ids(USD_CURRENCY, RELAY_CHAIN_CURRENCY).unwrap(),
			TradingPair::from_currency_ids(USD_CURRENCY, LIQUID_CURRENCY).unwrap(),
		];
		pub TreasuryAccount: AccountId = TreasuryPalletId::get().into_account();
	}

	pub const NATIVE_CURRENCY: CurrencyId = KAR;
	pub const LIQUID_CURRENCY: CurrencyId = LKSM;
	pub const RELAY_CHAIN_CURRENCY: CurrencyId = KSM;
	pub const USD_CURRENCY: CurrencyId = KUSD;
	pub const LPTOKEN: CurrencyId = CurrencyId::DexShare(
		primitives::DexShare::Token(TokenSymbol::KUSD),
		primitives::DexShare::Token(TokenSymbol::KSM),
	);
}

const ORACLE1: [u8; 32] = [0u8; 32];
const ORACLE2: [u8; 32] = [1u8; 32];
const ORACLE3: [u8; 32] = [2u8; 32];
const ORACLE4: [u8; 32] = [3u8; 32];
const ORACLE5: [u8; 32] = [4u8; 32];

pub const ALICE: [u8; 32] = [4u8; 32];
pub const BOB: [u8; 32] = [5u8; 32];

fn run_to_block(n: u32) {
	while System::block_number() < n {
		Scheduler::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		Scheduler::on_initialize(System::block_number());
		Scheduler::on_initialize(System::block_number());
		Session::on_initialize(System::block_number());
		SessionManager::on_initialize(System::block_number());
	}
}

fn set_relaychain_block_number(number: BlockNumber) {
	ParachainSystem::on_initialize(number);

	let (relay_storage_root, proof) = RelayStateSproofBuilder::default().into_state_root_and_proof();

	assert_ok!(ParachainSystem::set_validation_data(
		Origin::none(),
		cumulus_primitives_parachain_inherent::ParachainInherentData {
			validation_data: cumulus_primitives_core::PersistedValidationData {
				parent_head: Default::default(),
				relay_parent_number: number,
				relay_parent_storage_root: relay_storage_root,
				max_pov_size: Default::default(),
			},
			relay_chain_state: proof,
			downward_messages: Default::default(),
			horizontal_messages: Default::default(),
		}
	));
}

pub struct ExtBuilder {
	balances: Vec<(AccountId, CurrencyId, Balance)>,
}

impl Default for ExtBuilder {
	fn default() -> Self {
		Self { balances: vec![] }
	}
}

impl ExtBuilder {
	pub fn balances(mut self, balances: Vec<(AccountId, CurrencyId, Balance)>) -> Self {
		self.balances = balances;
		self
	}

	pub fn build(self) -> sp_io::TestExternalities {
		let evm_genesis_accounts = evm_genesis();

		let mut t = frame_system::GenesisConfig::default()
			.build_storage::<Runtime>()
			.unwrap();

		let native_currency_id = GetNativeCurrencyId::get();
		let existential_deposit = NativeTokenExistentialDeposit::get();
		let initial_enabled_trading_pairs = EnabledTradingPairs::get();

		module_dex::GenesisConfig::<Runtime> {
			initial_enabled_trading_pairs: initial_enabled_trading_pairs,
			initial_listing_trading_pairs: Default::default(),
			initial_added_liquidity_pools: vec![],
		}
		.assimilate_storage(&mut t)
		.unwrap();

		pallet_balances::GenesisConfig::<Runtime> {
			balances: self
				.balances
				.clone()
				.into_iter()
				.filter(|(_, currency_id, _)| *currency_id == native_currency_id)
				.map(|(account_id, _, initial_balance)| (account_id, initial_balance))
				.chain(
					get_all_module_accounts()
						.iter()
						.map(|x| (x.clone(), existential_deposit)),
				)
				.collect::<Vec<_>>(),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		orml_tokens::GenesisConfig::<Runtime> {
			balances: self
				.balances
				.into_iter()
				.filter(|(_, currency_id, _)| *currency_id != native_currency_id)
				.collect::<Vec<_>>(),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		pallet_membership::GenesisConfig::<Runtime, pallet_membership::Instance5> {
			members: vec![
				AccountId::from(ORACLE1),
				AccountId::from(ORACLE2),
				AccountId::from(ORACLE3),
				AccountId::from(ORACLE4),
				AccountId::from(ORACLE5),
			],
			phantom: Default::default(),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		module_evm::GenesisConfig::<Runtime> {
			accounts: evm_genesis_accounts,
			treasury: Default::default(),
		}
		.assimilate_storage(&mut t)
		.unwrap();

		module_session_manager::GenesisConfig::<Runtime> { session_duration: 10 }
			.assimilate_storage(&mut t)
			.unwrap();

		<parachain_info::GenesisConfig as GenesisBuild<Runtime>>::assimilate_storage(
			&parachain_info::GenesisConfig {
				parachain_id: 2000.into(),
			},
			&mut t,
		)
		.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}
}

fn set_oracle_price(prices: Vec<(CurrencyId, Price)>) -> DispatchResult {
	AcalaOracle::on_finalize(0);
	assert_ok!(AcalaOracle::feed_values(
		Origin::signed(AccountId::from(ORACLE1)),
		prices.clone(),
	));
	assert_ok!(AcalaOracle::feed_values(
		Origin::signed(AccountId::from(ORACLE2)),
		prices.clone(),
	));
	assert_ok!(AcalaOracle::feed_values(
		Origin::signed(AccountId::from(ORACLE3)),
		prices.clone(),
	));
	assert_ok!(AcalaOracle::feed_values(
		Origin::signed(AccountId::from(ORACLE4)),
		prices.clone(),
	));
	assert_ok!(AcalaOracle::feed_values(
		Origin::signed(AccountId::from(ORACLE5)),
		prices,
	));
	Ok(())
}

pub fn alice_key() -> secp256k1::SecretKey {
	secp256k1::SecretKey::parse(&keccak_256(b"Alice")).unwrap()
}

pub fn bob_key() -> secp256k1::SecretKey {
	secp256k1::SecretKey::parse(&keccak_256(b"Bob")).unwrap()
}

pub fn alice() -> AccountId {
	let address = EvmAccounts::eth_address(&alice_key());
	let mut data = [0u8; 32];
	data[0..4].copy_from_slice(b"evm:");
	data[4..24].copy_from_slice(&address[..]);
	AccountId::from(Into::<[u8; 32]>::into(data))
}

pub fn bob() -> AccountId {
	let address = EvmAccounts::eth_address(&bob_key());
	let mut data = [0u8; 32];
	data[0..4].copy_from_slice(b"evm:");
	data[4..24].copy_from_slice(&address[..]);
	AccountId::from(Into::<[u8; 32]>::into(data))
}

#[test]
fn emergency_shutdown_and_cdp_treasury() {
	ExtBuilder::default()
		.balances(vec![
			(AccountId::from(ALICE), USD_CURRENCY, 2_000_000 * dollar(USD_CURRENCY)),
			(AccountId::from(BOB), USD_CURRENCY, 8_000_000 * dollar(USD_CURRENCY)),
			(
				AccountId::from(BOB),
				RELAY_CHAIN_CURRENCY,
				300_000_000 * dollar(RELAY_CHAIN_CURRENCY),
			),
			(
				AccountId::from(BOB),
				LIQUID_CURRENCY,
				50_000_000 * dollar(LIQUID_CURRENCY),
			),
		])
		.build()
		.execute_with(|| {
			assert_ok!(CdpTreasury::deposit_collateral(
				&AccountId::from(BOB),
				RELAY_CHAIN_CURRENCY,
				200_000_000 * dollar(RELAY_CHAIN_CURRENCY)
			));
			assert_ok!(CdpTreasury::deposit_collateral(
				&AccountId::from(BOB),
				LIQUID_CURRENCY,
				40_000_000 * dollar(LIQUID_CURRENCY)
			));
			assert_eq!(
				CdpTreasury::total_collaterals(RELAY_CHAIN_CURRENCY),
				200_000_000 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(
				CdpTreasury::total_collaterals(LIQUID_CURRENCY),
				40_000_000 * dollar(LIQUID_CURRENCY)
			);

			// Total liquidity to collaterize is calculated using Stable currency - USD
			assert_noop!(
				EmergencyShutdown::refund_collaterals(
					Origin::signed(AccountId::from(ALICE)),
					1_000_000 * dollar(USD_CURRENCY)
				),
				module_emergency_shutdown::Error::<Runtime>::CanNotRefund,
			);
			assert_ok!(EmergencyShutdown::emergency_shutdown(Origin::root()));
			assert_ok!(EmergencyShutdown::open_collateral_refund(Origin::root()));
			assert_ok!(EmergencyShutdown::refund_collaterals(
				Origin::signed(AccountId::from(ALICE)),
				1_000_000 * dollar(USD_CURRENCY)
			));

			assert_eq!(
				CdpTreasury::total_collaterals(RELAY_CHAIN_CURRENCY),
				180_000_000 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(
				CdpTreasury::total_collaterals(LIQUID_CURRENCY),
				36_000_000 * dollar(LIQUID_CURRENCY)
			);
			assert_eq!(
				Currencies::free_balance(USD_CURRENCY, &AccountId::from(ALICE)),
				1_000_000 * dollar(USD_CURRENCY)
			);
			assert_eq!(
				Currencies::free_balance(RELAY_CHAIN_CURRENCY, &AccountId::from(ALICE)),
				20_000_000 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(
				Currencies::free_balance(LIQUID_CURRENCY, &AccountId::from(ALICE)),
				4_000_000 * dollar(LIQUID_CURRENCY)
			);
		});
}

#[test]
fn liquidate_cdp() {
	ExtBuilder::default()
		.balances(vec![
			(
				AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				51 * dollar(RELAY_CHAIN_CURRENCY),
			),
			(AccountId::from(BOB), USD_CURRENCY, 1_000_001 * dollar(USD_CURRENCY)),
			(
				AccountId::from(BOB),
				RELAY_CHAIN_CURRENCY,
				102 * dollar(RELAY_CHAIN_CURRENCY),
			),
		])
		.build()
		.execute_with(|| {
			assert_ok!(set_oracle_price(vec![(
				RELAY_CHAIN_CURRENCY,
				Price::saturating_from_rational(10000, 1)
			)])); // 10000 usd

			assert_ok!(Dex::add_liquidity(
				Origin::signed(AccountId::from(BOB)),
				RELAY_CHAIN_CURRENCY,
				USD_CURRENCY,
				100 * dollar(RELAY_CHAIN_CURRENCY),
				1_000_000 * dollar(USD_CURRENCY),
				0,
				false,
			));

			assert_ok!(CdpEngine::set_collateral_params(
				Origin::root(),
				RELAY_CHAIN_CURRENCY,
				Change::NewValue(Some(Rate::zero())),
				Change::NewValue(Some(Ratio::saturating_from_rational(200, 100))),
				Change::NewValue(Some(Rate::saturating_from_rational(20, 100))),
				Change::NewValue(Some(Ratio::saturating_from_rational(200, 100))),
				Change::NewValue(1_000_000 * dollar(USD_CURRENCY)),
			));

			assert_ok!(CdpEngine::adjust_position(
				&AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				(50 * dollar(RELAY_CHAIN_CURRENCY)) as i128,
				(2_500_000 * dollar(USD_CURRENCY)) as i128,
			));

			assert_ok!(CdpEngine::adjust_position(
				&AccountId::from(BOB),
				RELAY_CHAIN_CURRENCY,
				dollar(RELAY_CHAIN_CURRENCY) as i128,
				(50_000 * dollar(USD_CURRENCY)) as i128,
			));

			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).debit,
				2_500_000 * dollar(USD_CURRENCY)
			);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).collateral,
				50 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(BOB)).debit,
				50_000 * dollar(USD_CURRENCY)
			);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(BOB)).collateral,
				dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(CdpTreasury::debit_pool(), 0);
			assert_eq!(AuctionManager::collateral_auctions(0), None);

			assert_ok!(CdpEngine::set_collateral_params(
				Origin::root(),
				RELAY_CHAIN_CURRENCY,
				Change::NoChange,
				Change::NewValue(Some(Ratio::saturating_from_rational(400, 100))),
				Change::NoChange,
				Change::NewValue(Some(Ratio::saturating_from_rational(400, 100))),
				Change::NoChange,
			));

			assert_ok!(CdpEngine::liquidate_unsafe_cdp(
				AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY
			));

			let liquidate_alice_xbtc_cdp_event = Event::CdpEngine(module_cdp_engine::Event::LiquidateUnsafeCDP(
				RELAY_CHAIN_CURRENCY,
				AccountId::from(ALICE),
				50 * dollar(RELAY_CHAIN_CURRENCY),
				250_000 * dollar(USD_CURRENCY),
				LiquidationStrategy::Auction,
			));

			assert!(System::events()
				.iter()
				.any(|record| record.event == liquidate_alice_xbtc_cdp_event));

			assert_eq!(Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).debit, 0);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).collateral,
				0
			);
			assert!(AuctionManager::collateral_auctions(0).is_some());
			assert_eq!(CdpTreasury::debit_pool(), 250_000 * dollar(USD_CURRENCY));

			assert_ok!(CdpEngine::liquidate_unsafe_cdp(
				AccountId::from(BOB),
				RELAY_CHAIN_CURRENCY
			));

			let liquidate_bob_xbtc_cdp_event = Event::CdpEngine(module_cdp_engine::Event::LiquidateUnsafeCDP(
				RELAY_CHAIN_CURRENCY,
				AccountId::from(BOB),
				dollar(RELAY_CHAIN_CURRENCY),
				5_000 * dollar(USD_CURRENCY),
				LiquidationStrategy::Exchange,
			));
			assert!(System::events()
				.iter()
				.any(|record| record.event == liquidate_bob_xbtc_cdp_event));

			assert_eq!(Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(BOB)).debit, 0);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(BOB)).collateral,
				0
			);
			assert_eq!(CdpTreasury::debit_pool(), 255_000 * dollar(USD_CURRENCY));
			assert!(CdpTreasury::surplus_pool() >= 5_000 * dollar(USD_CURRENCY));
		});
}

#[test]
fn test_dex_module() {
	ExtBuilder::default()
		.balances(vec![
			(
				// NetworkContractSource
				MockAddressMapping::get_account_id(&H160::from_low_u64_be(0)),
				NATIVE_CURRENCY,
				1_000_000_000 * dollar(NATIVE_CURRENCY),
			),
			(
				AccountId::from(ALICE),
				USD_CURRENCY,
				1_000_000_000 * dollar(NATIVE_CURRENCY),
			),
			(
				AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				1_000_000_000 * dollar(NATIVE_CURRENCY),
			),
			(AccountId::from(BOB), USD_CURRENCY, 1_000_000 * dollar(NATIVE_CURRENCY)),
			(
				AccountId::from(BOB),
				RELAY_CHAIN_CURRENCY,
				1_000_000_000 * dollar(NATIVE_CURRENCY),
			),
		])
		.build()
		.execute_with(|| {
			assert_eq!(Dex::get_liquidity_pool(RELAY_CHAIN_CURRENCY, USD_CURRENCY), (0, 0));
			assert_eq!(Currencies::total_issuance(LPTOKEN), 0);
			assert_eq!(Currencies::free_balance(LPTOKEN, &AccountId::from(ALICE)), 0);

			assert_noop!(
				Dex::add_liquidity(
					Origin::signed(AccountId::from(ALICE)),
					RELAY_CHAIN_CURRENCY,
					USD_CURRENCY,
					0,
					10_000_000 * dollar(USD_CURRENCY),
					0,
					false,
				),
				module_dex::Error::<Runtime>::InvalidLiquidityIncrement,
			);

			assert_ok!(Dex::add_liquidity(
				Origin::signed(AccountId::from(ALICE)),
				RELAY_CHAIN_CURRENCY,
				USD_CURRENCY,
				10_000 * dollar(RELAY_CHAIN_CURRENCY),
				10_000_000 * dollar(USD_CURRENCY),
				0,
				false,
			));

			let add_liquidity_event = Event::Dex(module_dex::Event::AddLiquidity(
				AccountId::from(ALICE),
				USD_CURRENCY,
				10_000_000 * dollar(USD_CURRENCY),
				RELAY_CHAIN_CURRENCY,
				10_000 * dollar(RELAY_CHAIN_CURRENCY),
				20_000_000 * dollar(USD_CURRENCY),
			));
			assert!(System::events()
				.iter()
				.any(|record| record.event == add_liquidity_event));

			assert_eq!(
				Dex::get_liquidity_pool(RELAY_CHAIN_CURRENCY, USD_CURRENCY),
				(10_000 * dollar(RELAY_CHAIN_CURRENCY), 10_000_000 * dollar(USD_CURRENCY))
			);
			assert_eq!(Currencies::total_issuance(LPTOKEN), 20_000_000 * dollar(USD_CURRENCY));
			assert_eq!(
				Currencies::free_balance(LPTOKEN, &AccountId::from(ALICE)),
				20_000_000 * dollar(USD_CURRENCY)
			);
			assert_ok!(Dex::add_liquidity(
				Origin::signed(AccountId::from(BOB)),
				RELAY_CHAIN_CURRENCY,
				USD_CURRENCY,
				1 * dollar(RELAY_CHAIN_CURRENCY),
				1_000 * dollar(USD_CURRENCY),
				0,
				false,
			));
			assert_eq!(
				Dex::get_liquidity_pool(RELAY_CHAIN_CURRENCY, USD_CURRENCY),
				(10_001 * dollar(RELAY_CHAIN_CURRENCY), 10_001_000 * dollar(USD_CURRENCY))
			);
			assert_eq!(Currencies::total_issuance(LPTOKEN), 20_002_000 * dollar(USD_CURRENCY));
			assert_eq!(
				Currencies::free_balance(LPTOKEN, &AccountId::from(BOB)),
				2000 * dollar(USD_CURRENCY)
			);
			assert_noop!(
				Dex::add_liquidity(
					Origin::signed(AccountId::from(BOB)),
					RELAY_CHAIN_CURRENCY,
					USD_CURRENCY,
					1,
					999,
					0,
					false,
				),
				module_dex::Error::<Runtime>::InvalidLiquidityIncrement,
			);
			assert_eq!(
				Dex::get_liquidity_pool(RELAY_CHAIN_CURRENCY, USD_CURRENCY),
				(10_001 * dollar(RELAY_CHAIN_CURRENCY), 10_001_000 * dollar(USD_CURRENCY))
			);
			assert_eq!(Currencies::total_issuance(LPTOKEN), 20_002_000 * dollar(USD_CURRENCY));
			assert_eq!(
				Currencies::free_balance(LPTOKEN, &AccountId::from(BOB)),
				2_000 * dollar(USD_CURRENCY)
			);
			assert_ok!(Dex::add_liquidity(
				Origin::signed(AccountId::from(BOB)),
				RELAY_CHAIN_CURRENCY,
				USD_CURRENCY,
				2 * dollar(RELAY_CHAIN_CURRENCY),
				1_000 * dollar(USD_CURRENCY),
				0,
				false,
			));
			assert_eq!(
				Dex::get_liquidity_pool(RELAY_CHAIN_CURRENCY, USD_CURRENCY),
				(10_002 * dollar(RELAY_CHAIN_CURRENCY), 10_002_000 * dollar(USD_CURRENCY))
			);
			assert_ok!(Dex::add_liquidity(
				Origin::signed(AccountId::from(BOB)),
				RELAY_CHAIN_CURRENCY,
				USD_CURRENCY,
				1 * dollar(RELAY_CHAIN_CURRENCY),
				1_001 * dollar(USD_CURRENCY),
				0,
				false,
			));
			assert_eq!(
				Dex::get_liquidity_pool(RELAY_CHAIN_CURRENCY, USD_CURRENCY),
				(10_003 * dollar(RELAY_CHAIN_CURRENCY), 10_003_000 * dollar(USD_CURRENCY))
			);

			assert_eq!(Currencies::total_issuance(LPTOKEN), 20_005_999_999_999_999_995);
		});
}

#[test]
fn test_honzon_module() {
	ExtBuilder::default()
		.balances(vec![(
			AccountId::from(ALICE),
			RELAY_CHAIN_CURRENCY,
			1_000 * dollar(RELAY_CHAIN_CURRENCY),
		)])
		.build()
		.execute_with(|| {
			assert_ok!(set_oracle_price(vec![(
				RELAY_CHAIN_CURRENCY,
				Price::saturating_from_rational(1, 1)
			)]));

			assert_ok!(CdpEngine::set_collateral_params(
				Origin::root(),
				RELAY_CHAIN_CURRENCY,
				Change::NewValue(Some(Rate::saturating_from_rational(1, 100000))),
				Change::NewValue(Some(Ratio::saturating_from_rational(3, 2))),
				Change::NewValue(Some(Rate::saturating_from_rational(2, 10))),
				Change::NewValue(Some(Ratio::saturating_from_rational(9, 5))),
				Change::NewValue(10_000 * dollar(USD_CURRENCY)),
			));
			assert_ok!(CdpEngine::adjust_position(
				&AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				(100 * dollar(RELAY_CHAIN_CURRENCY)) as i128,
				(500 * dollar(USD_CURRENCY)) as i128
			));
			assert_eq!(
				Currencies::free_balance(RELAY_CHAIN_CURRENCY, &AccountId::from(ALICE)),
				900 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(
				Currencies::free_balance(USD_CURRENCY, &AccountId::from(ALICE)),
				50 * dollar(USD_CURRENCY)
			);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).debit,
				500 * dollar(USD_CURRENCY)
			);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).collateral,
				100 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(
				CdpEngine::liquidate(
					Origin::none(),
					RELAY_CHAIN_CURRENCY,
					MultiAddress::Id(AccountId::from(ALICE))
				)
				.is_ok(),
				false
			);
			assert_ok!(CdpEngine::set_collateral_params(
				Origin::root(),
				RELAY_CHAIN_CURRENCY,
				Change::NoChange,
				Change::NewValue(Some(Ratio::saturating_from_rational(3, 1))),
				Change::NoChange,
				Change::NoChange,
				Change::NoChange,
			));
			assert_ok!(CdpEngine::liquidate(
				Origin::none(),
				RELAY_CHAIN_CURRENCY,
				MultiAddress::Id(AccountId::from(ALICE))
			));

			assert_eq!(
				Currencies::free_balance(RELAY_CHAIN_CURRENCY, &AccountId::from(ALICE)),
				900 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(
				Currencies::free_balance(USD_CURRENCY, &AccountId::from(ALICE)),
				50 * dollar(USD_CURRENCY)
			);
			assert_eq!(Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).debit, 0);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).collateral,
				0
			);
		});
}

#[test]
fn test_cdp_engine_module() {
	ExtBuilder::default()
		.balances(vec![
			(AccountId::from(ALICE), USD_CURRENCY, 2_000 * dollar(USD_CURRENCY)),
			(
				AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				2_000 * dollar(RELAY_CHAIN_CURRENCY),
			),
		])
		.build()
		.execute_with(|| {
			assert_ok!(CdpEngine::set_collateral_params(
				Origin::root(),
				RELAY_CHAIN_CURRENCY,
				Change::NewValue(Some(Rate::saturating_from_rational(1, 100000))),
				Change::NewValue(Some(Ratio::saturating_from_rational(3, 2))),
				Change::NewValue(Some(Rate::saturating_from_rational(2, 10))),
				Change::NewValue(Some(Ratio::saturating_from_rational(9, 5))),
				Change::NewValue(10_000 * dollar(USD_CURRENCY)),
			));

			let new_collateral_params = CdpEngine::collateral_params(RELAY_CHAIN_CURRENCY);

			assert_eq!(
				new_collateral_params.interest_rate_per_sec,
				Some(Rate::saturating_from_rational(1, 100000))
			);
			assert_eq!(
				new_collateral_params.liquidation_ratio,
				Some(Ratio::saturating_from_rational(3, 2))
			);
			assert_eq!(
				new_collateral_params.liquidation_penalty,
				Some(Rate::saturating_from_rational(2, 10))
			);
			assert_eq!(
				new_collateral_params.required_collateral_ratio,
				Some(Ratio::saturating_from_rational(9, 5))
			);
			assert_eq!(
				new_collateral_params.maximum_total_debit_value,
				10_000 * dollar(USD_CURRENCY)
			);

			assert_eq!(
				CdpEngine::calculate_collateral_ratio(
					RELAY_CHAIN_CURRENCY,
					100 * dollar(RELAY_CHAIN_CURRENCY),
					50 * dollar(USD_CURRENCY),
					Price::saturating_from_rational(1 * dollar(USD_CURRENCY), dollar(RELAY_CHAIN_CURRENCY)),
				),
				Ratio::saturating_from_rational(100 * 10, 50)
			);

			assert_ok!(CdpEngine::check_debit_cap(
				RELAY_CHAIN_CURRENCY,
				99_999 * dollar(USD_CURRENCY)
			));
			assert_eq!(
				CdpEngine::check_debit_cap(RELAY_CHAIN_CURRENCY, 100_001 * dollar(USD_CURRENCY)).is_ok(),
				false
			);

			assert_ok!(CdpEngine::adjust_position(
				&AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				(200 * dollar(RELAY_CHAIN_CURRENCY)) as i128,
				0
			));
			assert_eq!(
				Currencies::free_balance(RELAY_CHAIN_CURRENCY, &AccountId::from(ALICE)),
				1800 * dollar(RELAY_CHAIN_CURRENCY)
			);
			assert_eq!(Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).debit, 0);
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).collateral,
				200 * dollar(RELAY_CHAIN_CURRENCY)
			);

			assert_noop!(
				CdpEngine::settle_cdp_has_debit(AccountId::from(ALICE), RELAY_CHAIN_CURRENCY),
				module_cdp_engine::Error::<Runtime>::NoDebitValue,
			);

			assert_ok!(set_oracle_price(vec![
				(USD_CURRENCY, Price::saturating_from_rational(1, 1)),
				(RELAY_CHAIN_CURRENCY, Price::saturating_from_rational(3, 1))
			]));

			assert_ok!(CdpEngine::adjust_position(
				&AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				0,
				(200 * dollar(USD_CURRENCY)) as i128
			));
			assert_eq!(
				Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).debit,
				200 * dollar(USD_CURRENCY)
			);
			assert_eq!(CdpTreasury::debit_pool(), 0);
			assert_eq!(CdpTreasury::total_collaterals(RELAY_CHAIN_CURRENCY), 0);
			assert_ok!(CdpEngine::settle_cdp_has_debit(
				AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY
			));

			let settle_cdp_in_debit_event = Event::CdpEngine(module_cdp_engine::Event::SettleCDPInDebit(
				RELAY_CHAIN_CURRENCY,
				AccountId::from(ALICE),
			));
			assert!(System::events()
				.iter()
				.any(|record| record.event == settle_cdp_in_debit_event));

			assert_eq!(Loans::positions(RELAY_CHAIN_CURRENCY, AccountId::from(ALICE)).debit, 0);
			assert_eq!(CdpTreasury::debit_pool(), 20 * dollar(USD_CURRENCY));

			// DOT is 10 decimal places where as ksm is 12 decimals. Hence the difference in collaterals.
			#[cfg(feature = "with-mandala-runtime")]
			assert_eq!(CdpTreasury::total_collaterals(RELAY_CHAIN_CURRENCY), 66_666_666_666);
			#[cfg(feature = "with-karura-runtime")]
			assert_eq!(CdpTreasury::total_collaterals(RELAY_CHAIN_CURRENCY), 6_666_666_666_666);
		});
}

#[test]
fn test_authority_module() {
	#[cfg(feature = "with-mandala-runtime")]
	const AUTHORITY_ORIGIN_ID: u8 = 70u8;

	#[cfg(feature = "with-karura-runtime")]
	const AUTHORITY_ORIGIN_ID: u8 = 60u8;

	ExtBuilder::default()
		.balances(vec![
			(AccountId::from(ALICE), USD_CURRENCY, 1_000 * dollar(USD_CURRENCY)),
			(
				AccountId::from(ALICE),
				RELAY_CHAIN_CURRENCY,
				1_000 * dollar(RELAY_CHAIN_CURRENCY),
			),
			(TreasuryAccount::get(), USD_CURRENCY, 1_000 * dollar(USD_CURRENCY)),
		])
		.build()
		.execute_with(|| {
			let ensure_root_call = Call::System(frame_system::Call::fill_block(Perbill::one()));
			let call = Call::Authority(orml_authority::Call::dispatch_as(
				AuthoritysOriginId::Root,
				Box::new(ensure_root_call.clone()),
			));

			// dispatch_as
			assert_ok!(Authority::dispatch_as(
				Origin::root(),
				AuthoritysOriginId::Root,
				Box::new(ensure_root_call.clone())
			));

			assert_noop!(
				Authority::dispatch_as(
					Origin::signed(AccountId::from(BOB)),
					AuthoritysOriginId::Root,
					Box::new(ensure_root_call.clone())
				),
				BadOrigin
			);

			assert_noop!(
				Authority::dispatch_as(
					Origin::signed(AccountId::from(BOB)),
					AuthoritysOriginId::Treasury,
					Box::new(ensure_root_call.clone())
				),
				BadOrigin
			);

			// schedule_dispatch
			run_to_block(1);
			// Treasury transfer
			let transfer_call = Call::Currencies(module_currencies::Call::transfer(
				AccountId::from(BOB).into(),
				USD_CURRENCY,
				500 * dollar(USD_CURRENCY),
			));
			let treasury_reserve_call = Call::Authority(orml_authority::Call::dispatch_as(
				AuthoritysOriginId::Treasury,
				Box::new(transfer_call.clone()),
			));

			let one_day_later = OneDay::get() + 1;

			assert_ok!(Authority::schedule_dispatch(
				Origin::root(),
				DispatchTime::At(one_day_later),
				0,
				true,
				Box::new(treasury_reserve_call.clone())
			));

			assert_ok!(Authority::schedule_dispatch(
				Origin::root(),
				DispatchTime::At(one_day_later),
				0,
				true,
				Box::new(call.clone())
			));
			System::assert_last_event(Event::Authority(orml_authority::Event::Scheduled(
				OriginCaller::Authority(DelayedOrigin {
					delay: one_day_later - 1,
					origin: Box::new(OriginCaller::system(RawOrigin::Root)),
				}),
				1,
			)));

			run_to_block(one_day_later);

			assert_eq!(
				Currencies::free_balance(USD_CURRENCY, &TreasuryPalletId::get().into_account()),
				500 * dollar(USD_CURRENCY)
			);
			assert_eq!(
				Currencies::free_balance(USD_CURRENCY, &AccountId::from(BOB)),
				500 * dollar(USD_CURRENCY)
			);

			// delay < SevenDays
			#[cfg(feature = "with-mandala-runtime")]
			System::assert_last_event(Event::Scheduler(pallet_scheduler::Event::<Runtime>::Dispatched(
				(OneDay::get() + 1, 1),
				Some([AUTHORITY_ORIGIN_ID, 64, 56, 0, 0, 0, 0, 1, 0, 0, 0].to_vec()),
				Err(DispatchError::BadOrigin),
			)));
			#[cfg(feature = "with-karura-runtime")]
			System::assert_last_event(Event::Scheduler(pallet_scheduler::Event::<Runtime>::Dispatched(
				(OneDay::get() + 1, 1),
				Some([AUTHORITY_ORIGIN_ID, 32, 28, 0, 0, 0, 0, 1, 0, 0, 0].to_vec()),
				Err(DispatchError::BadOrigin),
			)));

			let seven_days_later = one_day_later + SevenDays::get() + 1;

			// delay = SevenDays
			assert_ok!(Authority::schedule_dispatch(
				Origin::root(),
				DispatchTime::At(seven_days_later),
				0,
				true,
				Box::new(call.clone())
			));

			run_to_block(seven_days_later);

			#[cfg(feature = "with-mandala-runtime")]
			System::assert_last_event(Event::Scheduler(pallet_scheduler::Event::<Runtime>::Dispatched(
				(seven_days_later, 0),
				Some([AUTHORITY_ORIGIN_ID, 193, 137, 1, 0, 0, 0, 2, 0, 0, 0].to_vec()),
				Ok(()),
			)));

			#[cfg(feature = "with-karura-runtime")]
			System::assert_last_event(Event::Scheduler(pallet_scheduler::Event::<Runtime>::Dispatched(
				(seven_days_later, 0),
				Some([AUTHORITY_ORIGIN_ID, 225, 196, 0, 0, 0, 0, 2, 0, 0, 0].to_vec()),
				Ok(()),
			)));

			// with_delayed_origin = false
			assert_ok!(Authority::schedule_dispatch(
				Origin::root(),
				DispatchTime::At(seven_days_later + 1),
				0,
				false,
				Box::new(call.clone())
			));
			System::assert_last_event(Event::Authority(orml_authority::Event::Scheduled(
				OriginCaller::system(RawOrigin::Root),
				3,
			)));

			run_to_block(seven_days_later + 1);
			System::assert_last_event(Event::Scheduler(pallet_scheduler::Event::<Runtime>::Dispatched(
				(seven_days_later + 1, 0),
				Some([0, 0, 3, 0, 0, 0].to_vec()),
				Ok(()),
			)));

			assert_ok!(Authority::schedule_dispatch(
				Origin::root(),
				DispatchTime::At(seven_days_later + 2),
				0,
				false,
				Box::new(call.clone())
			));

			// fast_track_scheduled_dispatch
			assert_ok!(Authority::fast_track_scheduled_dispatch(
				Origin::root(),
				Box::new(frame_system::RawOrigin::Root.into()),
				4,
				DispatchTime::At(seven_days_later + 3),
			));

			// delay_scheduled_dispatch
			assert_ok!(Authority::delay_scheduled_dispatch(
				Origin::root(),
				Box::new(frame_system::RawOrigin::Root.into()),
				4,
				4,
			));

			// cancel_scheduled_dispatch
			assert_ok!(Authority::schedule_dispatch(
				Origin::root(),
				DispatchTime::At(seven_days_later + 2),
				0,
				true,
				Box::new(call.clone())
			));
			System::assert_last_event(Event::Authority(orml_authority::Event::Scheduled(
				OriginCaller::Authority(DelayedOrigin {
					delay: 1,
					origin: Box::new(OriginCaller::system(RawOrigin::Root)),
				}),
				5,
			)));

			let schedule_origin = {
				let origin: <Runtime as orml_authority::Config>::Origin = From::from(Origin::root());
				let origin: <Runtime as orml_authority::Config>::Origin = From::from(DelayedOrigin::<
					BlockNumber,
					<Runtime as orml_authority::Config>::PalletsOrigin,
				> {
					delay: 1,
					origin: Box::new(origin.caller().clone()),
				});
				origin
			};

			let pallets_origin = Box::new(schedule_origin.caller().clone());
			assert_ok!(Authority::cancel_scheduled_dispatch(Origin::root(), pallets_origin, 5));
			System::assert_last_event(Event::Authority(orml_authority::Event::Cancelled(
				OriginCaller::Authority(DelayedOrigin {
					delay: 1,
					origin: Box::new(OriginCaller::system(RawOrigin::Root)),
				}),
				5,
			)));

			assert_ok!(Authority::schedule_dispatch(
				Origin::root(),
				DispatchTime::At(seven_days_later + 3),
				0,
				false,
				Box::new(call.clone())
			));
			System::assert_last_event(Event::Authority(orml_authority::Event::Scheduled(
				OriginCaller::system(RawOrigin::Root),
				6,
			)));

			assert_ok!(Authority::cancel_scheduled_dispatch(
				Origin::root(),
				Box::new(frame_system::RawOrigin::Root.into()),
				6
			));
			System::assert_last_event(Event::Authority(orml_authority::Event::Cancelled(
				OriginCaller::system(RawOrigin::Root),
				6,
			)));
		});
}

#[test]
fn test_nft_module() {
	ExtBuilder::default()
		.balances(vec![(
			AccountId::from(ALICE),
			NATIVE_CURRENCY,
			1_000 * dollar(NATIVE_CURRENCY),
		)])
		.build()
		.execute_with(|| {
			let metadata = vec![1];
			assert_eq!(
				Balances::free_balance(AccountId::from(ALICE)),
				1_000 * dollar(NATIVE_CURRENCY)
			);
			assert_eq!(Balances::reserved_balance(AccountId::from(ALICE)), 0);
			assert_ok!(NFT::create_class(
				Origin::signed(AccountId::from(ALICE)),
				metadata.clone(),
				module_nft::Properties(
					module_nft::ClassProperty::Transferable
						| module_nft::ClassProperty::Burnable
						| module_nft::ClassProperty::Mintable
				),
				Default::default(),
			));
			let deposit =
				Proxy::deposit(1u32) + CreateClassDeposit::get() + DataDepositPerByte::get() * (metadata.len() as u128);
			assert_eq!(Balances::free_balance(&NftPalletId::get().into_sub_account(0)), 0);
			assert_eq!(
				Balances::reserved_balance(&NftPalletId::get().into_sub_account(0)),
				deposit
			);
			assert_eq!(
				Balances::free_balance(AccountId::from(ALICE)),
				1_000 * dollar(NATIVE_CURRENCY) - deposit
			);
			assert_eq!(Balances::reserved_balance(AccountId::from(ALICE)), 0);
			assert_ok!(Balances::deposit_into_existing(
				&NftPalletId::get().into_sub_account(0),
				1 * (CreateTokenDeposit::get() + DataDepositPerByte::get())
			));
			assert_ok!(NFT::mint(
				Origin::signed(NftPalletId::get().into_sub_account(0)),
				MultiAddress::Id(AccountId::from(BOB)),
				0,
				metadata.clone(),
				Default::default(),
				1
			));
			assert_ok!(NFT::burn(Origin::signed(AccountId::from(BOB)), (0, 0)));
			assert_eq!(
				Balances::free_balance(AccountId::from(BOB)),
				CreateTokenDeposit::get() + DataDepositPerByte::get()
			);
			assert_noop!(
				NFT::destroy_class(
					Origin::signed(NftPalletId::get().into_sub_account(0)),
					0,
					MultiAddress::Id(AccountId::from(BOB))
				),
				pallet_proxy::Error::<Runtime>::NotFound
			);
			assert_ok!(NFT::destroy_class(
				Origin::signed(NftPalletId::get().into_sub_account(0)),
				0,
				MultiAddress::Id(AccountId::from(ALICE))
			));
			assert_eq!(
				Balances::free_balance(AccountId::from(BOB)),
				CreateTokenDeposit::get() + DataDepositPerByte::get()
			);
			assert_eq!(Balances::reserved_balance(AccountId::from(BOB)), 0);
			assert_eq!(
				Balances::free_balance(AccountId::from(ALICE)),
				1_000 * dollar(NATIVE_CURRENCY)
			);
			assert_eq!(Balances::reserved_balance(AccountId::from(ALICE)), 0);
		});
}

#[test]
fn test_evm_accounts_module() {
	ExtBuilder::default()
		.balances(vec![(bob(), NATIVE_CURRENCY, 1_000 * dollar(NATIVE_CURRENCY))])
		.build()
		.execute_with(|| {
			assert_eq!(Balances::free_balance(AccountId::from(ALICE)), 0);
			assert_eq!(Balances::free_balance(bob()), 1_000 * dollar(NATIVE_CURRENCY));
			assert_ok!(EvmAccounts::claim_account(
				Origin::signed(AccountId::from(ALICE)),
				EvmAccounts::eth_address(&alice_key()),
				EvmAccounts::eth_sign(&alice_key(), &AccountId::from(ALICE).encode(), &[][..])
			));
			System::assert_last_event(Event::EvmAccounts(module_evm_accounts::Event::ClaimAccount(
				AccountId::from(ALICE),
				EvmAccounts::eth_address(&alice_key()),
			)));

			// claim another eth address
			assert_noop!(
				EvmAccounts::claim_account(
					Origin::signed(AccountId::from(ALICE)),
					EvmAccounts::eth_address(&alice_key()),
					EvmAccounts::eth_sign(&alice_key(), &AccountId::from(ALICE).encode(), &[][..])
				),
				module_evm_accounts::Error::<Runtime>::AccountIdHasMapped
			);
			assert_noop!(
				EvmAccounts::claim_account(
					Origin::signed(AccountId::from(BOB)),
					EvmAccounts::eth_address(&alice_key()),
					EvmAccounts::eth_sign(&alice_key(), &AccountId::from(BOB).encode(), &[][..])
				),
				module_evm_accounts::Error::<Runtime>::EthAddressHasMapped
			);

			// evm padded address will transfer_all to origin.
			assert_eq!(Balances::free_balance(bob()), 1_000 * dollar(NATIVE_CURRENCY));
			assert_eq!(Balances::free_balance(&AccountId::from(BOB)), 0);
			assert_eq!(System::providers(&bob()), 1);
			assert_eq!(System::providers(&AccountId::from(BOB)), 0);
			assert_ok!(EvmAccounts::claim_account(
				Origin::signed(AccountId::from(BOB)),
				EvmAccounts::eth_address(&bob_key()),
				EvmAccounts::eth_sign(&bob_key(), &AccountId::from(BOB).encode(), &[][..])
			));
			assert_eq!(System::providers(&bob()), 0);
			assert_eq!(System::providers(&AccountId::from(BOB)), 1);
			assert_eq!(Balances::free_balance(bob()), 0);
			assert_eq!(
				Balances::free_balance(&AccountId::from(BOB)),
				1_000 * dollar(NATIVE_CURRENCY)
			);
		});
}

#[test]
fn test_vesting_use_relaychain_block_number() {
	ExtBuilder::default().build().execute_with(|| {
		#[cfg(feature = "with-mandala-runtime")]
		let signer: AccountId = TreasuryPalletId::get().into_account();
		#[cfg(feature = "with-karura-runtime")]
		let signer: AccountId = KaruraFoundationAccounts::get()[0].clone();

		assert_ok!(Balances::set_balance(
			Origin::root(),
			signer.clone().into(),
			1_000 * dollar(ACA),
			0
		));

		assert_ok!(Vesting::vested_transfer(
			Origin::signed(signer),
			alice().into(),
			VestingSchedule {
				start: 10,
				period: 2,
				period_count: 5,
				per_period: 3 * dollar(NATIVE_CURRENCY),
			}
		));

		assert_eq!(Balances::free_balance(&alice()), 15 * dollar(NATIVE_CURRENCY));
		assert_eq!(Balances::usable_balance(&alice()), 0);

		set_relaychain_block_number(10);

		assert_ok!(Vesting::claim(Origin::signed(alice())));
		assert_eq!(Balances::usable_balance(&alice()), 0);

		set_relaychain_block_number(12);

		assert_ok!(Vesting::claim(Origin::signed(alice())));
		assert_eq!(Balances::usable_balance(&alice()), 3 * dollar(NATIVE_CURRENCY));

		set_relaychain_block_number(15);

		assert_ok!(Vesting::claim(Origin::signed(alice())));
		assert_eq!(Balances::usable_balance(&alice()), 6 * dollar(NATIVE_CURRENCY));

		set_relaychain_block_number(20);

		assert_ok!(Vesting::claim(Origin::signed(alice())));
		assert_eq!(Balances::usable_balance(&alice()), 15 * dollar(NATIVE_CURRENCY));

		set_relaychain_block_number(22);

		assert_ok!(Vesting::claim(Origin::signed(alice())));
		assert_eq!(Balances::usable_balance(&alice()), 15 * dollar(NATIVE_CURRENCY));
	});
}

#[test]
fn test_session_manager_module() {
	ExtBuilder::default().build().execute_with(|| {
		assert_eq!(Session::session_index(), 0);
		assert_eq!(SessionManager::session_duration(), 10);
		run_to_block(10);
		assert_eq!(Session::session_index(), 1);
		assert_eq!(SessionManager::session_duration(), 10);

		assert_ok!(SessionManager::schedule_session_duration(RawOrigin::Root.into(), 2, 11));

		run_to_block(19);
		assert_eq!(Session::session_index(), 1);
		assert_eq!(SessionManager::session_duration(), 10);

		run_to_block(20);
		assert_eq!(Session::session_index(), 2);
		assert_eq!(SessionManager::session_duration(), 11);

		run_to_block(31);
		assert_eq!(Session::session_index(), 3);
		assert_eq!(SessionManager::session_duration(), 11);

		assert_ok!(SessionManager::schedule_session_duration(RawOrigin::Root.into(), 4, 9));

		run_to_block(42);
		assert_eq!(Session::session_index(), 4);
		assert_eq!(SessionManager::session_duration(), 9);

		run_to_block(50);
		assert_eq!(Session::session_index(), 4);
		assert_eq!(SessionManager::session_duration(), 9);

		run_to_block(51);
		assert_eq!(Session::session_index(), 5);
		assert_eq!(SessionManager::session_duration(), 9);
	});
}

#[test]
fn treasury_should_take_xcm_execution_revenue() {
	ExtBuilder::default().build().execute_with(|| {
		let dot_amount = 1000 * dollar(RELAY_CHAIN_CURRENCY);
		#[cfg(feature = "with-mandala-runtime")]
		let actual_amount = 9_999_999_760_000;
		#[cfg(feature = "with-karura-runtime")]
		let actual_amount = 999_999_952_000_000;

		#[cfg(feature = "with-mandala-runtime")]
		let shallow_weight = 3_000_000;
		#[cfg(feature = "with-karura-runtime")]
		let shallow_weight = 600_000_000;
		let origin = MultiLocation::X1(Junction::Parent);

		// receive relay chain token
		let mut msg = Xcm::<Call>::ReserveAssetDeposit {
			assets: vec![MultiAsset::ConcreteFungible {
				id: MultiLocation::X1(Junction::Parent),
				amount: dot_amount,
			}],
			effects: vec![
				BuyExecution {
					fees: MultiAsset::All,
					weight: 0,
					debt: shallow_weight,
					halt_on_error: true,
					xcm: vec![],
				},
				DepositAsset {
					assets: vec![MultiAsset::All],
					dest: MultiLocation::X1(Junction::AccountId32 {
						network: NetworkId::Any,
						id: ALICE,
					}),
				},
			],
		};
		use xcm_executor::traits::WeightBounds;
		let debt = <XcmConfig as xcm_executor::Config>::Weigher::shallow(&mut msg).unwrap_or_default();
		let deep = <XcmConfig as xcm_executor::Config>::Weigher::deep(&mut msg).unwrap_or_default();
		// println!("debt = {:?}, deep = {:?}", debt, deep);
		assert_eq!(debt, shallow_weight);

		assert_eq!(Tokens::free_balance(RELAY_CHAIN_CURRENCY, &ALICE.into()), 0);
		assert_ok!(Currencies::deposit(
			RELAY_CHAIN_CURRENCY,
			&TreasuryAccount::get(),
			ExistentialDeposits::get(&RELAY_CHAIN_CURRENCY)
		));
		assert_eq!(
			Tokens::free_balance(RELAY_CHAIN_CURRENCY, &TreasuryAccount::get()),
			ExistentialDeposits::get(&RELAY_CHAIN_CURRENCY)
		);

		let weight_limit = debt + deep + 1;
		assert_eq!(
			XcmExecutor::<XcmConfig>::execute_xcm(origin, msg, weight_limit),
			Outcome::Complete(shallow_weight)
		);

		assert_eq!(Tokens::free_balance(RELAY_CHAIN_CURRENCY, &ALICE.into()), actual_amount);
		assert_eq!(
			Tokens::free_balance(RELAY_CHAIN_CURRENCY, &TreasuryAccount::get()),
			ExistentialDeposits::get(&RELAY_CHAIN_CURRENCY) + dot_amount - actual_amount
		);
	});
}

#[test]
fn currency_id_convert() {
	ExtBuilder::default().build().execute_with(|| {
		let id: u32 = ParachainInfo::get().into();

		assert_eq!(CurrencyIdConvert::convert(RELAY_CHAIN_CURRENCY), Some(X1(Parent)));
		assert_eq!(
			CurrencyIdConvert::convert(NATIVE_CURRENCY),
			Some(X3(Parent, Parachain(id), GeneralKey(NATIVE_CURRENCY.encode())))
		);
		assert_eq!(
			CurrencyIdConvert::convert(USD_CURRENCY),
			Some(X3(Parent, Parachain(id), GeneralKey(USD_CURRENCY.encode())))
		);
		assert_eq!(
			CurrencyIdConvert::convert(LIQUID_CURRENCY),
			Some(X3(Parent, Parachain(id), GeneralKey(LIQUID_CURRENCY.encode())))
		);
		assert_eq!(
			CurrencyIdConvert::convert(RENBTC),
			Some(X3(Parent, Parachain(id), GeneralKey(RENBTC.encode())))
		);

		#[cfg(feature = "with-mandala-runtime")]
		{
			assert_eq!(CurrencyIdConvert::convert(KAR), None);
			assert_eq!(CurrencyIdConvert::convert(KUSD), None);
			assert_eq!(CurrencyIdConvert::convert(KSM), None);
			assert_eq!(CurrencyIdConvert::convert(LKSM), None);

			assert_eq!(CurrencyIdConvert::convert(X1(Parent)), Some(RELAY_CHAIN_CURRENCY));
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(NATIVE_CURRENCY.encode()))),
				Some(NATIVE_CURRENCY)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(USD_CURRENCY.encode()))),
				Some(USD_CURRENCY)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(LIQUID_CURRENCY.encode()))),
				Some(LIQUID_CURRENCY)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(RENBTC.encode()))),
				Some(RENBTC)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(KAR.encode()))),
				None
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(KUSD.encode()))),
				None
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(KSM.encode()))),
				None
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(LKSM.encode()))),
				None
			);

			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id + 1), GeneralKey(RENBTC.encode()))),
				None
			);

			assert_eq!(
				CurrencyIdConvert::convert(MultiAsset::ConcreteFungible {
					id: X3(Parent, Parachain(id), GeneralKey(NATIVE_CURRENCY.encode())),
					amount: 1
				}),
				Some(NATIVE_CURRENCY)
			);
		}

		#[cfg(feature = "with-karura-runtime")]
		{
			assert_eq!(CurrencyIdConvert::convert(ACA), None);
			assert_eq!(CurrencyIdConvert::convert(AUSD), None);
			assert_eq!(CurrencyIdConvert::convert(DOT), None);
			assert_eq!(CurrencyIdConvert::convert(LDOT), None);
			assert_eq!(CurrencyIdConvert::convert(X1(Parent)), Some(RELAY_CHAIN_CURRENCY));
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(NATIVE_CURRENCY.encode()))),
				Some(NATIVE_CURRENCY)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(USD_CURRENCY.encode()))),
				Some(USD_CURRENCY)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(LIQUID_CURRENCY.encode()))),
				Some(LIQUID_CURRENCY)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(RENBTC.encode()))),
				Some(RENBTC)
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(ACA.encode()))),
				None
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(AUSD.encode()))),
				None
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(DOT.encode()))),
				None
			);
			assert_eq!(
				CurrencyIdConvert::convert(X3(Parent, Parachain(id), GeneralKey(LDOT.encode()))),
				None
			);

			assert_eq!(
				CurrencyIdConvert::convert(MultiAsset::ConcreteFungible {
					id: X3(Parent, Parachain(id), GeneralKey(NATIVE_CURRENCY.encode())),
					amount: 1
				}),
				Some(NATIVE_CURRENCY)
			);
		}
	});
}

#[test]
fn sanity_check_weight_per_time_constants_are_as_expected() {
	// These values comes from Substrate, we want to make sure that if it
	// ever changes we don't accidently break Polkadot
	assert_eq!(WEIGHT_PER_SECOND, 1_000_000_000_000);
	assert_eq!(WEIGHT_PER_MILLIS, WEIGHT_PER_SECOND / 1000);
	assert_eq!(WEIGHT_PER_MICROS, WEIGHT_PER_MILLIS / 1000);
	assert_eq!(WEIGHT_PER_NANOS, WEIGHT_PER_MICROS / 1000);
}

#[test]
fn parachain_subaccounts_are_unique() {
	ExtBuilder::default().build().execute_with(|| {
		let parachain: AccountId = ParachainInfo::parachain_id().into_account();
		assert_eq!(
			parachain,
			hex_literal::hex!["70617261d0070000000000000000000000000000000000000000000000000000"].into()
		);

		assert_eq!(
			RelaychainSovereignSubAccount::get(),
			create_x2_parachain_multilocation(0)
		);

		assert_eq!(
			create_x2_parachain_multilocation(0),
			MultiLocation::X2(
				Junction::Parent,
				Junction::AccountId32 {
					network: NetworkId::Any,
					id: hex_literal::hex!["d7b8926b326dd349355a9a7cca6606c1e0eb6fd2b506066b518c7155ff0d8297"].into(),
				}
			)
		);
		assert_eq!(
			create_x2_parachain_multilocation(1),
			MultiLocation::X2(
				Junction::Parent,
				Junction::AccountId32 {
					network: NetworkId::Any,
					id: hex_literal::hex!["74d37d762e06c6841a5dad64463a9afe0684f7e45245f6a7296ca613cca74669"].into(),
				}
			)
		);
	});
}
