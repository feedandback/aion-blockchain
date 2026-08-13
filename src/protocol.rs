// Kybernetes V2 merkezi protokol sabitleri.
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

// Kybernetes ağ kimliği.
// Farklı network_id kullanan node'lar birbirine bağlanmamalıdır.
#[allow(dead_code)]
pub const NETWORK_ID: &str = "kybernetes-mainnet-v2";

// P2P mesaj protokolü sürümü.
#[allow(dead_code)]
pub const NETWORK_PROTOCOL_VERSION: u32 = 2;

// TCP bağlantı ve mesaj okuma/yazma işlemleri
// sonsuza kadar bekleyemez.
pub const NETWORK_CONNECT_TIMEOUT_SECONDS: u64 = 5;
pub const NETWORK_IO_TIMEOUT_SECONDS: u64 = 10;

// Aynı anda işlenebilecek maksimum TCP bağlantısı.
pub const MAX_CONCURRENT_NETWORK_CONNECTIONS: usize = 64;

// Handshake mesajları eskiyse kabul edilmez.
// Replay saldırılarına karşı zaman penceresi.
pub const MAX_HANDSHAKE_AGE_SECONDS: u64 = 120;

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

// Kybernetes ağının sabit genesis yapılandırması.
// Her bağımsız node aynı genesis state ve validator setini
// private key paylaşmadan doğrulayabilmelidir.
pub const GENESIS_TIMESTAMP: u64 = 1_754_690_000;
pub const GENESIS_SUPPLY_MICRO_KBN: u64 =
    1_000_000_000;

pub const GENESIS_VALIDATOR_A_ADDRESS: &str =
    "e78e5a3f52365b555c495371141c05e5992b5f786dd526c778af16dfb8cf822b";
pub const GENESIS_VALIDATOR_A_STAKE: u64 = 700;
pub const GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN: u64 =
    GENESIS_SUPPLY_MICRO_KBN;

pub const GENESIS_VALIDATOR_B_ADDRESS: &str =
    "a3c7e0f73b41bc841f8cefe6ff0d43fc24aa259eb4f90db4b408fdc3f4eb5fb4";
pub const GENESIS_VALIDATOR_B_STAKE: u64 = 300;
pub const GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN: u64 = 0;

// Kybernetes V2 consensus ekonomi parametreleri.
// Economy ve canonical genesis fingerprint aynı sabitleri kullanır.
pub const MAX_SUPPLY_MICRO_KBN: u64 =
    100_000_000 * 1_000_000;
pub const BLOCK_REWARD_MICRO_KBN: u64 =
    10 * 1_000_000;
pub const MIN_TRANSACTION_FEE_MICRO_KBN: u64 = 10;
pub const TRANSACTION_FEE_DIVISOR: u64 = 100_000;
pub const VALIDATOR_FEE_PERCENT: u64 = 15;
pub const LIQUIDITY_RESERVE_FEE_PERCENT: u64 = 80;
pub const TREASURY_FEE_PERCENT: u64 = 5;
pub const BURN_FEE_PERCENT: u64 = 0;

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
