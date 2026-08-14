use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use kybernetes::consensus::Consensus;
use kybernetes::validator::{ValidatorCandidateKeystore, ValidatorKeystore};

const PASSWORD: &str = "correct horse battery staple";
const WRONG_PASSWORD: &str = "wrong horse battery staple";
const GENESIS_FINGERPRINT: &str = "candidate-test-genesis-fingerprint";
static TEMP_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(label: &str) -> Self {
        let unique = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must follow unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kybernetes-candidate-{label}-{}-{timestamp}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("isolated test directory must be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let system_temp = std::env::temp_dir();
        if self.path.starts_with(&system_temp) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn consensus_with_address(address: &str) -> Consensus {
    let mut consensus = Consensus::new();
    assert!(consensus.add_validator(address.to_string(), 1));
    consensus
}

fn generate_candidate(directory: &Path, password: &str) -> (ValidatorCandidateKeystore, String) {
    let keystore = ValidatorCandidateKeystore::at(directory);
    let address = keystore
        .generate(password)
        .expect("candidate generation must succeed");
    (keystore, address)
}

fn run_candidate_address(directory: &Path, password: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kybernetes"))
        .args(["validator", "candidate-address"])
        .env("KYBERNETES_DATA_DIR", directory)
        .env("KYBERNETES_VALIDATOR_PASSWORD", password)
        .output()
        .expect("candidate-address command must run")
}

#[test]
fn candidate_generation_succeeds() {
    let directory = TestDirectory::new("generate");
    let (keystore, address) = generate_candidate(directory.path(), PASSWORD);

    assert_eq!(address.len(), 64);
    assert!(hex::decode(&address).is_ok());
    assert!(
        keystore
            .exists()
            .expect("candidate path must be inspectable")
    );
    assert_eq!(
        keystore.path(),
        directory.path().join("validator-candidate.json")
    );
}

#[test]
fn candidate_private_key_is_not_stored_as_plaintext() {
    let directory = TestDirectory::new("encrypted-payload");
    let (keystore, address) = generate_candidate(directory.path(), PASSWORD);
    let contents =
        std::fs::read_to_string(keystore.path()).expect("candidate envelope must be readable");
    let envelope: serde_json::Value =
        serde_json::from_str(&contents).expect("candidate envelope must be valid JSON");

    assert!(!contents.contains("private_key_hex"));
    assert!(!contents.contains("kybernetes-validator-candidate-v1"));
    assert!(!contents.contains(&address));
    assert_eq!(envelope["version"], 1);
    assert_eq!(envelope["kdf"], "argon2id");
    assert_eq!(envelope["cipher"], "chacha20poly1305");
    assert!(
        envelope["ciphertext_hex"]
            .as_str()
            .is_some_and(|ciphertext| !ciphertext.is_empty())
    );
    let mut envelope_fields = envelope
        .as_object()
        .expect("candidate envelope must be a JSON object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    envelope_fields.sort_unstable();
    assert_eq!(
        envelope_fields,
        [
            "cipher",
            "ciphertext_hex",
            "kdf",
            "kdf_iterations",
            "kdf_memory_kib",
            "kdf_parallelism",
            "nonce_hex",
            "salt_hex",
            "version",
        ]
    );
}

#[test]
fn candidate_opens_with_the_correct_password() {
    let directory = TestDirectory::new("correct-password");
    let (keystore, address) = generate_candidate(directory.path(), PASSWORD);
    let candidate = keystore
        .load(PASSWORD)
        .expect("candidate must decrypt")
        .expect("candidate must exist");

    assert_eq!(candidate.address(), address);
}

#[test]
fn candidate_address_command_returns_only_the_public_address() {
    let directory = TestDirectory::new("address-command");
    let (_, address) = generate_candidate(directory.path(), PASSWORD);

    let output = run_candidate_address(directory.path(), PASSWORD);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("command output must be UTF-8")
            .replace("\r\n", "\n"),
        format!("Candidate validator address: {address}\n")
    );
    assert!(output.stderr.is_empty());
    assert!(
        !ValidatorKeystore::at(directory.path())
            .exists()
            .expect("active keystore path must be inspectable")
    );
}

#[test]
fn candidate_address_command_rejects_the_wrong_password() {
    let directory = TestDirectory::new("address-command-wrong-password");
    let (keystore, _) = generate_candidate(directory.path(), PASSWORD);
    let original = std::fs::read(keystore.path()).expect("candidate must be readable");

    let output = run_candidate_address(directory.path(), WRONG_PASSWORD);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr)
            .expect("command error output must be UTF-8")
            .replace("\r\n", "\n"),
        "Candidate validator address could not be read\n"
    );
    assert_eq!(
        std::fs::read(keystore.path()).expect("candidate must remain readable"),
        original
    );
    assert!(
        !ValidatorKeystore::at(directory.path())
            .exists()
            .expect("active keystore path must be inspectable")
    );
}

#[test]
fn candidate_address_command_keeps_the_candidate_byte_for_byte_unchanged() {
    let directory = TestDirectory::new("address-command-read-only");
    let (keystore, _) = generate_candidate(directory.path(), PASSWORD);
    let original = std::fs::read(keystore.path()).expect("candidate must be readable");

    let output = run_candidate_address(directory.path(), PASSWORD);

    assert!(output.status.success());
    assert_eq!(
        std::fs::read(keystore.path()).expect("candidate must remain readable"),
        original
    );
    assert!(
        !ValidatorKeystore::at(directory.path())
            .exists()
            .expect("active keystore path must be inspectable")
    );
}

#[test]
fn candidate_rejects_the_wrong_password() {
    let directory = TestDirectory::new("wrong-password");
    let (keystore, _) = generate_candidate(directory.path(), PASSWORD);

    assert!(keystore.load(WRONG_PASSWORD).is_err());
    assert!(
        keystore
            .exists()
            .expect("candidate must remain inspectable")
    );
}

#[test]
fn second_candidate_generation_does_not_overwrite_the_first() {
    let directory = TestDirectory::new("generate-no-overwrite");
    let (keystore, first_address) = generate_candidate(directory.path(), PASSWORD);
    let original = std::fs::read(keystore.path()).expect("first candidate must be readable");

    assert!(keystore.generate(PASSWORD).is_err());
    assert_eq!(
        std::fs::read(keystore.path()).expect("first candidate must remain readable"),
        original
    );
    let candidate = keystore
        .load(PASSWORD)
        .expect("first candidate must still decrypt")
        .expect("first candidate must still exist");
    assert_eq!(candidate.address(), first_address);
}

#[test]
fn noncanonical_candidate_cannot_be_activated() {
    let directory = TestDirectory::new("noncanonical-activation");
    let (candidate_keystore, _) = generate_candidate(directory.path(), PASSWORD);
    let original = std::fs::read(candidate_keystore.path()).expect("candidate must be readable");
    let consensus = Consensus::new();

    assert!(
        candidate_keystore
            .activate(PASSWORD, &consensus, GENESIS_FINGERPRINT)
            .is_err()
    );
    assert_eq!(
        std::fs::read(candidate_keystore.path()).expect("candidate must be preserved"),
        original
    );
    assert!(
        !ValidatorKeystore::at(directory.path())
            .exists()
            .expect("active path must be inspectable")
    );
}

#[test]
fn canonical_candidate_is_converted_to_an_active_keystore() {
    let directory = TestDirectory::new("successful-activation");
    let (candidate_keystore, address) = generate_candidate(directory.path(), PASSWORD);
    let consensus = consensus_with_address(&address);

    let activation = candidate_keystore
        .activate(PASSWORD, &consensus, GENESIS_FINGERPRINT)
        .expect("canonical candidate must activate");
    let active_keystore = ValidatorKeystore::at(directory.path());
    let identity = active_keystore
        .load_authorized(PASSWORD, &consensus, GENESIS_FINGERPRINT)
        .expect("active keystore must decrypt")
        .expect("active identity must exist");

    assert_eq!(activation.address(), address);
    assert!(activation.candidate_removed());
    assert_eq!(identity.address(), address);
    assert!(
        !candidate_keystore
            .exists()
            .expect("candidate path must be inspectable")
    );
    assert!(
        active_keystore
            .exists()
            .expect("active path must be inspectable")
    );
}

#[test]
fn active_keystore_is_bound_to_the_activation_genesis_fingerprint() {
    let directory = TestDirectory::new("fingerprint-binding");
    let (candidate_keystore, address) = generate_candidate(directory.path(), PASSWORD);
    let consensus = consensus_with_address(&address);
    candidate_keystore
        .activate(PASSWORD, &consensus, GENESIS_FINGERPRINT)
        .expect("canonical candidate must activate");
    let active_keystore = ValidatorKeystore::at(directory.path());

    assert!(
        active_keystore
            .load_authorized(PASSWORD, &consensus, GENESIS_FINGERPRINT)
            .expect("matching fingerprint must be accepted")
            .is_some()
    );
    assert!(
        active_keystore
            .load_authorized(PASSWORD, &consensus, "different-genesis-fingerprint")
            .is_err()
    );
}

#[test]
fn activation_does_not_overwrite_an_existing_active_keystore() {
    let directory = TestDirectory::new("active-no-overwrite");
    let (candidate_keystore, first_address) = generate_candidate(directory.path(), PASSWORD);
    let first_consensus = consensus_with_address(&first_address);
    candidate_keystore
        .activate(PASSWORD, &first_consensus, GENESIS_FINGERPRINT)
        .expect("first candidate must activate");
    let active_keystore = ValidatorKeystore::at(directory.path());
    let original_active =
        std::fs::read(active_keystore.path()).expect("active keystore must be readable");

    let second_address = candidate_keystore
        .generate(PASSWORD)
        .expect("a new pending candidate may be generated");
    let mut both_validators = Consensus::new();
    assert!(both_validators.add_validator(first_address, 1));
    assert!(both_validators.add_validator(second_address, 1));

    assert!(
        candidate_keystore
            .activate(PASSWORD, &both_validators, GENESIS_FINGERPRINT)
            .is_err()
    );
    assert_eq!(
        std::fs::read(active_keystore.path()).expect("active keystore must remain readable"),
        original_active
    );
    assert!(
        candidate_keystore
            .exists()
            .expect("second candidate must be preserved")
    );
}

#[test]
fn failed_activation_preserves_the_candidate() {
    let directory = TestDirectory::new("failed-activation-preserves");
    let (candidate_keystore, address) = generate_candidate(directory.path(), PASSWORD);
    let consensus = consensus_with_address(&address);
    let original = std::fs::read(candidate_keystore.path()).expect("candidate must be readable");

    assert!(
        candidate_keystore
            .activate(WRONG_PASSWORD, &consensus, GENESIS_FINGERPRINT)
            .is_err()
    );
    assert_eq!(
        std::fs::read(candidate_keystore.path()).expect("candidate must be preserved"),
        original
    );
    assert!(
        !ValidatorKeystore::at(directory.path())
            .exists()
            .expect("active path must be inspectable")
    );
}

#[test]
fn candidate_keystores_are_isolated_between_data_directories() {
    let root = TestDirectory::new("directory-isolation");
    let directory_a = root.path().join("node-a");
    let directory_b = root.path().join("node-b");
    let (keystore_a, address_a) = generate_candidate(&directory_a, PASSWORD);
    let (keystore_b, address_b) = generate_candidate(&directory_b, WRONG_PASSWORD);

    assert_ne!(keystore_a.path(), keystore_b.path());
    assert_ne!(address_a, address_b);
    assert_eq!(
        keystore_a
            .load(PASSWORD)
            .expect("node A candidate must decrypt")
            .expect("node A candidate must exist")
            .address(),
        address_a
    );
    assert_eq!(
        keystore_b
            .load(WRONG_PASSWORD)
            .expect("node B candidate must decrypt")
            .expect("node B candidate must exist")
            .address(),
        address_b
    );
    assert!(keystore_a.load(WRONG_PASSWORD).is_err());
    assert!(keystore_b.load(PASSWORD).is_err());
}
