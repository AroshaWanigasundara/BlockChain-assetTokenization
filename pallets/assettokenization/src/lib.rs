//! # Asset Tokenization Pallet
//!
//! Tokenizes physical or digital assets as NFTs and attaches off-chain legal contracts
//! (stored on IPFS) to each token. All NFT logic is implemented natively.
//!
//! ## New in this version
//!
//! - **Collections** — assets can optionally belong to a named collection whose owner
//!   controls four roles: admin, issuer, and freezer.  Only accounts holding the issuer
//!   role (or the collection owner) may mint into a collection.
//! - **Role management** — `create_collection` / `set_collection_roles` extrinsics.
//! - **Contract version history** — every call to `update_contract` archives the
//!   previous hash in `ContractHistory` before overwriting, giving a full audit trail.
//! - **Fungible token ledger** — when `is_fungible = true` the full supply is credited
//!   to the minter; `transfer_fungible` moves fractional units between accounts.
//! - **Collection freezing** — `freeze_collection` prevents new assets from being minted
//!   into a collection, providing immutability for completed collections.

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

    // -------------------------------------------------------------------------
    // Config
    // -------------------------------------------------------------------------

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

    /// Metadata for an asset collection.
    ///
    /// A collection groups related assets and defines who may mint into it
    /// via the four-role model (owner, admin, issuer, freezer).
    #[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    #[scale_info(skip_type_params(T))]
    pub struct CollectionInfo<T: Config> {
        /// Human-readable collection name (max 64 bytes).
        pub name: BoundedVec<u8, ConstU32<64>>,
        /// Account that created and owns the collection; has all privileges.
        pub owner: T::AccountId,
        /// When `true`, no new assets may be minted into this collection.
        pub is_frozen: bool,
    }

    /// Per-account role flags within a collection.
    ///
    /// The collection owner always has all privileges.  This struct grants
    /// additional roles to other accounts.
    ///
    /// | Role    | Capability                                               |
    /// |---------|----------------------------------------------------------|
    /// | admin   | Update collection settings; assign issuer/freezer roles |
    /// | issuer  | Mint new assets into the collection                      |
    /// | freezer | Freeze individual assets within the collection           |
    #[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, Default)]
    pub struct CollectionRoleSet {
        /// Can update collection settings and assign issuer/freezer roles to others.
        /// Only the collection owner can grant or revoke the admin role itself.
        pub is_admin: bool,
        /// Can mint new assets into the collection.
        pub is_issuer: bool,
        /// Can freeze individual assets within the collection.
        pub is_freezer: bool,
    }

    // -------------------------------------------------------------------------
    // Storage — existing
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
    // Storage — collections
    // -------------------------------------------------------------------------

    /// Auto-incrementing counter used to assign unique IDs to new collections.
    #[pallet::storage]
    pub type NextCollectionId<T> = StorageValue<_, u64, ValueQuery>;

    /// Maps a collection ID to its metadata.
    #[pallet::storage]
    pub type Collections<T: Config> = StorageMap<_, Blake2_128Concat, u64, CollectionInfo<T>>;

    /// Role assignments within a collection.
    /// Key: (collection_id, account) → role flags.
    #[pallet::storage]
    pub type CollectionRoles<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        u64,
        Blake2_128Concat,
        T::AccountId,
        CollectionRoleSet,
    >;

    /// Optional link from an asset to its parent collection.
    /// Absent when the asset was minted without specifying a collection.
    #[pallet::storage]
    pub type AssetCollection<T> = StorageMap<_, Blake2_128Concat, u64, u64>;

    // -------------------------------------------------------------------------
    // Storage — contract history
    // -------------------------------------------------------------------------

    /// Historical contract hashes for an asset, indexed by update sequence number.
    ///
    /// Entry `(asset_id, n)` holds the hash that was *replaced* during the (n+1)-th
    /// update, i.e. entry 0 is the original hash, entry 1 is the hash after the first
    /// update, and so on.  The *current* hash is always in `Assets[asset_id].contract_hash`.
    #[pallet::storage]
    pub type ContractHistory<T> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        u64,
        Blake2_128Concat,
        u32,
        [u8; 32],
    >;

    /// Number of history entries recorded for each asset
    /// (equals the number of times `update_contract` has been called for that asset).
    #[pallet::storage]
    pub type ContractHistoryCount<T> = StorageMap<_, Blake2_128Concat, u64, u32, ValueQuery>;

    // -------------------------------------------------------------------------
    // Storage — fungible balances
    // -------------------------------------------------------------------------

    /// Per-account balance of a fungible asset.
    /// Key: (asset_id, AccountId) → balance.
    /// Only meaningful when `AssetInfo.is_fungible == true`.
    #[pallet::storage]
    pub type FungibleBalances<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        u64,
        Blake2_128Concat,
        T::AccountId,
        u128,
        ValueQuery,
    >;

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        // --- asset lifecycle ---
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
        /// `version` is the new update count (1 after the first update, 2 after the second, …).
        ContractUpdated {
            asset_id: u64,
            new_hash: [u8; 32],
            version: u32,
        },
        /// An asset's metadata was frozen; no further updates are possible.
        AssetFrozen { asset_id: u64 },
        /// Ownership of an asset was transferred.
        AssetTransferred {
            asset_id: u64,
            from: T::AccountId,
            to: T::AccountId,
        },

        // --- collection management ---
        /// A new collection was created.
        CollectionCreated {
            collection_id: u64,
            owner: T::AccountId,
        },
        /// Role flags for an account within a collection were updated.
        CollectionRolesUpdated {
            collection_id: u64,
            who: T::AccountId,
            roles: CollectionRoleSet,
        },
        /// A collection was frozen; no new assets may be minted into it.
        CollectionFrozen {
            collection_id: u64,
        },

        // --- fungible tokens ---
        /// Fungible token units were transferred between accounts.
        FungibleTransferred {
            asset_id: u64,
            from: T::AccountId,
            to: T::AccountId,
            amount: u128,
        },
    }

    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    #[pallet::error]
    pub enum Error<T> {
        // --- asset errors ---
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
        ContractNotSigned,
        /// `fungible_supply` must be `Some` when `is_fungible` is `true`, and
        /// `None` when `is_fungible` is `false`.
        InconsistentFungibleSupply,

        // --- collection errors ---
        /// No collection exists with the given ID.
        CollectionNotFound,
        /// The caller is not the owner of the collection.
        NotCollectionOwner,
        /// The caller lacks the required role for this operation.
        NotAuthorized,
        /// The collection is frozen; no new assets may be minted into it.
        CollectionIsFrozen,

        // --- fungible errors ---
        /// The asset was not minted as a fungible token.
        AssetNotFungible,
        /// The caller does not hold enough fungible balance for this transfer.
        InsufficientFungibleBalance,
        /// Arithmetic overflow while computing a new balance.
        ArithmeticOverflow,
    }

    // -------------------------------------------------------------------------
    // Dispatchables
    // -------------------------------------------------------------------------

    #[pallet::call]
    impl<T: Config> Pallet<T> {

        // ── call_index 0: mint_asset ──────────────────────────────────────────

        /// Mint a new tokenized asset.
        ///
        /// Optionally, the asset can be minted into a `collection_id`.  When a
        /// collection is specified the caller must be the collection owner or hold
        /// the **issuer** role; the collection must not be frozen.
        ///
        /// When `is_fungible = true` the entire `fungible_supply` is immediately
        /// credited to the caller's fungible balance for that asset.
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
            collection_id: Option<u64>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(contract_hash != [0u8; 32], Error::<T>::InvalidContractHash);

            // Enforce fungible supply consistency at mint time.
            match (is_fungible, &fungible_supply) {
                (true, None) | (false, Some(_)) =>
                    return Err(Error::<T>::InconsistentFungibleSupply.into()),
                _ => {},
            }

            // If minting into a collection, verify access rights.
            if let Some(coll_id) = collection_id {
                let collection =
                    Collections::<T>::get(coll_id).ok_or(Error::<T>::CollectionNotFound)?;
                ensure!(!collection.is_frozen, Error::<T>::CollectionIsFrozen);
                let is_owner = collection.owner == who;
                let roles = CollectionRoles::<T>::get(coll_id, &who).unwrap_or_default();
                ensure!(is_owner || roles.is_issuer, Error::<T>::NotAuthorized);
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

            // Link to the parent collection if supplied.
            if let Some(coll_id) = collection_id {
                AssetCollection::<T>::insert(asset_id, coll_id);
            }

            // Credit full fungible supply to the creator.
            if is_fungible {
                if let Some(supply) = fungible_supply {
                    FungibleBalances::<T>::insert(asset_id, &who, supply);
                }
            }

            Self::deposit_event(Event::AssetMinted {
                asset_id,
                owner: who,
                contract_hash,
            });

            Ok(())
        }

        // ── call_index 1: sign_contract ───────────────────────────────────────

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
            let block_u64: u64 = TryInto::<u64>::try_into(current_block).unwrap_or(0u64);

            ContractSignatures::<T>::insert(asset_id, &who, block_u64);

            Self::deposit_event(Event::ContractSigned {
                asset_id,
                signer: who,
                block: current_block,
            });

            Ok(())
        }

        // ── call_index 2: update_contract ────────────────────────────────────

        /// Update the IPFS contract URI and hash for an asset.
        ///
        /// Only the current owner may call this. Fails if the asset is frozen.
        ///
        /// **Audit trail**: before overwriting, the current hash is appended to
        /// `ContractHistory` so the complete version history is preserved on-chain.
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

            // Archive the current hash before overwriting.
            let history_index = ContractHistoryCount::<T>::get(asset_id);
            Assets::<T>::try_mutate(asset_id, |maybe_info| -> DispatchResult {
                let info = maybe_info.as_mut().ok_or(Error::<T>::AssetNotFound)?;
                // Save the outgoing hash to history.
                ContractHistory::<T>::insert(asset_id, history_index, info.contract_hash);
                info.contract_uri = new_contract_uri;
                info.contract_hash = new_contract_hash;
                Ok(())
            })?;
            ContractHistoryCount::<T>::mutate(asset_id, |c| *c = c.saturating_add(1));

            // The new version number equals the updated history count.
            let new_version = ContractHistoryCount::<T>::get(asset_id);

            Self::deposit_event(Event::ContractUpdated {
                asset_id,
                new_hash: new_contract_hash,
                version: new_version,
            });

            Ok(())
        }

        // ── call_index 3: freeze_asset ────────────────────────────────────────

        /// Permanently freeze an asset's metadata.
        ///
        /// May be called by the **asset owner** or an account holding the **freezer**
        /// role in the asset's parent collection.  Once frozen the asset cannot be
        /// updated, signed, or re-frozen.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::freeze_asset())]
        pub fn freeze_asset(origin: OriginFor<T>, asset_id: u64) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(Assets::<T>::contains_key(asset_id), Error::<T>::AssetNotFound);

            let owner = AssetOwner::<T>::get(asset_id).ok_or(Error::<T>::AssetNotFound)?;

            // Authorized callers: asset owner OR collection freezer/owner.
            let authorized = if owner == who {
                true
            } else if let Some(coll_id) = AssetCollection::<T>::get(asset_id) {
                let collection =
                    Collections::<T>::get(coll_id).ok_or(Error::<T>::CollectionNotFound)?;
                let roles = CollectionRoles::<T>::get(coll_id, &who).unwrap_or_default();
                collection.owner == who || roles.is_freezer
            } else {
                false
            };
            ensure!(authorized, Error::<T>::NotAssetOwner);

            ensure!(!FrozenAssets::<T>::get(asset_id), Error::<T>::AssetIsFrozen);

            FrozenAssets::<T>::insert(asset_id, true);

            Self::deposit_event(Event::AssetFrozen { asset_id });

            Ok(())
        }

        // ── call_index 4: transfer_asset ─────────────────────────────────────

        /// Transfer ownership of an asset to another account.
        ///
        /// The current owner must have signed the contract (via `sign_contract`)
        /// before a transfer is permitted — this ensures rights and obligations
        /// have been formally acknowledged before changing hands.
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

        // ── call_index 5: create_collection ──────────────────────────────────

        /// Create a new asset collection.
        ///
        /// The caller becomes the collection owner with full authority.  Additional
        /// accounts can be granted the admin, issuer, or freezer roles via
        /// `set_collection_roles`.
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::create_collection())]
        pub fn create_collection(
            origin: OriginFor<T>,
            name: BoundedVec<u8, ConstU32<64>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let collection_id = NextCollectionId::<T>::get();

            let info = CollectionInfo { name, owner: who.clone(), is_frozen: false };

            Collections::<T>::insert(collection_id, info);
            NextCollectionId::<T>::put(collection_id.saturating_add(1));

            Self::deposit_event(Event::CollectionCreated { collection_id, owner: who });

            Ok(())
        }

        // ── call_index 6: set_collection_roles ───────────────────────────────

        /// Update the role flags for an account within a collection.
        ///
        /// Rules:
        /// - Only the **collection owner** or an existing **admin** may call this.
        /// - An admin may grant/revoke the **issuer** and **freezer** roles but
        ///   cannot grant or revoke the **admin** role — that power is reserved for
        ///   the collection owner.
        /// - Passing an all-`false` `CollectionRoleSet` effectively removes an
        ///   account's roles (the entry is removed from storage).
        #[pallet::call_index(6)]
        #[pallet::weight(T::WeightInfo::set_collection_roles())]
        pub fn set_collection_roles(
            origin: OriginFor<T>,
            collection_id: u64,
            who: T::AccountId,
            roles: CollectionRoleSet,
        ) -> DispatchResult {
            let caller = ensure_signed(origin)?;

            let collection =
                Collections::<T>::get(collection_id).ok_or(Error::<T>::CollectionNotFound)?;

            let is_owner = collection.owner == caller;
            let caller_roles =
                CollectionRoles::<T>::get(collection_id, &caller).unwrap_or_default();

            // Must be collection owner OR an admin.
            ensure!(is_owner || caller_roles.is_admin, Error::<T>::NotAuthorized);

            // Admins cannot assign or revoke the admin role — only the owner can.
            if !is_owner && roles.is_admin {
                return Err(Error::<T>::NotAuthorized.into());
            }

            // Remove the storage entry when all flags are cleared to save space.
            if roles == CollectionRoleSet::default() {
                CollectionRoles::<T>::remove(collection_id, &who);
            } else {
                CollectionRoles::<T>::insert(collection_id, &who, roles.clone());
            }

            Self::deposit_event(Event::CollectionRolesUpdated {
                collection_id,
                who,
                roles,
            });

            Ok(())
        }

        // ── call_index 7: transfer_fungible ──────────────────────────────────

        /// Transfer fungible token units from the caller to another account.
        ///
        /// The asset must have been minted with `is_fungible = true`.
        /// Frozen assets cannot have their balances transferred.
        /// Zero-balance entries are removed from storage automatically.
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::transfer_fungible())]
        pub fn transfer_fungible(
            origin: OriginFor<T>,
            asset_id: u64,
            to: T::AccountId,
            amount: u128,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let info = Assets::<T>::get(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(info.is_fungible, Error::<T>::AssetNotFungible);
            ensure!(!FrozenAssets::<T>::get(asset_id), Error::<T>::AssetIsFrozen);

            let sender_balance = FungibleBalances::<T>::get(asset_id, &who);
            ensure!(sender_balance >= amount, Error::<T>::InsufficientFungibleBalance);

            let recipient_balance = FungibleBalances::<T>::get(asset_id, &to);
            let new_recipient_balance = recipient_balance
                .checked_add(amount)
                .ok_or(Error::<T>::ArithmeticOverflow)?;

            // Clean up zero-balance entries to avoid storage bloat.
            let new_sender_balance = sender_balance - amount;
            if new_sender_balance == 0 {
                FungibleBalances::<T>::remove(asset_id, &who);
            } else {
                FungibleBalances::<T>::insert(asset_id, &who, new_sender_balance);
            }
            FungibleBalances::<T>::insert(asset_id, &to, new_recipient_balance);

            Self::deposit_event(Event::FungibleTransferred {
                asset_id,
                from: who,
                to,
                amount,
            });

            Ok(())
        }

        // ── call_index 8: freeze_collection ──────────────────────────────────

        /// Permanently freeze a collection to prevent new assets from being minted.
        ///
        /// Only the collection owner or an account with the admin role may call this.
        /// Once frozen, the collection cannot be unfrozen and no new assets can be
        /// minted into it.
        #[pallet::call_index(8)]
        #[pallet::weight(T::WeightInfo::freeze_collection())]
        pub fn freeze_collection(
            origin: OriginFor<T>,
            collection_id: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            Collections::<T>::try_mutate(collection_id, |maybe_info| -> DispatchResult {
                let info = maybe_info.as_mut().ok_or(Error::<T>::CollectionNotFound)?;

                // Only collection owner or admin can freeze.
                let is_owner = info.owner == who;
                let roles = CollectionRoles::<T>::get(collection_id, &who).unwrap_or_default();
                ensure!(is_owner || roles.is_admin, Error::<T>::NotAuthorized);

                // Prevent freezing an already frozen collection.
                ensure!(!info.is_frozen, Error::<T>::CollectionIsFrozen);

                info.is_frozen = true;

                Ok(())
            })?;

            Self::deposit_event(Event::CollectionFrozen { collection_id });

            Ok(())
        }
    }
}
