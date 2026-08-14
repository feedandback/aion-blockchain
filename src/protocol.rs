// Kybernetes V2 central protocol constants.
//
// Consensus-critical limits and format values
// are managed from a single location. Node, Blockchain, and Mempool
// must use these values directly from here.

pub const MAX_FUTURE_DRIFT_SECONDS: u64 = 120;

pub const MAX_MEMPOOL_TRANSACTIONS: usize = 10_000;

pub const MAX_NETWORK_PEERS: usize = 128;
pub const MAX_NETWORK_MESSAGE_HISTORY: usize = 10_000;
pub const MAX_NETWORK_INBOX_MESSAGES: usize = 10_000;
pub const MAX_PEER_ADDRESS_LENGTH: usize = 256;

// Kybernetes network identity.
// Nodes using different network_id values must not connect to each other.
#[allow(dead_code)]
pub const NETWORK_ID: &str = "kybernetes-mainnet-v2";

// P2P message protocol version.
#[allow(dead_code)]
pub const NETWORK_PROTOCOL_VERSION: u32 = 2;

// TCP connection and message read/write operations
// must not wait forever.
pub const NETWORK_CONNECT_TIMEOUT_SECONDS: u64 = 5;
pub const NETWORK_IO_TIMEOUT_SECONDS: u64 = 10;

// Maximum number of concurrent TCP connections.
pub const MAX_CONCURRENT_NETWORK_CONNECTIONS: usize = 64;

// Stale handshake messages are rejected.
// Time window for replay-attack protection.
pub const MAX_HANDSHAKE_AGE_SECONDS: u64 = 120;

// A single message over TCP may be at most 8 MiB.
// This is additional DoS protection independent of the block-count limit.
pub const MAX_NETWORK_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

// Blockchain synchronization does not grow without bound in a single message.
// The chain is transferred in chunks of 256 blocks.
pub const MAX_SYNC_BLOCKS_PER_MESSAGE: usize = 256;

pub const MAX_NORMAL_TRANSACTIONS_PER_BLOCK: usize = 1_000;
pub const MAX_TOTAL_TRANSACTIONS_PER_BLOCK: usize = MAX_NORMAL_TRANSACTIONS_PER_BLOCK + 1;

pub const HASH_HEX_LENGTH: usize = 64;
pub const ADDRESS_HEX_LENGTH: usize = 64;
pub const PUBLIC_KEY_HEX_LENGTH: usize = 64;
pub const SIGNATURE_HEX_LENGTH: usize = 128;

pub const SYSTEM_ADDRESS: &str = "SYSTEM";
pub const SYSTEM_PUBLIC_KEY: &str = "SYSTEM";
pub const SYSTEM_REWARD_SIGNATURE: &str = "SYSTEM_REWARD";

pub const GENESIS_PREVIOUS_HASH: &str = "0";
pub const GENESIS_VALIDATOR: &str = "GENESIS";

// Fixed genesis configuration of the Kybernetes network.
// Every independent node must validate the same genesis state and validator set
// without sharing private keys.
pub const GENESIS_TIMESTAMP: u64 = 1_754_690_000;
pub const GENESIS_SUPPLY_MICRO_KBN: u64 = 1_000_000_000;

pub const GENESIS_VALIDATOR_A_ADDRESS: &str =
    "66e0a6e68b5d5f2f01c42d4f5bcc32ff475917d29a84891a564281d06dac0194";
pub const GENESIS_VALIDATOR_A_STAKE: u64 = 700;
pub const GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN: u64 = GENESIS_SUPPLY_MICRO_KBN;

pub const GENESIS_VALIDATOR_B_ADDRESS: &str =
    "fe36b693330e3e48ae0ed1a78f92e38a99eb228bc3d3ab617ac70d75b28f5e78";
pub const GENESIS_VALIDATOR_B_STAKE: u64 = 300;
pub const GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN: u64 = 0;

// Kybernetes V2 consensus economy parameters.
// Economy and the canonical genesis fingerprint use the same constants.
pub const MAX_SUPPLY_MICRO_KBN: u64 = 100_000_000 * 1_000_000;
pub const BLOCK_REWARD_MICRO_KBN: u64 = 10 * 1_000_000;
pub const MIN_TRANSACTION_FEE_MICRO_KBN: u64 = 10;
pub const TRANSACTION_FEE_DIVISOR: u64 = 100_000;
pub const VALIDATOR_FEE_PERCENT: u64 = 15;
pub const LIQUIDITY_RESERVE_FEE_PERCENT: u64 = 80;
pub const TREASURY_FEE_PERCENT: u64 = 5;
pub const BURN_FEE_PERCENT: u64 = 0;

pub fn is_fixed_hex(value: &str, expected_length: usize) -> bool {
    value.len() == expected_length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
