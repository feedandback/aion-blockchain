// AION V1 merkezi protokol sabitleri.
//
// Konsensüs açısından kritik limit ve format değerleri
// tek noktadan yönetilir. Node, Blockchain ve Mempool
// bu değerleri doğrudan buradan kullanmalıdır.

pub const MAX_FUTURE_DRIFT_SECONDS: u64 = 120;

pub const MAX_MEMPOOL_TRANSACTIONS: usize = 10_000;

pub const MAX_NETWORK_PEERS: usize = 128;
pub const MAX_NETWORK_MESSAGE_HISTORY: usize = 10_000;
pub const MAX_NETWORK_INBOX_MESSAGES: usize = 10_000;
pub const MAX_PEER_ADDRESS_LENGTH: usize = 256;

// AION ağ kimliği.
// Farklı network_id kullanan node'lar birbirine bağlanmamalıdır.
#[allow(dead_code)]
pub const NETWORK_ID: &str = "aion-mainnet-v1";

// P2P mesaj protokolü sürümü.
#[allow(dead_code)]
pub const NETWORK_PROTOCOL_VERSION: u32 = 1;

// TCP bağlantı ve mesaj okuma/yazma işlemleri
// sonsuza kadar bekleyemez.
pub const NETWORK_CONNECT_TIMEOUT_SECONDS: u64 = 5;
pub const NETWORK_IO_TIMEOUT_SECONDS: u64 = 10;

// Aynı anda işlenebilecek maksimum TCP bağlantısı.
pub const MAX_CONCURRENT_NETWORK_CONNECTIONS: usize = 64;

// TCP üzerinden tek bir mesaj en fazla 8 MiB olabilir.
// Block sayısı sınırından bağımsız ek DoS korumasıdır.
pub const MAX_NETWORK_MESSAGE_BYTES: usize =
    8 * 1024 * 1024;

// Blockchain senkronizasyonu tek mesajda sınırsız büyümez.
// Zincir 256 blokluk parçalar halinde aktarılır.
pub const MAX_SYNC_BLOCKS_PER_MESSAGE: usize = 256;

pub const MAX_NORMAL_TRANSACTIONS_PER_BLOCK: usize = 1_000;
pub const MAX_TOTAL_TRANSACTIONS_PER_BLOCK: usize =
    MAX_NORMAL_TRANSACTIONS_PER_BLOCK + 1;

pub const HASH_HEX_LENGTH: usize = 64;
pub const ADDRESS_HEX_LENGTH: usize = 64;
pub const PUBLIC_KEY_HEX_LENGTH: usize = 64;
pub const SIGNATURE_HEX_LENGTH: usize = 128;

pub const SYSTEM_ADDRESS: &str = "SYSTEM";
pub const SYSTEM_PUBLIC_KEY: &str = "SYSTEM";
pub const SYSTEM_REWARD_SIGNATURE: &str =
    "SYSTEM_REWARD";

pub const GENESIS_PREVIOUS_HASH: &str = "0";
pub const GENESIS_VALIDATOR: &str = "GENESIS";

pub fn is_fixed_hex(
    value: &str,
    expected_length: usize,
) -> bool {
    value.len() == expected_length
        && value
            .bytes()
            .all(
                |byte| {
                    byte.is_ascii_hexdigit()
                },
            )
}