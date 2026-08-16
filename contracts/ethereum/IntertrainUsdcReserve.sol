// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title Intertrain USDC Reserve
/// @notice Dedicated USDC escrow for devnet reserve accounting. This contract
/// is intentionally separate from the WETH bridge. It never mints MNA; the
/// finalized Deposit event is consumed by the Intertrain relayer and consensus
/// reserve ledger. Audit before any production deployment.
interface IERC20 {
    function transferFrom(address from, address to, uint256 value) external returns (bool);
    function transfer(address to, uint256 value) external returns (bool);
    function balanceOf(address account) external view returns (uint256);
}

contract IntertrainUsdcReserve {
    IERC20 public immutable usdc;
    address public owner;
    address public relayer;
    bool public paused;
    mapping(bytes32 => bool) public deposits;
    mapping(bytes32 => bool) public releases;

    event Deposit(bytes32 indexed depositId, address indexed sender, uint256 amount, string destination);
    event Release(bytes32 indexed burnId, address indexed recipient, uint256 amount);
    event RelayerUpdated(address indexed previousRelayer, address indexed newRelayer);
    event Paused(address indexed caller);
    event Unpaused(address indexed caller);

    modifier onlyOwner() {
        require(msg.sender == owner, "not owner");
        _;
    }

    modifier onlyRelayer() {
        require(msg.sender == relayer, "not relayer");
        _;
    }

    modifier whenNotPaused() {
        require(!paused, "paused");
        _;
    }

    constructor(address usdc_, address relayer_) {
        require(usdc_ != address(0) && relayer_ != address(0), "zero address");
        owner = msg.sender;
        usdc = IERC20(usdc_);
        relayer = relayer_;
    }

    function deposit(bytes32 depositId, uint256 amount, string calldata destination)
        external
        whenNotPaused
    {
        require(amount > 0 && bytes(destination).length > 0, "invalid deposit");
        require(!deposits[depositId], "deposit used");
        deposits[depositId] = true;
        require(usdc.transferFrom(msg.sender, address(this), amount), "transferFrom failed");
        emit Deposit(depositId, msg.sender, amount, destination);
    }

    function release(bytes32 burnId, address recipient, uint256 amount)
        external
        onlyRelayer
        whenNotPaused
    {
        require(!releases[burnId], "release used");
        require(recipient != address(0) && amount > 0, "invalid release");
        require(usdc.balanceOf(address(this)) >= amount, "insufficient reserve");
        releases[burnId] = true;
        require(usdc.transfer(recipient, amount), "transfer failed");
        emit Release(burnId, recipient, amount);
    }

    function setRelayer(address newRelayer) external onlyOwner {
        require(newRelayer != address(0), "zero relayer");
        emit RelayerUpdated(relayer, newRelayer);
        relayer = newRelayer;
    }

    function pause() external onlyOwner {
        paused = true;
        emit Paused(msg.sender);
    }

    function unpause() external onlyOwner {
        paused = false;
        emit Unpaused(msg.sender);
    }
}
