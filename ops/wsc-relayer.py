#!/usr/bin/env python3
"""Intertrain Sepolia relayer with durable idempotency and retry state.

This is testnet infrastructure. It never stores a raw Ethereum private key;
Foundry signs releases from an encrypted keystore selected by account name.
"""
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
import time
import urllib.request
from pathlib import Path


def required(name):
    value = os.environ.get(name, "").strip()
    if not value:
        raise RuntimeError(f"missing required environment variable: {name}")
    return value


ETH_RPC_URL = required("WSC_ETHEREUM_RPC_URL")
WSC_RPC_URL = os.environ.get("WSC_RPC_URL", "http://127.0.0.1:26657/rpc")
BRIDGE_CONTRACT = required("WSC_ETHEREUM_BRIDGE_CONTRACT")
DEPOSIT_TOPIC = required("WSC_ETHEREUM_DEPOSIT_TOPIC")
USDC_BRIDGE_CONTRACT = os.environ.get("WSC_ETHEREUM_USDC_BRIDGE_CONTRACT", "").strip()
USDC_DEPOSIT_TOPIC = os.environ.get("WSC_ETHEREUM_USDC_DEPOSIT_TOPIC", "").strip()
USDC_ASSET_ID = os.environ.get("WSC_USDC_ASSET_ID", "ethereum:USDC:sepolia:0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238")
OPERATOR_TOKEN = required("WSC_BRIDGE_OPERATOR_TOKEN")
WETH_ASSET_ID = os.environ.get("WSC_WETH_ASSET_ID", "ethereum:WETH:sepolia:bridge-placeholder")
CONFIRMATIONS = int(os.environ.get("WSC_ETHEREUM_CONFIRMATIONS", "12"))
POLL_SECONDS = int(os.environ.get("WSC_RELAYER_POLL_SECONDS", "10"))
STATE_DB = Path(os.environ.get("WSC_RELAYER_STATE", "/var/lib/worldstreet-relayer/state.sqlite3"))
HEALTH_FILE = Path(os.environ.get("WSC_RELAYER_HEALTH", "/var/lib/worldstreet-relayer/health.json"))
ACCOUNT = os.environ.get("WSC_RELAYER_ACCOUNT", "intertrain-sepolia-deployer")
KEYSTORE_DIR = os.environ.get("WSC_KEYSTORE_DIR", "/root/.foundry/keystores")
KEYSTORE_PASSWORD_FILE = os.environ.get("WSC_KEYSTORE_PASSWORD_FILE", "")
CAST = os.environ.get("WSC_CAST", "/root/.foundry/bin/cast")
START_BLOCK = os.environ.get("WSC_RELAYER_START_BLOCK", "latest")

SOLANA_MODE = os.environ.get("WSC_SOLANA_MODE", "disabled").strip().lower()
SOLANA_RPC_URL = os.environ.get("WSC_SOLANA_RPC_URL", "").strip()
SOLANA_VAULT_ADDRESS = os.environ.get("WSC_SOLANA_VAULT_ADDRESS", "").strip()
SOLANA_VAULT_KEYPAIR = os.environ.get("WSC_SOLANA_VAULT_KEYPAIR", "").strip()
SOLANA_PROGRAM_ID = os.environ.get("WSC_SOLANA_PROGRAM_ID", os.environ.get("WSC_SOLANA_BRIDGE_PROGRAM", "")).strip()
SOLANA_PROGRAM_AUTHORITY_KEYPAIR = os.environ.get("WSC_SOLANA_PROGRAM_AUTHORITY_KEYPAIR", "").strip()
SOLANA_CLIENT_BIN = os.environ.get("WSC_SOLANA_CLIENT_BIN", "/usr/local/bin/intertrain-wsol-client").strip()
SOLANA_STATE_ADDRESS = os.environ.get("WSC_SOLANA_STATE_ADDRESS", "").strip()
SOLANA_WSOL_MINT = os.environ.get("WSC_SOLANA_WSOL_MINT", "So11111111111111111111111111111111111111112").strip()
SOLANA_SPL_USDC_MINT = os.environ.get("WSC_SOLANA_SPL_USDC_MINT", "").strip()
SOLANA_SPL_USDC_VAULT_TOKEN_ACCOUNT = os.environ.get("WSC_SOLANA_SPL_USDC_VAULT_TOKEN_ACCOUNT", "").strip()
SOLANA_NETWORK = os.environ.get("WSC_SOLANA_NETWORK", "devnet").strip()
SOLANA_COMMITMENT = os.environ.get("WSC_SOLANA_COMMITMENT", "finalized").strip()
SOLANA_MNA_ENABLED = os.environ.get("WSC_SOLANA_MNA_ENABLED", "false").strip().lower() in ("1", "true", "yes", "on")
SOLANA_MNA_ASSET_ID = os.environ.get("WSC_SOLANA_MNA_ASSET_ID", "solana:SOL:devnet:native").strip()
# Devnet oracle adapter: the value is micro-USD per SOL and must be refreshed
# by the operator/oracle job; the chain validates the exact snapshot math.
SOLANA_MNA_PRICE_USD_MICRO_PER_SOL = int(os.environ.get("WSC_SOLANA_MNA_PRICE_USD_MICRO_PER_SOL", "0"))
SOLANA_MNA_ORACLE_URL = os.environ.get("WSC_SOLANA_MNA_ORACLE_URL", "https://hermes.pyth.network/v2/updates/price/latest?ids[]=ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d&parsed=true").strip()
SOLANA_MNA_ORACLE_MAX_AGE = int(os.environ.get("WSC_SOLANA_MNA_ORACLE_MAX_AGE", "120"))
SOLANA_MNA_FEE_BPS = max(50, min(500, int(os.environ.get("WSC_SOLANA_MNA_FEE_BPS", "50"))))
SOLANA_CLI = os.environ.get("WSC_SOLANA_CLI", "/root/.local/share/solana/install/active_release/bin/solana")
SOLANA_ENABLED = (
    SOLANA_MODE == "custody" and bool(SOLANA_RPC_URL and SOLANA_VAULT_ADDRESS and SOLANA_WSOL_MINT and SOLANA_VAULT_KEYPAIR)
) or (
    SOLANA_MODE == "program" and bool(SOLANA_RPC_URL and SOLANA_PROGRAM_ID and SOLANA_VAULT_ADDRESS and SOLANA_WSOL_MINT and SOLANA_PROGRAM_AUTHORITY_KEYPAIR and SOLANA_CLIENT_BIN)
)
SOLANA_ASSET_ID = os.environ.get(
    "WSC_WSOL_ASSET_ID",
    f"solana:WSOL:{SOLANA_NETWORK}:vault:{SOLANA_VAULT_ADDRESS}:{SOLANA_WSOL_MINT}",
)
SOLANA_SPL_USDC_ASSET_ID = f"solana:USDC:{SOLANA_NETWORK}:{SOLANA_SPL_USDC_MINT}"
SOLANA_SPL_ENABLED = (
    SOLANA_MODE == "program"
    and bool(
        SOLANA_RPC_URL
        and SOLANA_PROGRAM_ID
        and SOLANA_VAULT_ADDRESS
        and SOLANA_SPL_USDC_MINT
        and SOLANA_SPL_USDC_VAULT_TOKEN_ACCOUNT
        and SOLANA_PROGRAM_AUTHORITY_KEYPAIR
        and SOLANA_CLIENT_BIN
    )
)


def rpc(url, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    with urllib.request.urlopen(request, timeout=20) as response:
        result = json.loads(response.read().decode())
    if result.get("error"):
        raise RuntimeError(f"{method}: {result['error']}")
    return result.get("result")


def hex_int(value):
    return int(value, 16)


def db():
    STATE_DB.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(STATE_DB)
    connection.execute("PRAGMA journal_mode=WAL")
    connection.execute("""
        CREATE TABLE IF NOT EXISTS operations (
            operation_id TEXT PRIMARY KEY,
            direction TEXT NOT NULL,
            status TEXT NOT NULL,
            asset_id TEXT NOT NULL,
            address TEXT NOT NULL,
            destination TEXT NOT NULL,
            amount TEXT NOT NULL,
            external_transaction TEXT NOT NULL,
            source_block INTEGER NOT NULL DEFAULT 0,
            attempts INTEGER NOT NULL DEFAULT 0,
            next_attempt REAL NOT NULL DEFAULT 0,
            last_error TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL
        )
    """)
    connection.execute("CREATE TABLE IF NOT EXISTS cursors (name TEXT PRIMARY KEY, value TEXT NOT NULL)")
    connection.commit()
    return connection


def cursor(connection):
    row = connection.execute("SELECT value FROM cursors WHERE name='eth_block'").fetchone()
    if row:
        return int(row[0])
    if START_BLOCK == "latest":
        value = hex_int(rpc(ETH_RPC_URL, "eth_blockNumber", []))
    else:
        value = int(START_BLOCK, 0)
    connection.execute("INSERT OR REPLACE INTO cursors(name,value) VALUES('eth_block',?)", (str(value),))
    connection.commit()
    return value


def decode_deposit(data):
    raw = data[2:] if data.startswith("0x") else data
    if len(raw) < 128:
        raise ValueError("Deposit event data is too short")
    amount = int(raw[0:64], 16)
    offset = int(raw[64:128], 16)
    start = offset * 2
    if start + 64 > len(raw):
        raise ValueError("Deposit destination offset is invalid")
    length = int(raw[start:start + 64], 16)
    text_start = start + 64
    destination = bytes.fromhex(raw[text_start:text_start + length * 2]).decode()
    return amount, destination


def discover_deposits(connection, from_block, to_block):
    if from_block > to_block:
        return
    sources = [(BRIDGE_CONTRACT, DEPOSIT_TOPIC, WETH_ASSET_ID)]
    if USDC_BRIDGE_CONTRACT and USDC_DEPOSIT_TOPIC:
        sources.append((USDC_BRIDGE_CONTRACT, USDC_DEPOSIT_TOPIC, USDC_ASSET_ID))
    now = time.time()
    for contract, topic, asset_id in sources:
        logs = rpc(ETH_RPC_URL, "eth_getLogs", [{
            "fromBlock": hex(from_block),
            "toBlock": hex(to_block),
            "address": contract,
            "topics": [topic],
        }]) or []
        for log in logs:
            topics = log.get("topics", [])
            if len(topics) < 3:
                continue
            operation_id = topics[1].removeprefix("0x").lower()
            amount, destination = decode_deposit(log.get("data", "0x"))
            external_tx = log.get("transactionHash", "").removeprefix("0x")
            connection.execute("""
                INSERT OR IGNORE INTO operations
                (operation_id,direction,status,asset_id,address,destination,amount,external_transaction,source_block,created_at,updated_at)
                VALUES(?,?,?,?,?,?,?,?,?,?,?)
            """, (operation_id, "inbound", "detected", asset_id, destination, destination,
                  str(amount), external_tx, hex_int(log.get("blockNumber", "0x0")), now, now))
    connection.commit()

def solana_call(method, params):
    if not SOLANA_RPC_URL:
        raise RuntimeError("WSC_SOLANA_RPC_URL is not configured")
    return rpc(SOLANA_RPC_URL, method, params)


def solana_cursor(connection):
    row = connection.execute("SELECT value FROM cursors WHERE name='solana_signature'").fetchone()
    if row:
        return row[0]
    if not SOLANA_ENABLED:
        return ""
    signatures = solana_call("getSignaturesForAddress", [SOLANA_VAULT_ADDRESS, {"limit": 1, "commitment": SOLANA_COMMITMENT}]) or []
    value = signatures[0].get("signature", "") if signatures else ""
    connection.execute("INSERT OR REPLACE INTO cursors(name,value) VALUES('solana_signature',?)", (value,))
    connection.commit()
    return value


def solana_account_key(value):
    if isinstance(value, dict):
        return value.get("pubkey", "")
    return value


def solana_program_log(transaction):
    meta = transaction.get("meta") or {}
    for message in meta.get("logMessages") or []:
        match = re.search(r"INTERTRAIN_WSOL_DEPOSIT id=([0-9a-fA-F]{64}) amount=(\d+) destination=([^\s]+)", message)
        if match:
            return match.group(1).lower(), int(match.group(2)), match.group(3)
    return None

def solana_mna_program_log(transaction):
    meta = transaction.get("meta") or {}
    for message in meta.get("logMessages") or []:
        match = re.search(r"INTERTRAIN_SOL_MNA_DEPOSIT id=([0-9a-fA-F]{64}) amount=(\d+) destination=([^\s]+)", message)
        if match:
            return match.group(1).lower(), int(match.group(2)), match.group(3)
    return None


def solana_spl_program_log(transaction):
    meta = transaction.get("meta") or {}
    for message in meta.get("logMessages") or []:
        match = re.search(
            r"INTERTRAIN_SPL_DEPOSIT id=([0-9a-fA-F]{64}) mint=([1-9A-HJ-NP-Za-km-z]{32,44}) amount=(\d+) decimals=(\d+) destination=([^\s]+)",
            message,
        )
        if match:
            return (
                match.group(1).lower(),
                match.group(2),
                int(match.group(3)),
                int(match.group(4)),
                match.group(5),
            )
    return None


def solana_program_called(transaction):
    message = (transaction.get("transaction") or {}).get("message") or {}
    for instruction in message.get("instructions", []):
        if instruction.get("programId") == SOLANA_PROGRAM_ID or instruction.get("program") == SOLANA_PROGRAM_ID:
            return True
    return False


def solana_memo(transaction):
    message = (transaction.get("transaction") or {}).get("message") or {}
    for instruction in message.get("instructions", []):
        if instruction.get("program") in ("spl-memo", "spl-memo-v2"):
            parsed = instruction.get("parsed")
            if isinstance(parsed, str):
                return parsed
            if isinstance(instruction.get("data"), str):
                return instruction["data"]
    return ""


def discover_solana_deposits(connection):
    if not SOLANA_ENABLED:
        return
    previous = solana_cursor(connection)
    signatures = solana_call(
        "getSignaturesForAddress",
        [SOLANA_VAULT_ADDRESS, {"limit": 1000, "commitment": SOLANA_COMMITMENT}],
    ) or []
    if not signatures:
        return
    newest = signatures[0].get("signature", "")
    if not previous:
        connection.execute("INSERT OR REPLACE INTO cursors(name,value) VALUES('solana_signature',?)", (newest,))
        connection.commit()
        return
    unseen = []
    for item in signatures:
        signature = item.get("signature", "")
        if signature == previous:
            break
        unseen.append(item)
    unseen.reverse()
    if previous not in {item.get("signature", "") for item in signatures}:
        # The cursor fell outside the pagination window. Do not silently skip deposits.
        unseen = list(reversed(signatures))
    now = time.time()
    for item in unseen:
        signature = item.get("signature", "")
        if not signature or item.get("err") is not None:
            continue
        transaction = solana_call(
            "getTransaction",
            [signature, {"encoding": "jsonParsed", "commitment": SOLANA_COMMITMENT, "maxSupportedTransactionVersion": 0}],
        )
        if not transaction:
            continue
        message = (transaction.get("transaction") or {}).get("message") or {}
        meta = transaction.get("meta") or {}
        if SOLANA_MODE == "program":
            if not solana_program_called(transaction):
                continue
            mna_parsed = solana_mna_program_log(transaction) if SOLANA_MNA_ENABLED else None
            spl_parsed = solana_spl_program_log(transaction)
            if mna_parsed:
                operation_id, amount, destination = mna_parsed
                asset_id = SOLANA_MNA_ASSET_ID
            elif spl_parsed and spl_parsed[1] == SOLANA_SPL_USDC_MINT:
                operation_id, _mint, amount, decimals, destination = spl_parsed
                if decimals != 6 or not SOLANA_SPL_ENABLED:
                    continue
                asset_id = SOLANA_SPL_USDC_ASSET_ID
            else:
                parsed = solana_program_log(transaction)
                if not parsed:
                    continue
                operation_id, amount, destination = parsed
                asset_id = SOLANA_ASSET_ID
        else:
            keys = [solana_account_key(value) for value in message.get("accountKeys", [])]
            try:
                vault_index = keys.index(SOLANA_VAULT_ADDRESS)
                amount = int(meta.get("postBalances", [])[vault_index]) - int(meta.get("preBalances", [])[vault_index])
            except (ValueError, IndexError, TypeError, KeyError):
                continue
            if amount <= 0:
                continue
            memo = solana_memo(transaction)
            match = re.fullmatch(r"intertrain:wsol:deposit:([0-9a-fA-F]{64}):([^:]+)", memo)
            if not match:
                continue
            operation_id = hashlib.sha256(f"solana:wsol:deposit:{signature}".encode()).hexdigest()
            destination = match.group(2)
        connection.execute(
            """
            INSERT OR IGNORE INTO operations
            (operation_id,direction,status,asset_id,address,destination,amount,external_transaction,source_block,created_at,updated_at)
            VALUES(?,?,?,?,?,?,?,?,?,?,?)
            """,
            (operation_id, "inbound", "detected", asset_id, destination, destination,
             str(amount), signature, int(transaction.get("slot") or item.get("slot") or 0), now, now),
        )
    connection.execute("INSERT OR REPLACE INTO cursors(name,value) VALUES('solana_signature',?)", (newest,))
    connection.commit()


def solana_recipient(value):
    if not re.fullmatch(r"[1-9A-HJ-NP-Za-km-z]{32,44}", value):
        raise ValueError("burn destination must be a Solana public key")
    return value


def submit_solana_release(operation):
    recipient = solana_recipient(operation[5])
    lamports = int(operation[6])
    if lamports <= 0:
        raise ValueError("release amount must be positive")
    if operation[3].startswith("solana:USDC:"):
        if not SOLANA_SPL_ENABLED:
            raise RuntimeError("Solana SPL USDC bridge is not configured")
        command = [
            SOLANA_CLIENT_BIN,
            "release-spl",
            "--rpc-url", SOLANA_RPC_URL,
            "--program-id", SOLANA_PROGRAM_ID,
            "--keypair", SOLANA_PROGRAM_AUTHORITY_KEYPAIR,
            "--vault-token-account", SOLANA_SPL_USDC_VAULT_TOKEN_ACCOUNT,
            "--recipient-token-account", recipient,
            "--mint", SOLANA_SPL_USDC_MINT,
            "--amount", str(lamports),
            "--burn-id", operation[0],
        ]
    elif SOLANA_MODE == "program":
        if not SOLANA_PROGRAM_AUTHORITY_KEYPAIR:
            raise RuntimeError("WSC_SOLANA_PROGRAM_AUTHORITY_KEYPAIR is not configured")
        command = [SOLANA_CLIENT_BIN, "release", "--rpc-url", SOLANA_RPC_URL, "--program-id", SOLANA_PROGRAM_ID,
                   "--keypair", SOLANA_PROGRAM_AUTHORITY_KEYPAIR, "--recipient", recipient, "--amount", str(lamports),
                   "--burn-id", operation[0]]
    else:
        if not SOLANA_VAULT_KEYPAIR:
            raise RuntimeError("WSC_SOLANA_VAULT_KEYPAIR is not configured")
        command = [
            SOLANA_CLI, "transfer", recipient, f"{lamports / 1_000_000_000:.9f}",
            "--url", SOLANA_RPC_URL, "--keypair", SOLANA_VAULT_KEYPAIR,
            "--allow-unfunded-recipient", "--with-memo", f"intertrain:wsol:release:{operation[0]}",
            "--commitment", SOLANA_COMMITMENT, "--output", "json",
        ]
    result = subprocess.run(command, capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "solana transfer failed")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"signature": result.stdout.strip()}


def discover_burns(connection):
    result = rpc(WSC_RPC_URL, "bridge_operations_recent", {}) or {}
    now = time.time()
    for operation in result.get("operations", []):
        if operation.get("kind") != "burn":
            continue
        operation_id = operation.get("operation_id", "").removeprefix("0x").lower()
        if len(operation_id) != 64:
            continue
        connection.execute("""
            INSERT OR IGNORE INTO operations
            (operation_id,direction,status,asset_id,address,destination,amount,external_transaction,source_block,created_at,updated_at)
            VALUES(?,?,?,?,?,?,?,?,?,?,?)
        """, (operation_id, "outbound", "detected", operation["asset_id"], operation["address"],
              operation.get("destination", ""), operation["amount"], operation.get("external_transaction", ""), 0, now, now))
    connection.commit()


def wsc_call(method, params):
    return rpc(WSC_RPC_URL, method, params)



def solana_mna_oracle_snapshot():
    """Return (micro-USD/SOL, publish timestamp), failing closed if stale."""
    if not SOLANA_MNA_ORACLE_URL:
        if SOLANA_MNA_PRICE_USD_MICRO_PER_SOL <= 0:
            raise RuntimeError("SOL/MNA oracle is not configured")
        return SOLANA_MNA_PRICE_USD_MICRO_PER_SOL, int(time.time())
    request = urllib.request.Request(SOLANA_MNA_ORACLE_URL, headers={"User-Agent": "intertrain-relayer/1"})
    with urllib.request.urlopen(request, timeout=20) as response:
        payload = json.loads(response.read().decode())
    parsed = (payload.get("parsed") or [])
    if not parsed or not isinstance(parsed[0], dict):
        raise RuntimeError("SOL/MNA oracle returned no parsed price")
    price = parsed[0].get("price") or {}
    raw = int(price.get("price", "0"))
    expo = int(price.get("expo", "0"))
    published = int(price.get("publish_time", "0"))
    if raw <= 0 or published <= 0 or time.time() - published > SOLANA_MNA_ORACLE_MAX_AGE:
        raise RuntimeError("SOL/MNA oracle price is stale or invalid")
    if expo >= 0:
        micro = raw * (10 ** expo) * 1_000_000
    else:
        divisor = 10 ** (-expo)
        micro = (raw * 1_000_000) // divisor
    if micro <= 0:
        raise RuntimeError("SOL/MNA oracle price is below precision")
    return micro, published

def submit_mint(operation):
    if operation[3] == SOLANA_MNA_ASSET_ID:
        price_micro, oracle_timestamp = solana_mna_oracle_snapshot()
        lamports = int(operation[6])
        usd_micro = (lamports * price_micro) // 1_000_000_000
        gross_mna = usd_micro // 2
        fee_mna = (gross_mna * SOLANA_MNA_FEE_BPS) // 10_000
        amount_mna = gross_mna - fee_mna
        if usd_micro <= 0 or usd_micro % 2 or amount_mna <= 0:
            raise RuntimeError("SOL deposit is below the minimum quoteable MNA amount")
        return wsc_call("mna_reserve_mint", {
            "operator_token": OPERATOR_TOKEN,
            "collateral_asset": SOLANA_MNA_ASSET_ID,
            "address": operation[4],
            "amount_usdc": str(usd_micro),
            "amount_mna": str(amount_mna),
            "collateral_amount": str(lamports),
            "oracle_price_usd_micro_per_sol": str(price_micro),
            "oracle_timestamp": oracle_timestamp,
            "fee_mna": str(fee_mna),
            "external_transaction": operation[7],
            "destination": operation[5],
            "memo": "solana devnet SOL direct collateral",
        })
    return wsc_call("bridge_mint", {
        "operator_token": OPERATOR_TOKEN,
        "operation_id": operation[0],
        "asset_id": operation[3],
        "address": operation[4],
        "destination": operation[4],
        "amount": operation[6],
        "external_transaction": operation[7],
        "memo": "ethereum deposit relayed",
    })


def recipient_from_destination(destination):
    if not re.fullmatch(r"0x[a-fA-F0-9]{40}", destination):
        raise ValueError("burn destination must be an Ethereum 0x address")
    return destination


def submit_release(operation):
    if operation[3].startswith("solana:WSOL:"):
        return submit_solana_release(operation)
    recipient = recipient_from_destination(operation[5])
    command = [CAST, "send", BRIDGE_CONTRACT, "release(bytes32,address,uint256)",
               "0x" + operation[0], recipient, operation[6], "--rpc-url", ETH_RPC_URL]
    keystore_path = Path(KEYSTORE_DIR) / ACCOUNT
    if keystore_path.exists():
        command.extend(["--keystore", str(keystore_path)])
        if KEYSTORE_PASSWORD_FILE:
            command.extend(["--password-file", KEYSTORE_PASSWORD_FILE])
    else:
        command.extend(["--account", ACCOUNT])
    command.append("--json")
    result = subprocess.run(command, capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "cast send failed")
    return json.loads(result.stdout)

def mark_error(connection, operation_id, error):
    row = connection.execute("SELECT attempts FROM operations WHERE operation_id=?", (operation_id,)).fetchone()
    attempts = (row[0] if row else 0) + 1
    delay = min(3600, 2 ** min(attempts, 10))
    now = time.time()
    connection.execute("UPDATE operations SET status='retry',attempts=?,next_attempt=?,last_error=?,updated_at=? WHERE operation_id=?",
                       (attempts, now + delay, str(error)[:1000], now, operation_id))
    connection.commit()


def process_operations(connection):
    now = time.time()
    rows = connection.execute("""
        SELECT operation_id,direction,status,asset_id,address,destination,amount,external_transaction,source_block,attempts,next_attempt,last_error
        FROM operations WHERE status IN ('detected','retry') AND next_attempt <= ? ORDER BY created_at LIMIT 50
    """, (now,)).fetchall()
    for operation in rows:
        try:
            if operation[1] == "inbound":
                submit_mint(operation)
                status = "mint_submitted"
            else:
                submit_release(operation)
                status = "release_submitted"
            connection.execute("UPDATE operations SET status=?,updated_at=?,last_error='' WHERE operation_id=?", (status, now, operation[0]))
            connection.commit()
        except Exception as error:
            mark_error(connection, operation[0], error)

    submitted = connection.execute("""
        SELECT operation_id,direction FROM operations WHERE status IN ('mint_submitted','release_submitted')
    """).fetchall()
    for operation_id, direction in submitted:
        try:
            if direction == "inbound":
                status = wsc_call("bridge_operation_status", {"operation_id": operation_id}).get("status")
                if status == "confirmed":
                    connection.execute("UPDATE operations SET status='completed',updated_at=? WHERE operation_id=?", (time.time(), operation_id))
            else:
                # cast send has already returned a mined transaction receipt; this is the terminal local state.
                connection.execute("UPDATE operations SET status='completed',updated_at=? WHERE operation_id=?", (time.time(), operation_id))
            connection.commit()
        except Exception as error:
            mark_error(connection, operation_id, error)


def write_health(connection, last_error=""):
    HEALTH_FILE.parent.mkdir(parents=True, exist_ok=True)
    counts = dict(connection.execute("SELECT status,COUNT(*) FROM operations GROUP BY status").fetchall())
    payload = {"updated_at": int(time.time()), "status": "ok" if not last_error else "degraded", "counts": counts, "last_error": last_error}
    temporary = HEALTH_FILE.with_suffix(".tmp")
    temporary.write_text(json.dumps(payload, indent=2) + "\n")
    temporary.replace(HEALTH_FILE)


def main():
    connection = db()
    last_error = ""
    print(json.dumps({"event": "relayer_started", "account": ACCOUNT, "contract": BRIDGE_CONTRACT}), flush=True)
    while True:
        try:
            tip = hex_int(rpc(ETH_RPC_URL, "eth_blockNumber", []))
            previous = cursor(connection)
            # Only scan blocks that are finalized on Sepolia.  Do not use
            # the cursor as the safe tip: when it equals the current head,
            # querying that range can race Alchemy and produce an out-of-range
            # error.
            safe_tip = tip - CONFIRMATIONS
            if safe_tip >= previous:
                discover_deposits(connection, previous, safe_tip)
                connection.execute("INSERT OR REPLACE INTO cursors(name,value) VALUES('eth_block',?)", (str(safe_tip + 1),))
                connection.commit()
            discover_solana_deposits(connection)
            discover_burns(connection)
            process_operations(connection)
            write_health(connection)
            last_error = ""
        except Exception as error:
            last_error = str(error)
            print(json.dumps({"event": "relayer_cycle_failed", "error": last_error}), file=sys.stderr, flush=True)
            try:
                write_health(connection, last_error)
            except Exception:
                pass
        time.sleep(POLL_SECONDS)


if __name__ == "__main__":
    main()
