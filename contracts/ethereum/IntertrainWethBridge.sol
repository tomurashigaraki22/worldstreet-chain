// Testnet-only ETH escrow bridge for Intertrain wETH.
// This contract is a starting point and must be audited before real funds.
// Deposit native ETH; an authorized relayer releases ETH after an Intertrain burn.
pragma solidity ^0.8.24;

contract IntertrainWethBridge {
    address public immutable relayer;
    mapping(bytes32 => bool) public deposits;
    mapping(bytes32 => bool) public releases;

    event Deposit(bytes32 indexed depositId, address indexed sender, uint256 amount, string destination);
    event Release(bytes32 indexed burnId, address indexed recipient, uint256 amount);

    modifier onlyRelayer() {
        require(msg.sender == relayer, "not relayer");
        _;
    }

    constructor(address relayer_) {
        require(relayer_ != address(0), "zero relayer");
        relayer = relayer_;
    }

    function deposit(bytes32 depositId, string calldata destination) external payable {
        require(msg.value > 0, "zero value");
        require(!deposits[depositId], "deposit used");
        deposits[depositId] = true;
        emit Deposit(depositId, msg.sender, msg.value, destination);
    }

    function release(bytes32 burnId, address payable recipient, uint256 amount) external onlyRelayer {
        require(!releases[burnId], "release used");
        require(recipient != address(0) && amount > 0, "invalid release");
        require(address(this).balance >= amount, "insufficient reserve");
        releases[burnId] = true;
        (bool ok,) = recipient.call{value: amount}("");
        require(ok, "release failed");
        emit Release(burnId, recipient, amount);
    }

    receive() external payable {}
}
