//! STARK configuration: zero-knowledge FRI over Goldilocks with Keccak-256 Merkle commitments.
//!
//! The hiding PCS salts every Merkle leaf and adds random codewords, so that openings reveal
//! nothing about the witness: without it, a STARK is not zero-knowledge.

use p3_challenger::{HashChallenger, SerializingChallenger64};
use p3_commit::ExtensionMmcs;
use p3_dft::Radix2DitParallel;
use p3_field::extension::BinomialExtensionField;
use p3_fri::{FriParameters, HidingFriPcs};
use p3_goldilocks::Goldilocks;
use p3_keccak::{Keccak256Hash, KeccakF};
use p3_merkle_tree::MerkleTreeHidingMmcs;
use p3_symmetric::{CompressionFunctionFromHasher, PaddingFreeSponge, SerializingHasher};
use p3_uni_stark::StarkConfig;
use rand::{rngs::StdRng, SeedableRng};

/// Base field.
pub type Val = Goldilocks;
/// Challenge field: quadratic extension of Goldilocks (128 bits).
pub type Challenge = BinomialExtensionField<Val, 2>;

type ByteHash = Keccak256Hash;
type U64Hash = PaddingFreeSponge<KeccakF, 25, 17, 4>;
type FieldHash = SerializingHasher<U64Hash>;
type Compress = CompressionFunctionFromHasher<U64Hash, 2, 4>;
type ValMmcs = MerkleTreeHidingMmcs<
	[Val; p3_keccak::VECTOR_LEN],
	[u64; p3_keccak::VECTOR_LEN],
	FieldHash,
	Compress,
	StdRng,
	2,
	4,
	4,
>;
type ChallengeMmcs = ExtensionMmcs<Val, Challenge, ValMmcs>;
type Challenger = SerializingChallenger64<Val, HashChallenger<u8, ByteHash, 32>>;
type Dft = Radix2DitParallel<Val>;
type Pcs = HidingFriPcs<Val, Dft, ValMmcs, ChallengeMmcs, StdRng>;

/// The STARK configuration of shielded transactions.
pub type Config = StarkConfig<Pcs, Challenge, Challenger>;

/// `log2` of the FRI blowup factor.
pub const LOG_BLOWUP: usize = 3;
/// Number of FRI queries.
pub const NUM_QUERIES: usize = 38;
/// Proof of work before sampling the queries.
pub const QUERY_POW_BITS: usize = 16;
/// Random codewords added by the hiding PCS.
pub const NUM_RANDOM_CODEWORDS: usize = 4;

/// Configuration with the given randomness for salts and masks. The prover must use fresh
/// randomness for every proof; the verifier never draws from it.
fn config(salt_rng: StdRng, mask_rng: StdRng) -> Config {
	let byte_hash = ByteHash {};
	let u64_hash = U64Hash::new(KeccakF {});
	let field_hash = FieldHash::new(u64_hash);
	let compress = Compress::new(u64_hash);
	let val_mmcs = ValMmcs::new(field_hash, compress, 0, salt_rng);
	let challenge_mmcs = ChallengeMmcs::new(val_mmcs.clone());
	let fri_params = FriParameters {
		log_blowup: LOG_BLOWUP,
		log_final_poly_len: 0,
		max_log_arity: 1,
		num_queries: NUM_QUERIES,
		batch_proof_of_work_bits: 0,
		commit_proof_of_work_bits: 0,
		query_proof_of_work_bits: QUERY_POW_BITS,
		mmcs: challenge_mmcs,
	};
	let pcs = Pcs::new(Dft::default(), val_mmcs, fri_params, NUM_RANDOM_CODEWORDS, mask_rng);
	let challenger = Challenger::from_hasher(alloc::vec::Vec::new(), byte_hash);
	Config::new(pcs, challenger)
}

/// Verifier configuration.
pub fn verifier_config() -> Config {
	config(StdRng::seed_from_u64(0), StdRng::seed_from_u64(0))
}

/// Prover configuration with fresh operating system randomness.
#[cfg(feature = "std")]
pub fn prover_config() -> Config {
	let mut seeds = [0u8; 64];
	getrandom::fill(&mut seeds).expect("the operating system RNG is available; qed");
	let salt_seed: [u8; 32] = seeds[..32].try_into().expect("32 bytes; qed");
	let mask_seed: [u8; 32] = seeds[32..].try_into().expect("32 bytes; qed");
	config(StdRng::from_seed(salt_seed), StdRng::from_seed(mask_seed))
}

/// Conjectured soundness of the configuration, in bits (ethSTARK conjecture).
pub const fn conjectured_security_bits() -> usize {
	LOG_BLOWUP * NUM_QUERIES + QUERY_POW_BITS
}
