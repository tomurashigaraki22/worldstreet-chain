use clap::{Args, Parser, Subcommand};
use std::{
    env, fs,
    io::{self, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use wsc_core::{Address, GenesisConfig, Validator, NATIVE_ASSET_NAME, NATIVE_ASSET_SYMBOL};
use wsc_crypto::KeyPair;
use wsc_node::{Node, NodeConfig};
use wsc_wallet::{EncryptedKeystore, Wallet};

#[derive(Debug, Parser)]
#[command(name = "wsc", version, about = "Worldstreet Chain wallet and node CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    Wallet {
        #[command(subcommand)]
        command: WalletCommand,
    },
    Devnet {
        #[command(subcommand)]
        command: DevnetCommand,
    },
}

#[derive(Debug, Subcommand)]
enum NodeCommand {
    Init(NodeInitArgs),
    Start(NodeStartArgs),
    Network(NodeNetworkArgs),
    Status(NodeDataDirArgs),
    Faucet(NodeFaucetArgs),
}

#[derive(Debug, Args)]
struct NodeInitArgs {
    #[arg(long, default_value = ".wsc/node")]
    data_dir: PathBuf,
    #[arg(long, default_value = "worldstreet-devnet-1")]
    chain_id: String,
}

#[derive(Debug, Args)]
struct NodeStartArgs {
    #[command(flatten)]
    data: NodeDataDirArgs,
    #[arg(long)]
    once: bool,
    #[arg(long, default_value = "127.0.0.1:26657")]
    rpc_bind: String,
    #[arg(long)]
    no_rpc: bool,
    #[arg(long)]
    p2p_bind: Option<SocketAddr>,
    #[arg(long = "peer")]
    peers: Vec<String>,
    #[arg(long, default_value = "wsc-node")]
    node_id: String,
    #[arg(long)]
    validator_secret_env: Option<String>,
}

#[derive(Debug, Subcommand)]
enum DevnetCommand {
    Init(DevnetInitArgs),
}

#[derive(Debug, Args)]
struct DevnetInitArgs {
    #[arg(long, default_value = "devnet/data")]
    root: PathBuf,
    #[arg(long, default_value = "worldstreet-devnet-1")]
    chain_id: String,
}

#[derive(Debug, Args)]
struct NodeNetworkArgs {
    #[command(flatten)]
    data: NodeDataDirArgs,
    #[arg(long, default_value = "0.0.0.0:26656")]
    listen: SocketAddr,
    #[arg(long = "peer")]
    peers: Vec<String>,
    #[arg(long, default_value = "wsc-node")]
    node_id: String,
}

#[derive(Debug, Args)]
struct NodeDataDirArgs {
    #[arg(long, default_value = ".wsc/node")]
    data_dir: PathBuf,
}

#[derive(Debug, Args)]
struct NodeFaucetArgs {
    #[command(flatten)]
    data: NodeDataDirArgs,
    #[arg(long)]
    address: Address,
    #[arg(long)]
    amount: u128,
}

#[derive(Debug, Subcommand)]
enum WalletCommand {
    Create(WalletPathArgs),
    Restore(RestoreArgs),
    Address(WalletPathArgs),
    SignMessage(SignMessageArgs),
}

#[derive(Debug, Args)]
struct WalletPathArgs {
    #[arg(long)]
    keystore: Option<PathBuf>,
    #[arg(long)]
    password_env: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RestoreArgs {
    #[command(flatten)]
    path: WalletPathArgs,
    #[arg(long)]
    mnemonic: Option<String>,
    #[arg(long)]
    mnemonic_file: Option<PathBuf>,
    #[arg(long)]
    mnemonic_passphrase_env: Option<String>,
}

#[derive(Debug, Args)]
struct SignMessageArgs {
    #[command(flatten)]
    path: WalletPathArgs,
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    message_file: Option<PathBuf>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!(
                "wsc {} | {NATIVE_ASSET_NAME} ({NATIVE_ASSET_SYMBOL})",
                env!("CARGO_PKG_VERSION")
            );
        }
        Command::Node { command } => match command {
            NodeCommand::Init(args) => {
                let config = Node::init(args.data_dir, args.chain_id)?;
                println!("Node initialized at {}", config.data_dir.display());
                println!("Genesis: {}", config.genesis_path.display());
            }
            NodeCommand::Start(args) => {
                let config = NodeConfig::load(args.data.data_dir)?;
                let mut node = Node::open(config)?;
                let proposer = load_validator_key(args.validator_secret_env.as_deref())?;
                if args.once || args.no_rpc {
                    node.run_with_proposer(args.once, proposer.as_ref())?;
                } else {
                    let bind = args.rpc_bind.parse()?;
                    if let Some(p2p_bind) = args.p2p_bind {
                        let network = wsc_network::NetworkConfig {
                            chain_id: node.config().chain_id.clone(),
                            listen_addr: p2p_bind,
                            peers: args.peers,
                            node_id: args.node_id,
                        };
                        wsc_rpc::run_with_network(node, bind, network, proposer)?;
                    } else {
                        wsc_rpc::run_with_proposer(node, bind, proposer)?;
                    }
                }
            }
            NodeCommand::Network(args) => {
                let config = NodeConfig::load(args.data.data_dir)?;
                let node = Node::open(config.clone())?;
                let network = wsc_network::NetworkConfig {
                    chain_id: config.chain_id,
                    listen_addr: args.listen,
                    peers: args.peers,
                    node_id: args.node_id,
                };
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(wsc_network::run(Arc::new(Mutex::new(node)), network))?;
            }
            NodeCommand::Status(args) => {
                let config = NodeConfig::load(args.data_dir)?;
                let node = Node::open(config)?;
                println!("chain_id={}", node.genesis().chain_id);
                println!("height={}", node.latest_block().header.height);
                println!(
                    "block_hash={}",
                    wsc_node::block_id_for(node.latest_block())?
                );
                println!("finalized_height={}", node.finalized_height()?);
                println!("finalized_hash={}", node.finalized_hash()?);
                println!("mempool={}", node.mempool_len());
            }
            NodeCommand::Faucet(args) => {
                let config = NodeConfig::load(args.data.data_dir)?;
                let mut node = Node::open(config)?;
                let block = node.faucet(args.address, args.amount)?;
                println!("faucet_block_height={}", block.header.height);
                println!("address={}", args.address);
                println!("amount_microMNA={}", args.amount);
            }
        },
        Command::Devnet { command } => match command {
            DevnetCommand::Init(args) => init_devnet(args)?,
        },
        Command::Wallet { command } => match command {
            WalletCommand::Create(args) => create_wallet(args)?,
            WalletCommand::Restore(args) => restore_wallet(args)?,
            WalletCommand::Address(args) => {
                let wallet = load_wallet(&args)?;
                println!("{}", wallet.address());
            }
            WalletCommand::SignMessage(args) => sign_message(args)?,
        },
    }
    Ok(())
}

fn load_validator_key(
    env_name: Option<&str>,
) -> Result<Option<KeyPair>, Box<dyn std::error::Error>> {
    let Some(env_name) = env_name else {
        return Ok(None);
    };
    let value = env::var(env_name)?;
    let bytes = hex::decode(value.trim())?;
    if bytes.len() != 32 {
        return Err("validator secret must be exactly 32 bytes of hex".into());
    }
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&bytes);
    Ok(Some(KeyPair::from_secret_bytes(secret)))
}

fn init_devnet(args: DevnetInitArgs) -> Result<(), Box<dyn std::error::Error>> {
    const SECRETS: [[u8; 32]; 4] = [[1; 32], [2; 32], [3; 32], [4; 32]];
    let genesis_path = args.root.join("genesis.json");
    if genesis_path.exists() {
        let genesis: GenesisConfig = serde_json::from_str(&fs::read_to_string(&genesis_path)?)?;
        if genesis.chain_id != args.chain_id {
            return Err(format!(
                "existing devnet at {} has chain ID {} instead of {}",
                args.root.display(),
                genesis.chain_id,
                args.chain_id
            )
            .into());
        }
        for index in 1..=SECRETS.len() {
            NodeConfig::load(args.root.join(format!("node-{index}")))?;
        }
        if !args.root.join("validators.env").is_file() {
            return Err(format!(
                "existing devnet at {} is incomplete: validators.env is missing",
                args.root.display()
            )
            .into());
        }
        println!(
            "Devnet already initialized at {}; reusing existing genesis and node data.",
            args.root.display()
        );
        return Ok(());
    }
    let validators = SECRETS
        .iter()
        .enumerate()
        .map(|(index, secret)| Validator {
            name: format!("validator-{}", index + 1),
            public_key: KeyPair::from_secret_bytes(*secret).public_key(),
        })
        .collect::<Vec<_>>();
    let genesis = GenesisConfig {
        version: 1,
        chain_id: args.chain_id.clone(),
        genesis_time: 1_700_000_000,
        block_time_ms: 2000,
        initial_supply: 0,
        fee_minimum: 1,
        validators,
        allocations: vec![],
        assets: vec![],
    };
    fs::create_dir_all(&args.root)?;
    let genesis_json = serde_json::to_string_pretty(&genesis)?;
    fs::write(&genesis_path, format!("{genesis_json}\n"))?;
    let mut env_file = String::new();
    for (index, secret) in SECRETS.iter().enumerate() {
        let node_dir = args.root.join(format!("node-{}", index + 1));
        let config = Node::init(&node_dir, &args.chain_id)?;
        fs::write(&config.genesis_path, format!("{genesis_json}\n"))?;
        env_file.push_str(&format!(
            "WSC_VALIDATOR_{}={}\n",
            index + 1,
            hex::encode(secret)
        ));
    }
    fs::write(args.root.join("validators.env"), env_file)?;
    println!("Devnet initialized at {}", args.root.display());
    println!("Genesis: {}", genesis_path.display());
    println!("Deterministic validator secrets written to validators.env for devnet testing only.");
    Ok(())
}

fn create_wallet(args: WalletPathArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_path(args.keystore)?;
    if path.exists() {
        return Err(format!("keystore already exists: {}", path.display()).into());
    }
    let password = read_password(args.password_env.as_deref(), "Keystore password: ")?;
    let (wallet, mnemonic) = Wallet::create()?;
    let keystore = wallet.save_encrypted(&password)?;
    write_keystore(&path, &keystore)?;
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "address": wallet.address().to_string(),
                "public_key": hex::encode(wallet.public_key().0),
                "mnemonic": mnemonic,
                "keystore": path,
                "asset": NATIVE_ASSET_SYMBOL
            })
        );
    } else {
        println!("Wallet created for {NATIVE_ASSET_NAME} ({NATIVE_ASSET_SYMBOL}).");
        println!("Address: {}", wallet.address());
        println!("Public key: {}", hex::encode(wallet.public_key().0));
        println!();
        println!("BACK UP THIS RECOVERY PHRASE SECURELY:");
        println!("{mnemonic}");
        println!();
        println!("Keystore: {}", path.display());
    }
    Ok(())
}

fn restore_wallet(args: RestoreArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = resolve_path(args.path.keystore)?;
    if path.exists() {
        return Err(format!("keystore already exists: {}", path.display()).into());
    }
    let mnemonic = match args.mnemonic {
        Some(value) => value,
        None => match args.mnemonic_file {
            Some(file) => fs::read_to_string(file)?,
            None => prompt_line("Recovery phrase: ")?,
        },
    };
    let password = read_password(args.path.password_env.as_deref(), "New keystore password: ")?;
    let mnemonic_passphrase = match args.mnemonic_passphrase_env {
        Some(name) => env::var(name)?,
        None => String::new(),
    };
    let wallet = Wallet::restore(mnemonic.trim(), &mnemonic_passphrase)?;
    let keystore = wallet.save_encrypted(&password)?;
    write_keystore(&path, &keystore)?;
    if args.path.json {
        println!(
            "{}",
            serde_json::json!({
                "address": wallet.address().to_string(),
                "public_key": hex::encode(wallet.public_key().0),
                "keystore": path
            })
        );
    } else {
        println!("Wallet restored.");
        println!("Address: {}", wallet.address());
        println!("Keystore: {}", path.display());
    }
    Ok(())
}

fn sign_message(args: SignMessageArgs) -> Result<(), Box<dyn std::error::Error>> {
    let wallet = load_wallet(&args.path)?;
    let message = match args.message {
        Some(value) => value.into_bytes(),
        None => match args.message_file {
            Some(file) => fs::read(file)?,
            None => {
                let mut input = String::new();
                io::stdin().read_to_string(&mut input)?;
                input.into_bytes()
            }
        },
    };
    let signature = wallet.sign(&message);
    println!("{}", hex::encode(signature.0));
    Ok(())
}

fn load_wallet(args: &WalletPathArgs) -> Result<Wallet, Box<dyn std::error::Error>> {
    let path = resolve_path(args.keystore.clone())?;
    let value = fs::read_to_string(path)?;
    let keystore = EncryptedKeystore::from_json(&value)?;
    let password = read_password(args.password_env.as_deref(), "Keystore password: ")?;
    Ok(keystore.decrypt(&password)?)
}

fn write_keystore(
    path: &Path,
    keystore: &EncryptedKeystore,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let json = keystore.to_json_pretty()?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    use std::io::Write;
    file.write_all(json.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn resolve_path(path: Option<PathBuf>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = path {
        return Ok(path);
    }
    if let Some(home) = env::var_os("WSC_HOME") {
        return Ok(PathBuf::from(home).join("wallet.keystore.json"));
    }
    Ok(PathBuf::from(".wsc/wallet.keystore.json"))
}

fn read_password(
    env_name: Option<&str>,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(name) = env_name {
        return Ok(env::var(name)?);
    }
    if let Ok(name) = env::var("WSC_WALLET_PASSWORD") {
        return Ok(name);
    }
    Ok(rpassword::prompt_password(prompt)?)
}

fn prompt_line(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    print!("{prompt}");
    use std::io::Write;
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim_end().to_owned())
}
