// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.28;

/// @title Payments of atomic swaps against TESS
/// @notice A taker buying TESS locks its payment here for the maker of a Tesserax offer, under the
/// SHA-256 hash of a secret. The maker locks the TESS on Tesserax under the same hash; the taker
/// claims them with the secret, which anyone then submits here to pay the maker. Unclaimed, the
/// payment returns to the taker after its timelock.
/// @dev ETH is the token `address(0)`. Tokens that take a fee on transfer are refused: the lock
/// must hold exactly the amount the maker checks.
contract TesseraxSwap {
    enum State {
        None,
        Locked,
        Claimed,
        Refunded
    }

    struct Lock {
        address sender;
        address recipient;
        address token;
        uint256 amount;
        bytes32 hash;
        uint64 timelock;
        State state;
    }

    /// @notice Locks by identifier.
    mapping(bytes32 => Lock) public locks;

    /// @notice Locks created, which makes every identifier unique.
    uint256 public count;

    /// @notice A payment was locked for `recipient`, the maker of the Tesserax offer `offer`, by
    /// `sender`, who claims `taker` TESS on Tesserax with the account `taker`.
    event Locked(
        bytes32 indexed id,
        address indexed sender,
        address indexed recipient,
        address token,
        uint256 amount,
        bytes32 hash,
        uint64 timelock,
        uint64 offer,
        bytes32 taker
    );

    /// @notice The payment was sent to its recipient: `secret` is the one of its hash.
    event Claimed(bytes32 indexed id, bytes32 secret);

    /// @notice The payment returned to its sender after its timelock.
    event Refunded(bytes32 indexed id);

    error InvalidLock();
    error WrongValue();
    error FeeOnTransfer();
    error NotLocked();
    error WrongSecret();
    error Expired();
    error NotExpired();
    error TransferFailed();

    /// @notice Lock `amount` of `token` for `recipient` until `timelock`, a unix time, released by
    /// the secret of the SHA-256 `hash`, for the Tesserax offer `offer` and account `taker`.
    function lock(
        address recipient,
        address token,
        uint256 amount,
        bytes32 hash,
        uint64 timelock,
        uint64 offer,
        bytes32 taker
    ) external payable returns (bytes32 id) {
        if (recipient == address(0) || amount == 0 || timelock <= block.timestamp || taker == bytes32(0)) {
            revert InvalidLock();
        }
        if (token == address(0)) {
            if (msg.value != amount) revert WrongValue();
        } else {
            if (msg.value != 0) revert WrongValue();
            uint256 before = _balanceOf(token);
            _call(token, abi.encodeCall(IERC20.transferFrom, (msg.sender, address(this), amount)));
            if (_balanceOf(token) - before != amount) revert FeeOnTransfer();
        }
        id = keccak256(abi.encode(block.chainid, address(this), count++));
        locks[id] = Lock(msg.sender, recipient, token, amount, hash, timelock, State.Locked);
        emit Locked(id, msg.sender, recipient, token, amount, hash, timelock, offer, taker);
    }

    /// @notice Pay lock `id` to its recipient with `secret`, before its timelock. Anyone can call
    /// it: the payment only goes to the recipient.
    function claim(bytes32 id, bytes32 secret) external {
        Lock storage l = locks[id];
        if (l.state != State.Locked) revert NotLocked();
        if (block.timestamp >= l.timelock) revert Expired();
        if (sha256(abi.encodePacked(secret)) != l.hash) revert WrongSecret();
        l.state = State.Claimed;
        emit Claimed(id, secret);
        _send(l.token, l.recipient, l.amount);
    }

    /// @notice Return lock `id` to its sender once its timelock passed. Anyone can call it.
    function refund(bytes32 id) external {
        Lock storage l = locks[id];
        if (l.state != State.Locked) revert NotLocked();
        if (block.timestamp < l.timelock) revert NotExpired();
        l.state = State.Refunded;
        emit Refunded(id);
        _send(l.token, l.sender, l.amount);
    }

    function _send(address token, address to, uint256 amount) private {
        if (token == address(0)) {
            (bool ok,) = to.call{value: amount}("");
            if (!ok) revert TransferFailed();
        } else {
            _call(token, abi.encodeCall(IERC20.transfer, (to, amount)));
        }
    }

    /// @dev A call to a token that succeeds and returns nothing or true, as USDT and USDC do.
    function _call(address token, bytes memory data) private {
        if (token.code.length == 0) revert TransferFailed();
        (bool ok, bytes memory result) = token.call(data);
        if (!ok || (result.length != 0 && !abi.decode(result, (bool)))) revert TransferFailed();
    }

    function _balanceOf(address token) private view returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }
}

interface IERC20 {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function balanceOf(address who) external view returns (uint256);
}
