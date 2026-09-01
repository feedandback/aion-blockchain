# Kybernetes

Kybernetes is an experimental independent blockchain implementation written in Rust.

> Current release: **v0.1.0**

## Status

Kybernetes is currently an experimental network implementation.

It is not yet intended for production use with real financial value.

## Features

- Independent blockchain core
- SHA-256 block hashing
- Ed25519 transaction signatures
- Native Kybernetes wallet
- Encrypted local wallet keystore
- Account balance and nonce tracking
- Transaction fees
- Validator support
- Stake-weighted validator selection
- Blockchain persistence
- TCP peer-to-peer networking
- Authenticated peer handshake
- Transaction submission over the network
- Live wallet balance and nonce queries from a running node
- Automated tests
- Automated Windows release builds
- SHA-256 release checksums
- GitHub build provenance attestations

## Download

Pre-built Windows x86_64 releases are available from the GitHub Releases page.

Current release:

`v0.1.0`

The Windows package contains:

- `kybernetes.exe`
- `SHA256SUMS.txt`
- `RELEASE_INFO.txt`

Always verify release checksums before running downloaded binaries.

## Command Line

```text
kybernetes [node [listen_address] [peer...]]
kybernetes wallet create
kybernetes wallet address
kybernetes wallet balance <peer_address>
kybernetes wallet send <peer_address> <recipient_address> <amount_microkbn>
kybernetes transaction submit <peer_address> <recipient_address> <amount_microkbn>
kybernetes validator generate-candidate
kybernetes validator candidate-address
kybernetes validator activate-candidate
kybernetes provision-validator
kybernetes demo
```

## Create a Wallet

First, choose a data directory:

```powershell
$env:KYBERNETES_DATA_DIR = "$PWD\wallet-data"
```

Enter the wallet password securely and create the wallet:

```powershell
$secure = Read-Host "Wallet password" -AsSecureString
$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)

try {
    $env:KYBERNETES_WALLET_PASSWORD =
        [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)

    .\kybernetes.exe wallet create
}
finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr)
    Remove-Item Env:\KYBERNETES_WALLET_PASSWORD -ErrorAction SilentlyContinue
}
```

The wallet is stored in an encrypted local keystore.

Never share your wallet password, private key, or keystore contents.

## Show Wallet Address

Enter the wallet password securely:

```powershell
$secure = Read-Host "Wallet password" -AsSecureString
$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)

try {
    $env:KYBERNETES_WALLET_PASSWORD =
        [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)

    .\kybernetes.exe wallet address
}
finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr)
    Remove-Item Env:\KYBERNETES_WALLET_PASSWORD -ErrorAction SilentlyContinue
}
```

## Send a Transaction

Enter the wallet password securely and submit the transaction through a running Kybernetes node:

```powershell
$secure = Read-Host "Wallet password" -AsSecureString
$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)

try {
    $env:KYBERNETES_WALLET_PASSWORD =
        [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)

    .\kybernetes.exe wallet send 127.0.0.1:7401 <recipient_address> <amount_microkbn>
}
finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr)
    Remove-Item Env:\KYBERNETES_WALLET_PASSWORD -ErrorAction SilentlyContinue
}
```

Transaction amounts are currently expressed in `microKBN`.

The sender must have enough balance to cover both the transaction amount and the network fee.

## Query Wallet State

Enter the wallet password securely and query a running Kybernetes node:

```powershell
$secure = Read-Host "Wallet password" -AsSecureString
$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)

try {
    $env:KYBERNETES_WALLET_PASSWORD =
        [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)

    .\kybernetes.exe wallet balance 127.0.0.1:7401
}
finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr)
    Remove-Item Env:\KYBERNETES_WALLET_PASSWORD -ErrorAction SilentlyContinue
}
```

The response includes the wallet address, balance, nonce, node tip index, and node tip hash.

The queried node is a source of account-state information. This is not currently a trustless light-client proof system.

## Run a Node

Start a node listening on a local address:

```powershell
.\kybernetes.exe node 127.0.0.1:7401
```

Connect another node to it:

```powershell
.\kybernetes.exe node 127.0.0.1:7402 127.0.0.1:7401
```

A running node is a long-lived process, so the terminal normally remains occupied until the node is stopped.


## Build From Source

Requirements:

- Rust stable
- Cargo
- Git

Clone the repository:

```powershell
git clone https://github.com/feedandback/aion-blockchain.git
cd aion-blockchain
```

Run the checks:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo test --locked --release
```

Build the release binary:

```powershell
cargo build --release --locked
```

The executable will be created at:

```text
target\release\kybernetes.exe
```

## Network

Current network identifier:

```text
kybernetes-mainnet-v2
```

Current protocol version:

```text
2
```

Kybernetes currently supports observer/full node operation and validator-related commands.

## Security

Kybernetes uses cryptographic signatures and encrypted wallet storage, but the project is still under active development.

Before production use, additional security hardening, external security review, operational testing, and network-level validation are required.

Do not use Kybernetes to store assets of real-world financial value until the project has completed additional security review and production validation.

## Releases

Kybernetes releases are automatically built and tested with GitHub Actions.

The release workflow performs:

- Formatting checks
- Strict Clippy checks
- Release tests
- Dependency security audit
- Release build
- SHA-256 checksum generation
- Binary and archive attestations
- Release archive verification
- Provenance verification

## License

Kybernetes is licensed under the Apache License 2.0. See [LICENSE](LICENSE) for details.
