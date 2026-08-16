use clap::{Parser, Subcommand};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use std::{path::PathBuf, str::FromStr};

const INIT: u8 = 0;
const LOCK: u8 = 1;
const LOCK_MNA: u8 = 5;
const RELEASE: u8 = 2;
const LOCK_SPL: u8 = 3;
const RELEASE_SPL: u8 = 4;
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

#[derive(Parser)]
#[command(
    name = "intertrain-wsol-client",
    about = "Intertrain Solana native/SPL bridge client"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Pdas {
        #[arg(long)]
        program_id: String,
    },
    Init {
        #[arg(long)]
        rpc_url: String,
        #[arg(long)]
        program_id: String,
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        spl_mint: Option<String>,
    },
    Lock {
        #[arg(long)]
        rpc_url: String,
        #[arg(long)]
        program_id: String,
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        deposit_id: String,
        #[arg(long)]
        destination: String,
    },
    LockMna {
        #[arg(long)]
        rpc_url: String,
        #[arg(long)]
        program_id: String,
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        deposit_id: String,
        #[arg(long)]
        destination: String,
    },
    Release {
        #[arg(long)]
        rpc_url: String,
        #[arg(long)]
        program_id: String,
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        burn_id: String,
    },
    LockSpl {
        #[arg(long)]
        rpc_url: String,
        #[arg(long)]
        program_id: String,
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        source_token_account: String,
        #[arg(long)]
        vault_token_account: String,
        #[arg(long)]
        mint: String,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        deposit_id: String,
        #[arg(long)]
        destination: String,
    },
    ReleaseSpl {
        #[arg(long)]
        rpc_url: String,
        #[arg(long)]
        program_id: String,
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        vault_token_account: String,
        #[arg(long)]
        recipient_token_account: String,
        #[arg(long)]
        mint: String,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        burn_id: String,
    },
}

fn program(value: &str) -> Pubkey {
    Pubkey::from_str(value).expect("invalid program id")
}
fn pubkey(value: &str) -> Pubkey {
    Pubkey::from_str(value).expect("invalid public key")
}
fn keypair(path: &PathBuf) -> Keypair {
    read_keypair_file(path)
        .unwrap_or_else(|e| panic!("cannot read keypair {}: {e}", path.display()))
}
fn rpc(url: &str) -> RpcClient {
    RpcClient::new_with_commitment(url.to_owned(), CommitmentConfig::finalized())
}
fn token_program() -> Pubkey {
    pubkey(TOKEN_PROGRAM_ID)
}
fn pdas(pid: &Pubkey) -> (Pubkey, u8, Pubkey, u8) {
    let (state, sb) = Pubkey::find_program_address(&[b"intertrain-wsol-state"], pid);
    let (vault, vb) = Pubkey::find_program_address(&[b"intertrain-wsol-vault"], pid);
    (state, sb, vault, vb)
}
fn id(value: &str) -> [u8; 32] {
    hex::decode(value.strip_prefix("0x").unwrap_or(value))
        .expect("id must be 64 hex characters")
        .try_into()
        .expect("id must be exactly 32 bytes")
}
fn destination(value: &str) {
    if value.is_empty() || value.len() > 128 || !value.is_ascii() {
        panic!("destination must be 1..128 ASCII bytes")
    }
}
fn send(client: &RpcClient, payer: &Keypair, instructions: Vec<Instruction>) -> String {
    let bh = client.get_latest_blockhash().expect("get blockhash failed");
    let tx = Transaction::new_signed_with_payer(&instructions, Some(&payer.pubkey()), &[payer], bh);
    client
        .send_and_confirm_transaction(&tx)
        .expect("transaction failed")
        .to_string()
}
fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Pdas { program_id } => {
            let pid = program(&program_id);
            let (state, sb, vault, vb) = pdas(&pid);
            println!(
                "program_id={pid}\nstate={state}\nstate_bump={sb}\nvault={vault}\nvault_bump={vb}"
            );
        }
        Command::Init {
            rpc_url,
            program_id,
            keypair: kp,
            spl_mint,
        } => {
            let pid = program(&program_id);
            let payer = keypair(&kp);
            let (state, _, vault, _) = pdas(&pid);
            let mut data = vec![INIT];
            if let Some(mint) = spl_mint {
                data.extend_from_slice(pubkey(&mint).as_ref());
            }
            let ix = Instruction::new_with_bytes(
                pid,
                &data,
                vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(state, false),
                    AccountMeta::new(vault, false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            );
            println!(
                "signature={}\nstate={state}\nvault={vault}",
                send(&rpc(&rpc_url), &payer, vec![ix])
            );
        }
        Command::Lock {
            rpc_url,
            program_id,
            keypair: kp,
            amount,
            deposit_id,
            destination: dest,
        } => {
            if amount == 0 {
                panic!("amount must be positive")
            }
            destination(&dest);
            let raw = id(&deposit_id);
            let pid = program(&program_id);
            let payer = keypair(&kp);
            let (_, _, vault, _) = pdas(&pid);
            let (replay, _) = Pubkey::find_program_address(&[b"deposit", &raw], &pid);
            let mut data = vec![LOCK];
            data.extend_from_slice(&amount.to_le_bytes());
            data.extend_from_slice(&raw);
            data.extend_from_slice(&(dest.len() as u16).to_le_bytes());
            data.extend_from_slice(dest.as_bytes());
            let ix = Instruction::new_with_bytes(
                pid,
                &data,
                vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new_readonly(pdas(&pid).0, false),
                    AccountMeta::new(replay, false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            );
            println!("signature={}", send(&rpc(&rpc_url), &payer, vec![ix]));
        }
        Command::LockMna {
            rpc_url,
            program_id,
            keypair: kp,
            amount,
            deposit_id,
            destination: dest,
        } => {
            if amount == 0 {
                panic!("amount must be positive")
            }
            destination(&dest);
            let raw = id(&deposit_id);
            let pid = program(&program_id);
            let payer = keypair(&kp);
            let (_, _, vault, _) = pdas(&pid);
            let (replay, _) = Pubkey::find_program_address(&[b"deposit", &raw], &pid);
            let mut data = vec![LOCK_MNA];
            data.extend_from_slice(&amount.to_le_bytes());
            data.extend_from_slice(&raw);
            data.extend_from_slice(&(dest.len() as u16).to_le_bytes());
            data.extend_from_slice(dest.as_bytes());
            let ix = Instruction::new_with_bytes(
                pid,
                &data,
                vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(vault, false),
                    AccountMeta::new_readonly(pdas(&pid).0, false),
                    AccountMeta::new(replay, false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            );
            println!("signature={}", send(&rpc(&rpc_url), &payer, vec![ix]));
        }
        Command::Release {
            rpc_url,
            program_id,
            keypair: kp,
            recipient,
            amount,
            burn_id,
        } => {
            if amount == 0 {
                panic!("amount must be positive")
            }
            let raw = id(&burn_id);
            let pid = program(&program_id);
            let authority = keypair(&kp);
            let recipient = pubkey(&recipient);
            let (state, _, vault, _) = pdas(&pid);
            let (replay, _) = Pubkey::find_program_address(&[b"release", &raw], &pid);
            let mut data = vec![RELEASE];
            data.extend_from_slice(&amount.to_le_bytes());
            data.extend_from_slice(&raw);
            let ix = Instruction::new_with_bytes(
                pid,
                &data,
                vec![
                    AccountMeta::new_readonly(authority.pubkey(), true),
                    AccountMeta::new_readonly(state, false),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(recipient, false),
                    AccountMeta::new(replay, false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            );
            println!("signature={}", send(&rpc(&rpc_url), &authority, vec![ix]));
        }
        Command::LockSpl {
            rpc_url,
            program_id,
            keypair: kp,
            source_token_account,
            vault_token_account,
            mint,
            amount,
            deposit_id,
            destination: dest,
        } => {
            if amount == 0 {
                panic!("amount must be positive")
            }
            destination(&dest);
            let raw = id(&deposit_id);
            let pid = program(&program_id);
            let payer = keypair(&kp);
            let (state, _, vault, _) = pdas(&pid);
            let source = pubkey(&source_token_account);
            let vault_token = pubkey(&vault_token_account);
            let mint = pubkey(&mint);
            let token = token_program();
            let (replay, _) = Pubkey::find_program_address(&[b"spl-deposit", &raw], &pid);
            let mut data = vec![LOCK_SPL];
            data.extend_from_slice(&amount.to_le_bytes());
            data.extend_from_slice(&raw);
            data.extend_from_slice(&(dest.len() as u16).to_le_bytes());
            data.extend_from_slice(dest.as_bytes());
            let ix = Instruction::new_with_bytes(
                pid,
                &data,
                vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(source, false),
                    AccountMeta::new_readonly(mint, false),
                    AccountMeta::new(vault_token, false),
                    AccountMeta::new_readonly(vault, false),
                    AccountMeta::new_readonly(state, false),
                    AccountMeta::new(replay, false),
                    AccountMeta::new_readonly(token, false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            );
            println!("signature={}", send(&rpc(&rpc_url), &payer, vec![ix]));
        }
        Command::ReleaseSpl {
            rpc_url,
            program_id,
            keypair: kp,
            vault_token_account,
            recipient_token_account,
            mint,
            amount,
            burn_id,
        } => {
            if amount == 0 {
                panic!("amount must be positive")
            }
            let raw = id(&burn_id);
            let pid = program(&program_id);
            let authority = keypair(&kp);
            let (state, _, vault, _) = pdas(&pid);
            let vault_token = pubkey(&vault_token_account);
            let recipient_token = pubkey(&recipient_token_account);
            let mint = pubkey(&mint);
            let token = token_program();
            let (replay, _) = Pubkey::find_program_address(&[b"spl-release", &raw], &pid);
            let mut data = vec![RELEASE_SPL];
            data.extend_from_slice(&amount.to_le_bytes());
            data.extend_from_slice(&raw);
            let ix = Instruction::new_with_bytes(
                pid,
                &data,
                vec![
                    AccountMeta::new_readonly(authority.pubkey(), true),
                    AccountMeta::new_readonly(state, false),
                    AccountMeta::new(vault_token, false),
                    AccountMeta::new(recipient_token, false),
                    AccountMeta::new_readonly(mint, false),
                    AccountMeta::new(replay, false),
                    AccountMeta::new_readonly(token, false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            );
            println!("signature={}", send(&rpc(&rpc_url), &authority, vec![ix]));
        }
    }
}
