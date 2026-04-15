//! # Asset Tokenization Pallet
//!
//! Tokenizes physical or digital assets as NFTs and attaches off-chain legal contracts
//! (stored on IPFS) to each token. All NFT logic is implemented natively — no external
//! NFT or Assets pallet is required.

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::*;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use codec::DecodeWithMemTracking;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Pallet configuration trait.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching runtime event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        /// Weight information for dispatchables.
        type WeightInfo: WeightInfo;
    }

    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    /// Classifies whether an asset is physical (e.g. real estate) or digital (e.g. software).
    #[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub enum AssetType {
        Physical,
        Digital,
    }

    /// All metadata associated with a tokenized asset.
    #[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    #[scale_info(skip_type_params(T))]
    pub struct AssetInfo<T: Config> {
        /// Human-readable name (max 64 bytes).
        pub name: BoundedVec<u8, ConstU32<64>>,
        /// Whether the underlying asset is physical or digital.
        pub asset_type: AssetType,
        /// IPFS URI pointing to the off-chain legal contract (max 256 bytes).
        pub contract_uri: BoundedVec<u8, ConstU32<256>>,
        /// SHA-256 hash of the off-chain contract, used for integrity verification.
        pub contract_hash: [u8; 32],
        /// Whether this token has a fungible supply.
        pub is_fungible: bool,
        /// Total supply when `is_fungible` is true.
        pub fungible_supply: Option<u128>,
        /// Account that originally minted the asset.
        pub creator: T::AccountId,
        /// Block at which the asset was created.
        pub created_at: BlockNumberFor<T>,
    }

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------

    /// Auto-incrementing counter used to assign unique IDs to new assets.
    #[pallet::storage]
    pub type NextAssetId<T> = StorageValue<_, u64, ValueQuery>;

    /// Maps an asset ID to its metadata.
    #[pallet::storage]
    pub type Assets<T: Config> = StorageMap<_, Blake2_128Concat, u64, AssetInfo<T>>;

    /// Maps an asset ID to its current owner.
    #[pallet::storage]
    pub type AssetOwner<T: Config> = StorageMap<_, Blake2_128Concat, u64, T::AccountId>;

    /// Records the block number at which a given account signed the contract for an asset.
    /// Key: (asset_id, signer_account) → block_number.
    #[pallet::storage]
    pub type ContractSignatures<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        u64,
        Blake2_128Concat,
        T::AccountId,
        u64,
    >;

    /// Tracks whether an asset's metadata has been permanently frozen (immutable).
    #[pallet::storage]
    pub type FrozenAssets<T> = StorageMap<_, Blake2_128Concat, u64, bool, ValueQuery>;

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new asset was minted.
        AssetMinted {
            asset_id: u64,
            owner: T::AccountId,
            contract_hash: [u8; 32],
        },
        /// An account signed the contract attached to an asset.
        ContractSigned {
            asset_id: u64,
            signer: T::AccountId,
            block: BlockNumberFor<T>,
        },
        /// The contract URI / hash for an asset was updated.
        ContractUpdated {
            asset_id: u64,
            new_hash: [u8; 32],
        },
        /// An asset's metadata was frozen; no further updates are possible.
        AssetFrozen { asset_id: u64 },
        /// Ownership of an asset was transferred.
        AssetTransferred {
            asset_id: u64,
            from: T::AccountId,
            to: T::AccountId,
        },
    }

    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    #[pallet::error]
    pub enum Error<T> {
        /// No asset exists with the given ID.
        AssetNotFound,
        /// The caller is not the owner of the asset.
        NotAssetOwner,
        /// The asset metadata is frozen and cannot be modified.
        AssetIsFrozen,
        /// The caller has already signed this asset's contract.
        AlreadySigned,
        /// The provided contract hash is invalid (all-zero hashes are rejected).
        InvalidContractHash,
        /// Transfer blocked: the asset owner has not yet signed the contract.
        /// The current owner must sign the contract before ownership can be transferred.
        ContractNotSigned,
        /// `fungible_supply` must be `Some` when `is_fungible` is `true`, and
        /// `None` when `is_fungible` is `false`.
        InconsistentFungibleSupply,
    }

    // -------------------------------------------------------------------------
    // Dispatchables
    // -------------------------------------------------------------------------

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Mint a new tokenized asset.
        ///
        /// Creates an `AssetInfo` entry, records the caller as owner, and increments
        /// the global asset-ID counter.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::mint_asset())]
        pub fn mint_asset(
            origin: OriginFor<T>,
            name: BoundedVec<u8, ConstU32<64>>,
            asset_type: AssetType,
            contract_uri: BoundedVec<u8, ConstU32<256>>,
            contract_hash: [u8; 32],
            is_fungible: bool,
            fungible_supply: Option<u128>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(contract_hash != [0u8; 32], Error::<T>::InvalidContractHash);

            // Fungible supply must be consistent with the is_fungible flag.
            // The design document treats fungible assets (fractional ownership) differently
            // from unique NFTs, so we enforce the invariant at mint time.
            match (is_fungible, &fungible_supply) {
                (true, None) | (false, Some(_)) =>
                    return Err(Error::<T>::InconsistentFungibleSupply.into()),
                _ => {},
            }

            let asset_id = NextAssetId::<T>::get();
            let current_block = <frame_system::Pallet<T>>::block_number();

            let info = AssetInfo {
                name,
                asset_type,
                contract_uri,
                contract_hash,
                is_fungible,
                fungible_supply,
                creator: who.clone(),
                created_at: current_block,
            };

            Assets::<T>::insert(asset_id, info);
            AssetOwner::<T>::insert(asset_id, who.clone());
            NextAssetId::<T>::put(asset_id.saturating_add(1));

            Self::deposit_event(Event::AssetMinted {
                asset_id,
                owner: who,
                contract_hash,
            });

            Ok(())
        }

        /// Sign the legal contract attached to an asset.
        ///
        /// Fails if the asset does not exist, its metadata is frozen, or the caller
        /// has already signed.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::sign_contract())]
        pub fn sign_contract(origin: OriginFor<T>, asset_id: u64) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(Assets::<T>::contains_key(asset_id), Error::<T>::AssetNotFound);
            ensure!(!FrozenAssets::<T>::get(asset_id), Error::<T>::AssetIsFrozen);
            ensure!(
                !ContractSignatures::<T>::contains_key(asset_id, &who),
                Error::<T>::AlreadySigned
            );

            let current_block = <frame_system::Pallet<T>>::block_number();
            let block_u64: u64 = TryInto::<u64>::try_into(current_block)
                .unwrap_or(0u64);

            ContractSignatures::<T>::insert(asset_id, &who, block_u64);

            Self::deposit_event(Event::ContractSigned {
                asset_id,
                signer: who,
                block: current_block,
            });

            Ok(())
        }

        /// Update the IPFS contract URI and hash for an asset.
        ///
        /// Only the current owner may call this. Fails if the asset is frozen.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::update_contract())]
        pub fn update_contract(
            origin: OriginFor<T>,
            asset_id: u64,
            new_contract_uri: BoundedVec<u8, ConstU32<256>>,
            new_contract_hash: [u8; 32],
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(new_contract_hash != [0u8; 32], Error::<T>::InvalidContractHash);
            ensure!(!FrozenAssets::<T>::get(asset_id), Error::<T>::AssetIsFrozen);

            let owner = AssetOwner::<T>::get(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(owner == who, Error::<T>::NotAssetOwner);

            Assets::<T>::try_mutate(asset_id, |maybe_info| -> DispatchResult {
                let info = maybe_info.as_mut().ok_or(Error::<T>::AssetNotFound)?;
                info.contract_uri = new_contract_uri;
                info.contract_hash = new_contract_hash;
                Ok(())
            })?;

            Self::deposit_event(Event::ContractUpdated {
                asset_id,
                new_hash: new_contract_hash,
            });

            Ok(())
        }

        /// Permanently freeze an asset's metadata.
        ///
        /// Only the current owner may call this. Once frozen the asset cannot be
        /// updated, signed, or re-frozen.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::freeze_asset())]
        pub fn freeze_asset(origin: OriginFor<T>, asset_id: u64) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(Assets::<T>::contains_key(asset_id), Error::<T>::AssetNotFound);

            let owner = AssetOwner::<T>::get(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(owner == who, Error::<T>::NotAssetOwner);

            ensure!(!FrozenAssets::<T>::get(asset_id), Error::<T>::AssetIsFrozen);

            FrozenAssets::<T>::insert(asset_id, true);

            Self::deposit_event(Event::AssetFrozen { asset_id });

            Ok(())
        }

        /// Transfer ownership of an asset to another account.
        ///
        /// Only the current owner may call this. The owner must have signed the
        /// contract (via `sign_contract`) before a transfer is permitted — this
        /// ensures that rights and obligations have been formally acknowledged
        /// before changing hands, as described in the design document.
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::transfer_asset())]
        pub fn transfer_asset(
            origin: OriginFor<T>,
            asset_id: u64,
            to: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(Assets::<T>::contains_key(asset_id), Error::<T>::AssetNotFound);

            let owner = AssetOwner::<T>::get(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(owner == who, Error::<T>::NotAssetOwner);

            // The design document states: "The pallet may include a check that prevents
            // transfer until signatures are recorded." We require the current owner to
            // have signed before they can hand the asset on.
            ensure!(
                ContractSignatures::<T>::contains_key(asset_id, &who),
                Error::<T>::ContractNotSigned
            );

            AssetOwner::<T>::insert(asset_id, to.clone());

            Self::deposit_event(Event::AssetTransferred {
                asset_id,
                from: who,
                to,
            });

            Ok(())
        }
    }
}
