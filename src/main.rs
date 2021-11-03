// --- std ---
use std::{
	fmt,
	fs::File,
	io::{Read, Write},
};
// --- crates.io ---
use blake2_rfc::blake2b;
use csv::Reader;
use parity_scale_codec::Encode;
use serde::Deserialize;
use serde_json::Value;
use subrpcer::client::u;
// --- github.com ---
use mmr::{
	helper,
	util::{MemMMR, MemStore},
	MMRStore, Merge,
};

fn offchain_key(pos: u64) -> String {
	const PREFIX: &[u8] = b"header-mmr-";

	let offchain_key = array_bytes::bytes2hex("0x", (PREFIX, pos).encode());

	// dbg!((pos, &offchain_key));

	offchain_key
}

pub struct Hasher;
impl Merge for Hasher {
	type Item = Hash;

	fn merge(lhs: &Self::Item, rhs: &Self::Item) -> Self::Item {
		pub fn hash(data: &[u8]) -> [u8; 32] {
			array_bytes::dyn2array!(blake2b::blake2b(32, &[], data).as_bytes(), 32)
		}

		let mut data = vec![];

		data.extend_from_slice(&lhs.0);
		data.extend_from_slice(&rhs.0);

		Hash(hash(&data))
	}
}

#[derive(Clone, PartialEq)]
pub struct Hash([u8; 32]);
impl From<[u8; 32]> for Hash {
	fn from(bytes: [u8; 32]) -> Self {
		Self(bytes)
	}
}
impl fmt::Display for Hash {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}", array_bytes::bytes2hex("0x", self.0))
	}
}
impl fmt::Debug for Hash {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		<Self as fmt::Display>::fmt(&self, f)
	}
}

#[derive(Debug, Deserialize)]
struct Record {
	block_number: u64,
	parent_mmr_root: String,
	hash: String,
}
impl Record {
	fn read_csv() -> Vec<Self> {
		let mut reader = Reader::from_path("data.csv").unwrap();
		let mut v = reader
			.deserialize::<Record>()
			.filter_map(|r| r.ok())
			.collect::<Vec<_>>();

		v.sort_by_key(|r| r.block_number);

		v
	}
}

fn get_node_from_rpc(uri: impl AsRef<str>, pos: u64) -> String {
	let k = offchain_key(pos);
	let uri = uri.as_ref();
	let rpc = subrpcer::rpc(
		0,
		"offchain_localStorageGet",
		serde_json::json!(["PERSISTENT", k]),
	);

	loop {
		if let Ok(response) = u::send_rpc(uri, &rpc) {
			let hash = response.into_json::<Value>().unwrap()["result"]
				.as_str()
				.unwrap()
				.to_string();

			// dbg!((pos, &hash));

			return hash;
		}
	}
}

fn insert_node_with_rpc(uri: impl AsRef<str>, pos: u64, hash: String) {
	let k = offchain_key(pos);
	let uri = uri.as_ref();
	let rpc = subrpcer::rpc(
		0,
		"offchain_localStorageSet",
		serde_json::json!(["PERSISTENT", k, hash]),
	);

	loop {
		if let Ok(response) = u::send_rpc(uri, &rpc) {
			let result = &response.into_json::<Value>().unwrap()["result"];

			dbg!(result);

			break;
		}
	}
}

fn build_mem_store(start_at: u64) -> MemStore<Hash> {
	let mem_store = MemStore::default();
	let mmr_size = mmr::leaf_index_to_mmr_size(start_at);
	let peaks = helper::get_peaks(mmr_size);

	for pos in peaks {
		let hash = get_node_from_rpc("http://localhost:20000", pos);
		let mut mem_store = mem_store.0.borrow_mut();

		mem_store.insert(pos, array_bytes::hex_into_unchecked(hash));
	}

	mem_store
}

fn read_nodes() -> Vec<(u64, String)> {
	let mut f = File::open("nodes").unwrap();
	let mut s = "".into();

	f.read_to_string(&mut s).unwrap();

	let mut v = vec![];

	for l in s.lines() {
		let (pos, hash) = l.split_once(":").unwrap();
		let pos = pos.parse().unwrap();

		v.push((pos, hash.into()));
	}

	v
}

fn write_nodes<S>(mmr_store: S, from: u64, to: u64)
where
	S: MMRStore<Hash>,
{
	let mut f = File::create("nodes").unwrap();

	for pos in from..=to {
		writeln!(f, "{}:{}", pos, mmr_store.get_elem(pos).unwrap().unwrap()).unwrap();
	}

	f.sync_all().unwrap();
}

#[allow(unused)]
fn gen_nodes() {
	let start_at = 4_999_999;
	let mem_store = build_mem_store(start_at);
	let mut mem_mmr = <MemMMR<Hash, Hasher>>::new(mmr::leaf_index_to_mmr_size(start_at), mem_store);
	let records = Record::read_csv();

	for Record {
		block_number,
		parent_mmr_root: expected_root,
		hash,
	} in records
	{
		let root = array_bytes::bytes2hex("", mem_mmr.get_root().unwrap().0);

		// dbg!((block_number, &expected_root, &root));
		assert_eq!(expected_root, root);

		mem_mmr.push(array_bytes::hex_into_unchecked(hash)).unwrap();
	}

	write_nodes(mem_mmr.store(), 11272187, 11403258);
}

fn check_nodes() {
	let nodes = read_nodes();

	for (pos, expected_hash) in nodes {
		let hash = get_node_from_rpc("http://localhost:20000", pos);

		if &expected_hash != &hash {
			dbg!((pos, &expected_hash, &hash));

			insert_node_with_rpc("http://localhost:20000", pos, expected_hash);
		}
	}
}

fn main() {
	check_nodes();
}
