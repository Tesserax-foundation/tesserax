use super::*;

fn light_vm(key: &[u8]) -> Vm {
	let flags = Flags::recommended();
	Vm::light(flags, Arc::new(Cache::new(flags, key).unwrap())).unwrap()
}

#[test]
fn deterministic() {
	let mut vm = light_vm(b"tessera test key");
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
	let key = b"tessera test key";
	let mut jit = light_vm(key);
	let flags = Flags::portable();
	let mut interpreter =
		Vm::light(flags, Arc::new(Cache::new(flags, key).unwrap())).unwrap();
	for input in [&b""[..], b"a", b"tessera", &[0xffu8; 200][..]] {
		assert_eq!(jit.hash(input), interpreter.hash(input));
	}
}

/// Reference vectors. Any change to the RandomX sources or to the RandomTESS configuration
/// that alters these values is a consensus change.
#[test]
fn reference_vectors() {
	let mut vm = light_vm(b"Tessera RandomTESS test key");
	let vectors: [(&[u8], &str); 3] = [
		(b"", "28802c65211573e8a7a0e0c203a4c21864356532937f7b1df28c34c5f2c14a3f"),
		(b"Tessera", "5c42c03daac654cd8e845deb0f40c9e9a1fc0cc2b7082b20ee9bf31d8e5c396a"),
		(&[0x42; 40], "f5d6e9bc115782cc6f6bbd413b35bd0a788d2d50c1f2fa8199f15e788ed68fa4"),
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
	let cache = Arc::new(Cache::new(flags, b"tessera test key").unwrap());
	let mut light = Vm::light(flags, cache.clone()).unwrap();
	let dataset = Arc::new(Dataset::new(flags, &cache, 4).unwrap());
	let mut fast = Vm::fast(flags, dataset).unwrap();
	for nonce in 0u64..64 {
		let input = nonce.to_le_bytes();
		assert_eq!(fast.hash(&input), light.hash(&input));
	}
}
