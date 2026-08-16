//! Devnet-only Intertrain bridge program for native SOL and one allow-listed SPL mint.
//!
//! The program owns a PDA vault, binds deposits/releases to an authority, and
//! creates one replay PDA per operation. The SPL path is standard SPL Token
//! only (not Token-2022) and is configured at initialization with one mint.
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::Sysvar,
};

entrypoint!(process_instruction);

const TAG_INITIALIZE: u8 = 0;
const TAG_LOCK_NATIVE_SOL: u8 = 1;
/// Direct SOL collateral lane. Same account layout as wSOL deposit, but a
/// distinct tag/log prevents a relayer from ever double-counting one deposit.
const TAG_LOCK_NATIVE_SOL_MNA: u8 = 5;
const TAG_RELEASE_NATIVE_SOL: u8 = 2;
const TAG_LOCK_SPL: u8 = 3;
const TAG_RELEASE_SPL: u8 = 4;
const STATE_LEN: usize = 96;
const REPLAY_LEN: usize = 1;
const SPL_TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn read_u64(input: &[u8]) -> Result<u64, ProgramError> {
    input
        .get(..8)
        .ok_or(ProgramError::InvalidInstructionData)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().expect("slice length checked")))
}

fn read_id(input: &[u8]) -> Result<[u8; 32], ProgramError> {
    input
        .get(..32)
        .ok_or(ProgramError::InvalidInstructionData)
        .map(|bytes| bytes.try_into().expect("slice length checked"))
}

fn pda(program_id: &Pubkey, seed: &[u8], id: Option<&[u8; 32]>) -> (Pubkey, u8) {
    match id {
        Some(id) => Pubkey::find_program_address(&[seed, id], program_id),
        None => Pubkey::find_program_address(&[seed], program_id),
    }
}

fn read_state(
    state: &AccountInfo,
    program_id: &Pubkey,
) -> Result<(Pubkey, Pubkey, Pubkey), ProgramError> {
    if state.owner != program_id || state.data_len() != STATE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let data = state.try_borrow_data()?;
    let authority = Pubkey::new_from_array(data[..32].try_into().expect("state length"));
    let vault = Pubkey::new_from_array(data[32..64].try_into().expect("state length"));
    let mint = Pubkey::new_from_array(data[64..96].try_into().expect("state length"));
    Ok((authority, vault, mint))
}

fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    program_id: &Pubkey,
    seeds: &[&[u8]],
    space: usize,
) -> ProgramResult {
    let (expected, bump) = Pubkey::find_program_address(seeds, program_id);
    if account.key != &expected || account.owner == program_id {
        return Ok(());
    }
    if account.owner != &system_program::id() {
        return Err(ProgramError::IllegalOwner);
    }
    let rent = Rent::get()?.minimum_balance(space);
    let mut signer_seeds = seeds.to_vec();
    let bump_seed = [bump];
    signer_seeds.push(&bump_seed);
    invoke_signed(
        &system_instruction::create_account(payer.key, account.key, rent, space as u64, program_id),
        &[payer.clone(), account.clone(), system.clone()],
        &[&signer_seeds],
    )
}

fn validate_replay(replay: &AccountInfo, program_id: &Pubkey) -> Result<(), ProgramError> {
    if replay.owner == program_id {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    if replay.owner != &system_program::id() {
        return Err(ProgramError::IllegalOwner);
    }
    Ok(())
}

fn read_token_account(
    account: &AccountInfo,
    mint: &Pubkey,
    owner: &Pubkey,
) -> Result<u64, ProgramError> {
    if account.owner != &SPL_TOKEN_PROGRAM_ID || account.data_len() < 72 {
        return Err(ProgramError::InvalidAccountData);
    }
    let data = account.try_borrow_data()?;
    let account_mint = Pubkey::new_from_array(data[..32].try_into().expect("token account length"));
    let account_owner =
        Pubkey::new_from_array(data[32..64].try_into().expect("token account length"));
    if account_mint != *mint || account_owner != *owner {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(u64::from_le_bytes(
        data[64..72].try_into().expect("token amount length"),
    ))
}

fn read_recipient_token_account(account: &AccountInfo, mint: &Pubkey) -> Result<(), ProgramError> {
    if account.owner != &SPL_TOKEN_PROGRAM_ID || account.data_len() < 72 {
        return Err(ProgramError::InvalidAccountData);
    }
    let data = account.try_borrow_data()?;
    let account_mint = Pubkey::new_from_array(data[..32].try_into().expect("token account length"));
    if account_mint != *mint {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

fn read_mint_decimals(mint: &AccountInfo) -> Result<u8, ProgramError> {
    if mint.owner != &SPL_TOKEN_PROGRAM_ID || mint.data_len() < 45 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(mint.try_borrow_data()?[44])
}

fn transfer_checked<'a>(
    token_program: &AccountInfo<'a>,
    source: &AccountInfo<'a>,
    mint: &AccountInfo<'a>,
    destination: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    amount: u64,
    decimals: u8,
    signed_seeds: Option<&[&[u8]]>,
) -> ProgramResult {
    if token_program.key != &SPL_TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let mut data = vec![12u8];
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);
    let ix = Instruction {
        program_id: SPL_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*source.key, false),
            AccountMeta::new_readonly(*mint.key, false),
            AccountMeta::new(*destination.key, false),
            AccountMeta::new_readonly(*authority.key, true),
        ],
        data,
    };
    let accounts = vec![
        source.clone(),
        mint.clone(),
        destination.clone(),
        authority.clone(),
        token_program.clone(),
    ];
    match signed_seeds {
        Some(seeds) => invoke_signed(&ix, &accounts, &[seeds]),
        None => invoke(&ix, &accounts),
    }
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    let (tag, rest) = input
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;
    match *tag {
        TAG_INITIALIZE => {
            let iter = &mut accounts.iter();
            let authority = next_account_info(iter)?;
            let state = next_account_info(iter)?;
            let vault = next_account_info(iter)?;
            let system = next_account_info(iter)?;
            if !authority.is_signer
                || !state.is_writable
                || !vault.is_writable
                || system.key != &system_program::id()
            {
                return Err(ProgramError::MissingRequiredSignature);
            }
            if rest.len() != 0 && rest.len() != 32 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let configured_mint = if rest.is_empty() {
                Pubkey::default()
            } else {
                Pubkey::new_from_array(rest.try_into().expect("mint length"))
            };
            let (state_pda, _) = pda(program_id, b"intertrain-wsol-state", None);
            let (vault_pda, _) = pda(program_id, b"intertrain-wsol-vault", None);
            if state.key != &state_pda || vault.key != &vault_pda {
                return Err(ProgramError::InvalidArgument);
            }
            create_pda(
                authority,
                state,
                system,
                program_id,
                &[b"intertrain-wsol-state"],
                STATE_LEN,
            )?;
            create_pda(
                authority,
                vault,
                system,
                program_id,
                &[b"intertrain-wsol-vault"],
                0,
            )?;
            let mut data = state.try_borrow_mut_data()?;
            if data.len() != STATE_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let existing = Pubkey::new_from_array(data[..32].try_into().expect("state length"));
            let existing_vault =
                Pubkey::new_from_array(data[32..64].try_into().expect("state length"));
            let existing_mint =
                Pubkey::new_from_array(data[64..96].try_into().expect("state length"));
            if existing != Pubkey::default()
                && (existing != *authority.key
                    || existing_vault != *vault.key
                    || existing_mint != configured_mint)
            {
                return Err(ProgramError::IllegalOwner);
            }
            data[..32].copy_from_slice(authority.key.as_ref());
            data[32..64].copy_from_slice(vault.key.as_ref());
            data[64..96].copy_from_slice(configured_mint.as_ref());
            msg!(
                "INTERTRAIN_INITIALIZED authority={} vault={} spl_mint={}",
                authority.key,
                vault.key,
                configured_mint
            );
        }
        TAG_LOCK_NATIVE_SOL | TAG_LOCK_NATIVE_SOL_MNA => {
            let amount = read_u64(rest)?;
            let deposit_id = read_id(&rest[8..])?;
            let dest_len = u16::from_le_bytes(
                rest.get(40..42)
                    .ok_or(ProgramError::InvalidInstructionData)?
                    .try_into()
                    .expect("length"),
            ) as usize;
            let destination = rest
                .get(42..42 + dest_len)
                .ok_or(ProgramError::InvalidInstructionData)?;
            if amount == 0 || dest_len == 0 || dest_len > 128 || !destination.is_ascii() {
                return Err(ProgramError::InvalidArgument);
            }
            let iter = &mut accounts.iter();
            let depositor = next_account_info(iter)?;
            let vault = next_account_info(iter)?;
            let state = next_account_info(iter)?;
            let replay = next_account_info(iter)?;
            let system = next_account_info(iter)?;
            if !depositor.is_signer
                || !vault.is_writable
                || !replay.is_writable
                || system.key != &system_program::id()
            {
                return Err(ProgramError::MissingRequiredSignature);
            }
            let (_, expected_vault, _) = read_state(state, program_id)?;
            if expected_vault != *vault.key || vault.owner != program_id {
                return Err(ProgramError::InvalidAccountData);
            }
            let (replay_pda, _) = pda(program_id, b"deposit", Some(&deposit_id));
            if replay.key != &replay_pda {
                return Err(ProgramError::InvalidArgument);
            }
            validate_replay(replay, program_id)?;
            create_pda(
                depositor,
                replay,
                system,
                program_id,
                &[b"deposit", &deposit_id],
                REPLAY_LEN,
            )?;
            invoke(
                &system_instruction::transfer(depositor.key, vault.key, amount),
                &[depositor.clone(), vault.clone(), system.clone()],
            )?;
            let id = hex::encode(deposit_id);
            let destination = core::str::from_utf8(destination)
                .map_err(|_| ProgramError::InvalidInstructionData)?;
            if *tag == TAG_LOCK_NATIVE_SOL_MNA {
                msg!(
                    "INTERTRAIN_SOL_MNA_DEPOSIT id={} amount={} destination={}",
                    id,
                    amount,
                    destination
                );
            } else {
                msg!(
                    "INTERTRAIN_WSOL_DEPOSIT id={} amount={} destination={}",
                    id,
                    amount,
                    destination
                );
            }
        }
        TAG_RELEASE_NATIVE_SOL => {
            let amount = read_u64(rest)?;
            let burn_id = read_id(&rest[8..])?;
            let iter = &mut accounts.iter();
            let authority = next_account_info(iter)?;
            let state = next_account_info(iter)?;
            let vault = next_account_info(iter)?;
            let recipient = next_account_info(iter)?;
            let replay = next_account_info(iter)?;
            let system = next_account_info(iter)?;
            if !authority.is_signer
                || !vault.is_writable
                || !recipient.is_writable
                || !replay.is_writable
                || system.key != &system_program::id()
            {
                return Err(ProgramError::MissingRequiredSignature);
            }
            let (expected_authority, expected_vault, _) = read_state(state, program_id)?;
            if expected_authority != *authority.key
                || expected_vault != *vault.key
                || vault.owner != program_id
                || amount == 0
            {
                return Err(ProgramError::InvalidArgument);
            }
            let (replay_pda, _) = pda(program_id, b"release", Some(&burn_id));
            if replay.key != &replay_pda {
                return Err(ProgramError::InvalidArgument);
            }
            validate_replay(replay, program_id)?;
            create_pda(
                authority,
                replay,
                system,
                program_id,
                &[b"release", &burn_id],
                REPLAY_LEN,
            )?;
            if **vault.lamports.borrow() < amount {
                return Err(ProgramError::InsufficientFunds);
            }
            **vault.try_borrow_mut_lamports()? -= amount;
            **recipient.try_borrow_mut_lamports()? = recipient
                .lamports()
                .checked_add(amount)
                .ok_or(ProgramError::ArithmeticOverflow)?;
            msg!(
                "INTERTRAIN_WSOL_RELEASE id={} amount={} recipient={}",
                hex::encode(burn_id),
                amount,
                recipient.key
            );
        }
        TAG_LOCK_SPL => {
            let amount = read_u64(rest)?;
            let deposit_id = read_id(&rest[8..])?;
            let dest_len = u16::from_le_bytes(
                rest.get(40..42)
                    .ok_or(ProgramError::InvalidInstructionData)?
                    .try_into()
                    .expect("length"),
            ) as usize;
            let destination = rest
                .get(42..42 + dest_len)
                .ok_or(ProgramError::InvalidInstructionData)?;
            if amount == 0 || dest_len == 0 || dest_len > 128 || !destination.is_ascii() {
                return Err(ProgramError::InvalidArgument);
            }
            let iter = &mut accounts.iter();
            let depositor = next_account_info(iter)?;
            let source = next_account_info(iter)?;
            let mint = next_account_info(iter)?;
            let vault_token = next_account_info(iter)?;
            let vault = next_account_info(iter)?;
            let state = next_account_info(iter)?;
            let replay = next_account_info(iter)?;
            let token_program = next_account_info(iter)?;
            let system = next_account_info(iter)?;
            if !depositor.is_signer
                || !source.is_writable
                || !vault_token.is_writable
                || !replay.is_writable
                || system.key != &system_program::id()
            {
                return Err(ProgramError::MissingRequiredSignature);
            }
            let (_, expected_vault, expected_mint) = read_state(state, program_id)?;
            if expected_mint == Pubkey::default()
                || expected_vault != *vault.key
                || expected_mint != *mint.key
            {
                return Err(ProgramError::InvalidArgument);
            }
            let decimals = read_mint_decimals(mint)?;
            let _ = read_token_account(source, mint.key, depositor.key)?;
            let _ = read_token_account(vault_token, mint.key, vault.key)?;
            let (replay_pda, _) = pda(program_id, b"spl-deposit", Some(&deposit_id));
            if replay.key != &replay_pda {
                return Err(ProgramError::InvalidArgument);
            }
            validate_replay(replay, program_id)?;
            create_pda(
                depositor,
                replay,
                system,
                program_id,
                &[b"spl-deposit", &deposit_id],
                REPLAY_LEN,
            )?;
            transfer_checked(
                token_program,
                source,
                mint,
                vault_token,
                depositor,
                amount,
                decimals,
                None,
            )?;
            let destination = core::str::from_utf8(destination)
                .map_err(|_| ProgramError::InvalidInstructionData)?;
            msg!(
                "INTERTRAIN_SPL_DEPOSIT id={} mint={} amount={} decimals={} destination={}",
                hex::encode(deposit_id),
                mint.key,
                amount,
                decimals,
                destination
            );
        }
        TAG_RELEASE_SPL => {
            let amount = read_u64(rest)?;
            let burn_id = read_id(&rest[8..])?;
            let iter = &mut accounts.iter();
            let authority = next_account_info(iter)?;
            let state = next_account_info(iter)?;
            let vault_token = next_account_info(iter)?;
            let recipient_token = next_account_info(iter)?;
            let mint = next_account_info(iter)?;
            let replay = next_account_info(iter)?;
            let token_program = next_account_info(iter)?;
            let system = next_account_info(iter)?;
            if !authority.is_signer
                || !vault_token.is_writable
                || !recipient_token.is_writable
                || !replay.is_writable
                || system.key != &system_program::id()
            {
                return Err(ProgramError::MissingRequiredSignature);
            }
            let (expected_authority, expected_vault, expected_mint) =
                read_state(state, program_id)?;
            if expected_authority != *authority.key
                || expected_mint == Pubkey::default()
                || expected_mint != *mint.key
                || amount == 0
            {
                return Err(ProgramError::InvalidArgument);
            }
            let (_, vault_pda_bump) = pda(program_id, b"intertrain-wsol-vault", None);
            let (vault_token_mint, _) = (
                read_token_account(vault_token, mint.key, &expected_vault)?,
                read_mint_decimals(mint)?,
            );
            if vault_token_mint < amount {
                return Err(ProgramError::InsufficientFunds);
            }
            read_recipient_token_account(recipient_token, mint.key)?;
            let (replay_pda, _) = pda(program_id, b"spl-release", Some(&burn_id));
            if replay.key != &replay_pda {
                return Err(ProgramError::InvalidArgument);
            }
            validate_replay(replay, program_id)?;
            create_pda(
                authority,
                replay,
                system,
                program_id,
                &[b"spl-release", &burn_id],
                REPLAY_LEN,
            )?;
            let bump_seed = [vault_pda_bump];
            let seeds: &[&[u8]] = &[b"intertrain-wsol-vault", &bump_seed];
            transfer_checked(
                token_program,
                vault_token,
                mint,
                recipient_token,
                state,
                amount,
                read_mint_decimals(mint)?,
                Some(seeds),
            )?;
            msg!(
                "INTERTRAIN_SPL_RELEASE id={} mint={} amount={} recipient_token={}",
                hex::encode(burn_id),
                mint.key,
                amount,
                recipient_token.key
            );
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    }
    Ok(())
}
