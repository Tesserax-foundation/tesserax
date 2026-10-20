use super::*;

fn light_vm(key: &[u8]) -> Vm {
	let flags = Flags::recommended();
	Vm::light(flags, Arc::new(Cache::new(flags, key).unwrap())).unwrap()
}

#[test]
fn deterministic() {
	let mut vm = light_vm(b"tesserax test key");
	assert_eq!(vm.hash(b"input"), vm.hash(b"input"));
	assert_ne!(vm.hash(b"input"), vm.hash(b"input2"));
}

#[test]
fn key_changes_hash() {
	let mut a = light_vm(b"key a");
	let mut b = light_vm(b"key b");
	assert_ne!(a.hash(b"input"), b.hash(b"input"));
}

#[test]
fn jit_matches_interpreter() {
	let key = b"tesserax test key";
	let mut jit = light_vm(key);
	let flags = Flags::portable();
	let mut interpreter =
		Vm::light(flags, Arc::new(Cache::new(flags, key).unwrap())).unwrap();
	for input in [&b""[..], b"a", b"tesserax", &[0xffu8; 200][..]] {
		assert_eq!(jit.hash(input), interpreter.hash(input));
	}
}

/// Reference vectors. Any change to the RandomX sources or to the RandomTESS configuration
/// that alters these values is a consensus change.
#[test]
fn reference_vectors() {
	let mut vm = light_vm(b"Tesserax RandomTESS test key");
	let vectors: [(&[u8], &str); 3] = [
		(b"", "90ca8996fd84a4fd7bef450ea9b04488bafc900a3ed195bac2dc5f2282fb329a"),
		(b"Tesserax", "365e9fdb82a9e7573168c0f432380c3249dbbccf5d4b356d467427bcc9deb277"),
		(&[0x42; 40], "7822da42b546814b1833614527604ae50c3aac2553680913c2fa0cd5bb88e76a"),
	];
	for (input, expected) in vectors {
		assert_eq!(hex::encode(vm.hash(input)), expected);
	}
}

/// Fast mode allocates and initializes the full 2 GiB dataset.
#[test]
#[ignore]
fn fast_matches_light() {
	let flags = Flags::recommended();
	let cache = Arc::new(Cache::new(flags, b"tesserax test key").unwrap());
	let mut light = Vm::light(flags, cache.clone()).unwrap();
	let dataset = Arc::new(Dataset::new(flags, &cache, 4).unwrap());
	let mut fast = Vm::fast(flags, dataset).unwrap();
	for nonce in 0u64..64 {
		let input = nonce.to_le_bytes();
		assert_eq!(fast.hash(&input), light.hash(&input));
	}
}
